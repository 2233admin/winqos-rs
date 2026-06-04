use crate::backend::{Backend, LocalDscpBackend, RouterQosdBackend, WinDivertLabBackend};
use crate::collector::collect_windows_tcp_connections;
use crate::config::{Config, DEFAULT_CONFIG};
use crate::feedback::{FeedbackEvent, load_feedback_state, record_feedback_event};
use crate::learning::{load_state, now_unix};
use crate::petscii::{render_explain, render_status};
use crate::policy::{ActionSelector, PolicyAction};
use crate::profile::ProfileId;
use crate::runner::{init_config, print_sample_table, run_cycle};
use anyhow::{Result, anyhow, bail};
use clap::{Parser, Subcommand, ValueEnum};
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
    Backend {
        target: BackendTarget,
        #[command(subcommand)]
        command: BackendCommands,
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

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Init { force } => init_config(&cli.config, force),
        Commands::Sample { json } => {
            let config = Config::load_or_default(&cli.config)?;
            let samples = collect_windows_tcp_connections()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&samples)?);
            } else {
                print_sample_table(&samples, &config);
            }
            Ok(())
        }
        Commands::Run { once, dry_run } => {
            let config = Config::load_or_default(&cli.config)?;
            loop {
                let report = run_cycle(&config, dry_run)?;
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
        Commands::Backend { target, command } => {
            let config = Config::load_or_default(&cli.config)?;
            run_backend_command(&config, target, command)
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
        BackendTarget::Dscp => Box::new(if dry_run {
            LocalDscpBackend::dry_run()
        } else {
            LocalDscpBackend::live()
        }),
        BackendTarget::Routerqosd => Box::new(RouterQosdBackend::new(config.clone(), dry_run)),
        BackendTarget::WindivertLab => Box::new(WinDivertLabBackend),
    }
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
