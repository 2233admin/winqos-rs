use crate::config::Config;
use crate::feedback::{load_feedback_state, save_feedback_state};
use crate::model::RunReport;
use crate::runner::run_cycle;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonOptions {
    pub dry_run: bool,
    pub once: bool,
    pub cycles: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct DaemonReport {
    pub cycles_completed: u32,
    pub paused: bool,
    pub last_error: Option<String>,
    pub last_report: Option<RunReport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallPlan {
    pub service_name: String,
    pub command: String,
    pub config_path: PathBuf,
    pub requires_admin: bool,
    pub writes_files: Vec<PathBuf>,
    pub note: String,
}

impl Default for DaemonOptions {
    fn default() -> Self {
        Self {
            dry_run: true,
            once: true,
            cycles: Some(1),
        }
    }
}

pub fn run_daemon(config: &Config, options: &DaemonOptions) -> Result<DaemonReport> {
    let max_cycles = if options.once {
        1
    } else {
        options.cycles.unwrap_or(u32::MAX)
    };
    let mut cycles_completed = 0;
    let mut last_report = None;
    for index in 0..max_cycles {
        match run_cycle(config, options.dry_run) {
            Ok(report) => {
                cycles_completed += 1;
                last_report = Some(report);
            }
            Err(error) => {
                let mut state = load_feedback_state(&config.policy_state_path)?;
                state.fail_closed(error.to_string(), crate::learning::now_unix());
                save_feedback_state(&config.policy_state_path, &state)?;
                return Ok(DaemonReport {
                    cycles_completed,
                    paused: true,
                    last_error: Some(error.to_string()),
                    last_report,
                });
            }
        }
        if index + 1 < max_cycles {
            std::thread::sleep(Duration::from_secs(config.interval_seconds.max(2)));
        }
    }
    let state = load_feedback_state(&config.policy_state_path)?;
    Ok(DaemonReport {
        cycles_completed,
        paused: state.paused,
        last_error: state.last_error,
        last_report,
    })
}

pub fn install_plan(config_path: &Path, config: &Config) -> InstallPlan {
    InstallPlan {
        service_name: "winqos-rs-autopilot".into(),
        command: format!(
            "winqos-rs --config {} daemon run --dry-run",
            config_path.display()
        ),
        config_path: config_path.into(),
        requires_admin: true,
        writes_files: vec![
            config.state_path.clone(),
            config.policy_state_path.clone(),
            config.receipts_path.clone(),
            config.feedback_path.clone(),
        ],
        note: "planning surface only; no service is installed by this command".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_plan_is_non_mutating_and_lists_runtime_files() {
        let config = Config::default_for_current_user();
        let plan = install_plan(Path::new("winqos.json"), &config);

        assert_eq!(plan.service_name, "winqos-rs-autopilot");
        assert!(plan.requires_admin);
        assert!(plan.command.contains("daemon run --dry-run"));
        assert!(plan.writes_files.contains(&config.receipts_path));
        assert!(plan.note.contains("no service is installed"));
    }
}
