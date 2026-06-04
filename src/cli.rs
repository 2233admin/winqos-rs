use crate::collector::collect_windows_tcp_connections;
use crate::config::{Config, DEFAULT_CONFIG};
use crate::feedback::{FeedbackEvent, load_feedback_state, record_feedback_event};
use crate::learning::{load_state, now_unix};
use crate::petscii::{render_explain, render_status};
use crate::profile::ProfileId;
use crate::runner::{init_config, print_sample_table, run_cycle};
use anyhow::Result;
use clap::{Parser, Subcommand};
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
    }
}
