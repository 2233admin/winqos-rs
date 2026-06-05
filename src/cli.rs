use crate::adapter::{collect_adapters, plan_for_adapters};
use crate::backend::{Backend, LocalDscpBackend, backend_for_kind};
use crate::collector::collect_windows_connections;
use crate::config::{Config, DEFAULT_CONFIG};
use crate::daemon::{DaemonOptions, install_plan as build_install_plan, run_daemon};
use crate::feedback::{
    FeedbackEvent, load_feedback_state, record_feedback_event, save_feedback_state,
};
use crate::lab::{LabScenario, optimize_latest, run_lab, summarize_lab};
use crate::learning::{load_state, now_unix};
use crate::petscii::{render_explain, render_run_report, render_status};
use crate::policy::{ActionSelector, BackendKind, PolicyAction};
use crate::profile::ProfileId;
use crate::receipt::{append_rollback_receipt, last_apply_receipt};
use crate::runner::{RunMode, init_config, print_sample_table, run_cycle_with_mode};
use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand, ValueEnum};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(name = "winqos")]
#[command(about = "Windows QoS learner and pluggable traffic-classification agent")]
struct Cli {
    #[arg(short, long, default_value = DEFAULT_CONFIG)]
    config: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Init {
        #[arg(long)]
        force: bool,
    },
    Sample {
        #[arg(long)]
        json: bool,
    },
    Run {
        #[arg(long)]
        once: bool,
        #[arg(long)]
        dry_run: bool,
        #[arg(long, value_enum)]
        mode: Option<RunModeArg>,
    },
    Status {
        #[arg(long)]
        json: bool,
    },
    Explain {
        #[arg(long)]
        json: bool,
    },
    Feedback {
        #[command(subcommand)]
        command: FeedbackCommands,
    },
    Pause {
        #[arg(long, default_value = "manual")]
        reason: String,
    },
    Resume,
    Rollback {
        #[arg(long)]
        last: bool,
        #[arg(long)]
        live: bool,
    },
    InstallPlan,
    Lab {
        #[command(subcommand)]
        command: LabCommands,
    },
    Adapters {
        #[command(subcommand)]
        command: AdapterCommands,
    },
    Daemon {
        #[command(subcommand)]
        command: DaemonCommands,
    },
    Backend {
        target: BackendTarget,
        #[command(subcommand)]
        command: BackendCommands,
    },
    Quickstart {
        #[arg(long)]
        live: bool,
        #[arg(long, default_value = "3")]
        cycles: u32,
        #[arg(long)]
        enable_router: bool,
        #[arg(long)]
        router_host: Option<String>,
        #[arg(long)]
        router_user: Option<String>,
        #[arg(long)]
        interval: Option<u64>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand, Debug)]
enum FeedbackCommands {
    Good {
        #[arg(long)]
        last: bool,
    },
    Bad {
        #[arg(long)]
        last: bool,
    },
    Rollback {
        #[arg(long)]
        last: bool,
    },
    IgnoreProcess {
        process_name: String,
        #[arg(long, default_value = "manual")]
        until: String,
    },
    Prefer {
        profile: ProfileId,
    },
}

#[derive(ValueEnum, Clone, Debug, PartialEq, Eq)]
enum BackendTarget {
    Dscp,
    Routerqosd,
    WindivertLab,
}

#[derive(Subcommand, Debug)]
enum BackendCommands {
    Inspect,
    Status {
        action_id: String,
    },
    Remove {
        action_id: String,
        #[arg(long)]
        live: bool,
    },
    Explain {
        action_id: String,
    },
    ApplyDscp {
        action_id: String,
        #[arg(long)]
        dscp: u8,
        #[arg(long)]
        process_path: Option<String>,
        #[arg(long)]
        remote_addr: Option<String>,
        #[arg(long)]
        remote_port: Option<u16>,
        #[arg(long, default_value = "tcp")]
        protocol: String,
        #[arg(long)]
        live: bool,
    },
}

#[derive(Subcommand, Debug)]
enum DaemonCommands {
    Run {
        #[arg(long)]
        once: bool,
        #[arg(long)]
        dry_run: bool,
    },
    InstallPlan,
}

#[derive(ValueEnum, Clone, Debug, PartialEq, Eq)]
enum RunModeArg {
    Observe,
    Assist,
    Live,
}

impl From<RunModeArg> for RunMode {
    fn from(value: RunModeArg) -> Self {
        match value {
            RunModeArg::Observe => RunMode::Observe,
            RunModeArg::Assist => RunMode::Assist,
            RunModeArg::Live => RunMode::Live,
        }
    }
}

#[derive(Subcommand, Debug)]
enum AdapterCommands {
    Inspect,
    Plan,
}

#[derive(Subcommand, Debug)]
enum LabCommands {
    Baseline,
    Run {
        scenario: LabTarget,
    },
    Report,
    Optimize {
        profile: ProfileId,
        #[arg(long)]
        live: bool,
    },
}

#[derive(ValueEnum, Clone, Debug, PartialEq, Eq)]
enum LabTarget {
    Game,
    Stream,
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Init { force } => init_config(&cli.config, force),
        Commands::Sample { json } => {
            let config = Config::load_or_default(&cli.config)?;
            let samples = collect_windows_connections()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&samples)?);
            } else {
                print_sample_table(&samples, &config);
            }
            Ok(())
        }
        Commands::Run {
            once,
            dry_run,
            mode,
        } => {
            let config = Config::load_or_default(&cli.config)?;
            loop {
                let effective_mode = mode
                    .as_ref()
                    .map(|value| RunMode::from(value.clone()))
                    .unwrap_or_else(|| RunMode::from_config(&config.automation.mode));
                let effective_mode = if dry_run {
                    RunMode::Observe
                } else {
                    effective_mode
                };
                let report =
                    run_cycle_with_mode(&config, collect_windows_connections()?, effective_mode)?;
                println!("{}", render_run_report(&report));
                println!("{}", serde_json::to_string_pretty(&report)?);
                if once {
                    break;
                }
                std::thread::sleep(Duration::from_secs(config.interval_seconds.max(2)));
            }
            Ok(())
        }
        Commands::Status { json } => {
            let config = Config::load_or_default(&cli.config)?;
            let learner = load_state(&config.state_path)?;
            let policy = load_feedback_state(&config.policy_state_path)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "learner": learner,
                        "policy": policy,
                    }))?
                );
            } else {
                println!("{}", render_status(&policy));
            }
            Ok(())
        }
        Commands::Explain { json } => {
            let config = Config::load_or_default(&cli.config)?;
            let policy = load_feedback_state(&config.policy_state_path)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&policy)?);
            } else {
                println!("{}", render_explain(&policy));
            }
            Ok(())
        }
        Commands::Feedback { command } => {
            let config = Config::load_or_default(&cli.config)?;
            let event = match command {
                FeedbackCommands::Good { last: _ } => FeedbackEvent::good_last(now_unix()),
                FeedbackCommands::Bad { last: _ } => FeedbackEvent::bad_last(now_unix()),
                FeedbackCommands::Rollback { last: _ } => FeedbackEvent::rollback_last(now_unix()),
                FeedbackCommands::IgnoreProcess {
                    process_name,
                    until,
                } => FeedbackEvent::ignore_process(process_name, until, now_unix()),
                FeedbackCommands::Prefer { profile } => FeedbackEvent::prefer(profile, now_unix()),
            };
            let report = record_feedback_event(&config, event)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        Commands::Pause { reason } => {
            let config = Config::load_or_default(&cli.config)?;
            let mut policy = load_feedback_state(&config.policy_state_path)?;
            policy.pause(reason, now_unix());
            save_feedback_state(&config.policy_state_path, &policy)?;
            println!("{}", render_status(&policy));
            Ok(())
        }
        Commands::Resume => {
            let config = Config::load_or_default(&cli.config)?;
            let mut policy = load_feedback_state(&config.policy_state_path)?;
            policy.resume(now_unix());
            save_feedback_state(&config.policy_state_path, &policy)?;
            println!("{}", render_status(&policy));
            Ok(())
        }
        Commands::Rollback { last, live } => {
            if !last {
                bail!("rollback currently requires --last");
            }
            let config = Config::load_or_default(&cli.config)?;
            rollback_last(&config, live)
        }
        Commands::InstallPlan => {
            let config = Config::load_or_default(&cli.config)?;
            let plan = build_install_plan(&cli.config, &config);
            println!("{}", serde_json::to_string_pretty(&plan)?);
            Ok(())
        }
        Commands::Lab { command } => {
            let config = Config::load_or_default(&cli.config)?;
            match command {
                LabCommands::Baseline => println!(
                    "{}",
                    serde_json::to_string_pretty(&run_lab(&config, LabScenario::Baseline)?)?
                ),
                LabCommands::Run { scenario } => println!(
                    "{}",
                    serde_json::to_string_pretty(&run_lab(&config, scenario.into())?)?
                ),
                LabCommands::Report => println!(
                    "{}",
                    serde_json::to_string_pretty(&summarize_lab(&config.lab_history_path)?)?
                ),
                LabCommands::Optimize { profile, live } => println!(
                    "{}",
                    serde_json::to_string_pretty(&optimize_latest(&config, profile, !live)?)?
                ),
            }
            Ok(())
        }
        Commands::Adapters { command } => {
            let adapters = collect_adapters()?;
            match command {
                AdapterCommands::Inspect => {
                    println!("{}", serde_json::to_string_pretty(&adapters)?);
                }
                AdapterCommands::Plan => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&plan_for_adapters(adapters))?
                    );
                }
            }
            Ok(())
        }
        Commands::Daemon { command } => {
            let config = Config::load_or_default(&cli.config)?;
            match command {
                DaemonCommands::Run { once, dry_run } => {
                    let report = run_daemon(
                        &config,
                        &DaemonOptions {
                            once,
                            dry_run,
                            cycles: None,
                        },
                    )?;
                    println!("{}", serde_json::to_string_pretty(&report)?);
                    Ok(())
                }
                DaemonCommands::InstallPlan => {
                    let plan = build_install_plan(&cli.config, &config);
                    println!("{}", serde_json::to_string_pretty(&plan)?);
                    Ok(())
                }
            }
        }
        Commands::Backend { target, command } => {
            let config = Config::load_or_default(&cli.config)?;
            run_backend_command(&config, target, command)
        }
        Commands::Quickstart {
            live,
            cycles,
            enable_router,
            router_host,
            router_user,
            interval,
            json,
        } => {
            let config = quickstart_prepare_config(
                &cli.config,
                enable_router,
                router_host,
                router_user,
                interval,
            )?;
            let report = run_daemon(
                &config,
                &DaemonOptions {
                    once: false,
                    dry_run: !live,
                    cycles: Some(cycles.max(1)),
                },
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                let install_plan = build_install_plan(&cli.config, &config);
                println!(
                    "quickstart: plan={}\n{}",
                    install_plan.service_name,
                    serde_json::to_string_pretty(&install_plan)?
                );
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into())
                );
            }
            Ok(())
        }
    }
}

fn quickstart_prepare_config(
    path: &PathBuf,
    enable_router: bool,
    router_host: Option<String>,
    router_user: Option<String>,
    interval_seconds: Option<u64>,
) -> Result<Config> {
    if !path.exists() {
        init_config(path, true)?;
    }

    let mut config = Config::load_or_default(path)?;
    let mut modified = false;

    if let Some(interval) = interval_seconds
        && config.interval_seconds != interval.max(2)
    {
        config.interval_seconds = interval.max(2);
        modified = true;
    }

    if enable_router || router_host.is_some() || router_user.is_some() {
        if !config.backends.routerqosd.enabled {
            config.backends.routerqosd.enabled = true;
            modified = true;
        }
        if let Some(host) = router_host
            && config.backends.routerqosd.host != host
        {
            config.backends.routerqosd.host = host;
            modified = true;
        }
        if let Some(user) = router_user
            && config.backends.routerqosd.user != user
        {
            config.backends.routerqosd.user = user;
            modified = true;
        }
    }

    if modified {
        fs::write(path, serde_json::to_string_pretty(&config)? + "\n")
            .with_context(|| format!("failed to write config {}", path.display()))?;
    }

    Ok(config)
}

impl From<LabTarget> for LabScenario {
    fn from(value: LabTarget) -> Self {
        match value {
            LabTarget::Game => Self::Game,
            LabTarget::Stream => Self::Stream,
        }
    }
}

fn run_backend_command(
    config: &Config,
    target: BackendTarget,
    command: BackendCommands,
) -> Result<()> {
    match command {
        BackendCommands::Inspect => {
            let backend = backend_for(config, target, true);
            println!("{}", serde_json::to_string_pretty(&backend.inspect()?)?);
        }
        BackendCommands::Status { action_id } => {
            let backend = backend_for(config, target, true);
            println!(
                "{}",
                serde_json::to_string_pretty(&backend.status(&action_id)?)?
            );
        }
        BackendCommands::Remove { action_id, live } => {
            let backend = backend_for(config, target, !live);
            println!(
                "{}",
                serde_json::to_string_pretty(&backend.remove(&action_id)?)?
            );
        }
        BackendCommands::Explain { action_id } => {
            let backend = backend_for(config, target, true);
            println!(
                "{}",
                serde_json::to_string_pretty(&backend.explain(&action_id)?)?
            );
        }
        BackendCommands::ApplyDscp {
            action_id,
            dscp,
            process_path,
            remote_addr,
            remote_port,
            protocol,
            live,
        } => {
            if target != BackendTarget::Dscp {
                bail!("apply-dscp only targets the local DSCP backend");
            }
            let selector = dscp_selector(process_path, remote_addr, remote_port, protocol)?;
            let action = PolicyAction::dscp_mark(
                action_id,
                ProfileId::Normal,
                selector,
                dscp,
                "manual backend command",
            );
            let backend = if live {
                LocalDscpBackend::live()
            } else {
                LocalDscpBackend::dry_run()
            };
            println!(
                "{}",
                serde_json::to_string_pretty(&backend.apply(&action)?)?
            );
        }
    }
    Ok(())
}

fn backend_for(config: &Config, target: BackendTarget, dry_run: bool) -> Box<dyn Backend> {
    match target {
        BackendTarget::Dscp => backend_for_kind(config, BackendKind::LocalDscp, dry_run),
        BackendTarget::Routerqosd => backend_for_kind(config, BackendKind::RouterQosd, dry_run),
        BackendTarget::WindivertLab => backend_for_kind(config, BackendKind::WinDivertLab, dry_run),
    }
}

fn rollback_last(config: &Config, live: bool) -> Result<()> {
    let receipt = last_apply_receipt(&config.receipts_path)?
        .ok_or_else(|| anyhow!("no apply receipt found to roll back"))?;
    let backend = backend_for_kind(config, receipt.action.backend, !live);
    let rollback = backend.remove(&receipt.action.id)?;
    append_rollback_receipt(&config.receipts_path, &rollback)?;
    println!("{}", serde_json::to_string_pretty(&rollback)?);
    Ok(())
}

fn dscp_selector(
    process_path: Option<String>,
    remote_addr: Option<String>,
    remote_port: Option<u16>,
    protocol: String,
) -> Result<ActionSelector> {
    if let Some(path) = process_path {
        return Ok(ActionSelector::ProcessPath { path });
    }
    match (remote_addr, remote_port) {
        (Some(remote_addr), Some(remote_port)) => Ok(ActionSelector::RemoteEndpoint {
            protocol,
            remote_addr,
            remote_port,
        }),
        _ => Err(anyhow!(
            "apply-dscp requires --process-path or both --remote-addr and --remote-port"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn quickstart_can_enable_router_defaults() -> Result<()> {
        let mut dir = std::env::temp_dir();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        dir.push(format!("winqos-cli-quickstart-{stamp}.json"));
        let config_path = dir;

        let mut config = Config::default_for_current_user();
        config.backends.routerqosd.enabled = false;
        fs::write(&config_path, serde_json::to_string_pretty(&config)? + "\n")?;

        let applied = quickstart_prepare_config(
            &config_path,
            true,
            Some("10.0.0.1".to_string()),
            Some("root2".to_string()),
            Some(4),
        )?;

        let _ = fs::remove_file(&config_path);
        assert!(applied.backends.routerqosd.enabled);
        assert_eq!(applied.backends.routerqosd.host, "10.0.0.1");
        assert_eq!(applied.backends.routerqosd.user, "root2");
        assert_eq!(applied.interval_seconds, 4);
        Ok(())
    }

    #[test]
    fn quickstart_preserves_defaults_without_flags() -> Result<()> {
        let mut dir = std::env::temp_dir();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis()
            + 1;
        dir.push(format!("winqos-cli-quickstart-nochange-{stamp}.json"));
        let config_path = dir;

        let mut config = Config::default_for_current_user();
        config.backends.routerqosd.enabled = false;
        fs::write(&config_path, serde_json::to_string_pretty(&config)? + "\n")?;

        let applied = quickstart_prepare_config(&config_path, false, None, None, None)?;

        let _ = fs::remove_file(&config_path);
        assert!(!applied.backends.routerqosd.enabled);
        assert_eq!(applied.backends.routerqosd.host, "192.168.1.1");
        Ok(())
    }
}
