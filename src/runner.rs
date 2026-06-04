use crate::autopilot::decide_autopilot;
use crate::backend::{
    Backend, LocalDscpBackend, RouterQosdBackend, WinDivertLabBackend, dedupe_candidates,
};
use crate::classifier::Classifier;
use crate::collector::collect_windows_tcp_connections;
use crate::config::Config;
use crate::feedback::{load_feedback_state, save_feedback_state};
use crate::learning::{load_state, now_unix, save_state, update_learning};
use crate::model::{BackendReport, ClassifiedConnection, ConnectionSample, RunReport};
use crate::policy::{BackendKind, PolicyAction, PolicyActionKind};
use crate::receipt::{Receipt, append_receipt};
use anyhow::{Context, Result, anyhow};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

pub fn init_config(path: &Path, force: bool) -> Result<()> {
    if path.exists() && !force {
        return Err(anyhow!(
            "{} already exists; pass --force to overwrite",
            path.display()
        ));
    }
    let config = Config::default_for_current_user();
    fs::write(path, serde_json::to_string_pretty(&config)? + "\n")
        .with_context(|| format!("failed to write config {}", path.display()))?;
    println!("wrote {}", path.display());
    Ok(())
}

pub fn print_sample_table(samples: &[ConnectionSample], config: &Config) {
    let mut state = load_state(&config.state_path).unwrap_or_default();
    let classifier = Classifier::new(config).expect("invalid classifier regex");
    for conn in samples.iter().take(80) {
        let classified = classifier.classify(conn, &state);
        println!(
            "{:<18} {:<6} {:<5} {:<22} {:<12} {}",
            conn.process_name,
            conn.pid,
            conn.remote_port,
            conn.remote_addr,
            format!("{:?}", classified.class).to_lowercase(),
            classified.reason
        );
        update_learning(config, &mut state, &classified);
    }
}

pub fn run_cycle(config: &Config, dry_run: bool) -> Result<RunReport> {
    run_cycle_with_samples(config, collect_windows_tcp_connections()?, dry_run)
}

pub fn run_cycle_with_samples(
    config: &Config,
    samples: Vec<ConnectionSample>,
    dry_run: bool,
) -> Result<RunReport> {
    let mut state = load_state(&config.state_path)?;
    let classifier = Classifier::new(config)?;
    let classified: Vec<_> = samples
        .iter()
        .map(|sample| classifier.classify(sample, &state))
        .collect();
    for item in &classified {
        update_learning(config, &mut state, item);
    }
    save_state(&config.state_path, &state)?;
    let mut feedback = load_feedback_state(&config.policy_state_path)?;
    let autopilot = decide_autopilot(&classified, &feedback, dry_run);
    let mut receipts = Vec::new();
    if !feedback.paused {
        match apply_policy_actions(config, &autopilot.actions, dry_run) {
            Ok(mut applied) => receipts.append(&mut applied),
            Err(error) => {
                feedback.fail_closed(error.to_string(), now_unix());
                save_feedback_state(&config.policy_state_path, &feedback)?;
                return Err(error);
            }
        }
    }
    feedback.set_last_decision(
        autopilot.profile,
        autopilot.confidence,
        autopilot
            .actions
            .iter()
            .map(|action| action.id.clone())
            .collect(),
        autopilot.information.clone(),
        now_unix(),
    );
    save_feedback_state(&config.policy_state_path, &feedback)?;

    let candidates = dedupe_candidates(
        classified
            .iter()
            .filter_map(|item| item.router_candidate.clone()),
    );
    let backend = if config.backends.routerqosd.enabled {
        match apply_router_candidates(config, &candidates, dry_run) {
            Ok((report, mut router_receipts)) => {
                receipts.append(&mut router_receipts);
                report
            }
            Err(error) => {
                let mut feedback = load_feedback_state(&config.policy_state_path)?;
                feedback.fail_closed(error.to_string(), now_unix());
                save_feedback_state(&config.policy_state_path, &feedback)?;
                return Err(error);
            }
        }
    } else {
        BackendReport {
            name: "routerqosd".into(),
            dry_run,
            executed: false,
            ok: true,
            stdout: "backend disabled".into(),
            stderr: String::new(),
        }
    };
    Ok(RunReport {
        updated_unix: now_unix(),
        sample_count: samples.len(),
        class_counts: class_counts(&classified),
        autopilot,
        receipts,
        candidate_count: candidates.len(),
        candidates,
        backend,
    })
}

pub fn class_counts(items: &[ClassifiedConnection]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for item in items {
        *counts
            .entry(format!("{:?}", item.class).to_lowercase())
            .or_default() += 1;
    }
    counts
}

fn apply_policy_actions(
    config: &Config,
    actions: &[PolicyAction],
    dry_run: bool,
) -> Result<Vec<Receipt>> {
    let mut receipts = Vec::new();
    for action in actions {
        if action.kind == PolicyActionKind::ObserveOnly {
            continue;
        }
        let receipt = backend_for_action(config, action.backend, dry_run).apply(action)?;
        append_receipt(&config.receipts_path, &receipt)?;
        receipts.push(receipt);
    }
    Ok(receipts)
}

fn apply_router_candidates(
    config: &Config,
    candidates: &[crate::model::RouterCandidate],
    dry_run: bool,
) -> Result<(BackendReport, Vec<Receipt>)> {
    let mut receipts = Vec::new();
    let backend = RouterQosdBackend::new(config.clone(), dry_run);
    for candidate in candidates {
        let action = PolicyAction::router_ipset(
            format!(
                "routerqosd.{}.{}",
                candidate.set_name,
                sanitize_action_id(&candidate.member)
            ),
            crate::profile::ProfileId::SteamSink,
            candidate.set_name.clone(),
            candidate.member.clone(),
            candidate.reason.clone(),
        );
        let receipt = backend.apply(&action)?;
        append_receipt(&config.receipts_path, &receipt)?;
        receipts.push(receipt);
    }
    Ok((
        BackendReport {
            name: "routerqosd".into(),
            dry_run,
            executed: receipts.iter().any(|receipt| !receipt.dry_run),
            ok: receipts
                .iter()
                .all(|receipt| receipt.status != crate::receipt::ReceiptStatus::Failed),
            stdout: format!("receipts={}", receipts.len()),
            stderr: String::new(),
        },
        receipts,
    ))
}

fn backend_for_action(config: &Config, kind: BackendKind, dry_run: bool) -> Box<dyn Backend + '_> {
    match kind {
        BackendKind::LocalDscp => Box::new(LocalDscpBackend::new(dry_run)),
        BackendKind::RouterQosd => Box::new(RouterQosdBackend::new(config.clone(), dry_run)),
        BackendKind::WinDivertLab => Box::new(WinDivertLabBackend),
    }
}

fn sanitize_action_id(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempFiles {
        paths: Vec<std::path::PathBuf>,
    }

    impl Drop for TempFiles {
        fn drop(&mut self) {
            for path in &self.paths {
                let _ = fs::remove_file(path);
            }
        }
    }

    fn temp_state_config() -> (Config, TempFiles) {
        let mut config = Config::default_for_current_user();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let temp = std::env::temp_dir();
        config.state_path = temp.join(format!("winqos-test-state-{unique}.json"));
        config.policy_state_path = temp.join(format!("winqos-test-policy-{unique}.json"));
        config.feedback_path = temp.join(format!("winqos-test-feedback-{unique}.jsonl"));
        config.receipts_path = temp.join(format!("winqos-test-receipts-{unique}.jsonl"));
        let guard = TempFiles {
            paths: vec![
                config.state_path.clone(),
                config.policy_state_path.clone(),
                config.feedback_path.clone(),
                config.receipts_path.clone(),
            ],
        };
        (config, guard)
    }

    fn sample(process: &str) -> ConnectionSample {
        ConnectionSample {
            pid: 1,
            process_name: process.into(),
            process_path: String::new(),
            protocol: "tcp".into(),
            remote_addr: "8.8.8.8".into(),
            remote_port: 443,
            state: "Established".into(),
        }
    }

    #[test]
    fn run_cycle_reports_disabled_backend_without_executing() {
        let (config, _guard) = temp_state_config();

        let report = run_cycle_with_samples(&config, vec![sample("steam")], true).unwrap();

        assert_eq!(report.sample_count, 1);
        assert_eq!(
            report.autopilot.profile,
            crate::profile::ProfileId::SteamSink
        );
        assert_eq!(report.receipts.len(), 1);
        assert_eq!(
            report.receipts[0].status,
            crate::receipt::ReceiptStatus::DryRun
        );
        assert_eq!(report.candidate_count, 1);
        assert_eq!(report.class_counts.get("bulk"), Some(&1));
        assert_eq!(report.backend.stdout, "backend disabled");
        assert!(!report.backend.executed);
    }
}
