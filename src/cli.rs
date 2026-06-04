use crate::collector::collect_windows_tcp_connections;
use crate::config::{Config, DEFAULT_CONFIG};
use crate::learning::load_state;
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
    Status,
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
        Commands::Status => {
            let config = Config::load_or_default(&cli.config)?;
            let state = load_state(&config.state_path)?;
            println!("{}", serde_json::to_string_pretty(&state)?);
            Ok(())
        }
    }
}
