use crate::autopilot::decide_autopilot;
use crate::backend::{backend_for_kind, dedupe_candidates};
use crate::classifier::Classifier;
use crate::collector::collect_windows_connections;
use crate::config::{AutomationMode, Config};
use crate::feedback::{load_feedback_state, save_feedback_state};
use crate::learning::{load_state, now_unix, save_state, update_learning};
use crate::model::{
    BackendReport, ClassifiedConnection, ConnectionSample, RunReport, TrafficClass,
};
use crate::policy::{ActionSelector, BackendKind, PolicyAction, PolicyActionKind};
use crate::profile::ProfileId;
use crate::receipt::{
    Receipt, ReceiptStatus, append_receipt, append_rollback_receipt, last_apply_receipt_for_action,
};
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
    run_cycle_with_samples(config, collect_windows_connections()?, dry_run)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    Observe,
    Assist,
    Live,
}

impl RunMode {
    pub fn from_config(automation_mode: &AutomationMode) -> Self {
        match automation_mode {
            AutomationMode::ObserveOnly => Self::Observe,
            AutomationMode::Assist => Self::Assist,
            AutomationMode::Live => Self::Live,
        }
    }
}

pub fn run_cycle_with_mode(
    config: &Config,
    samples: Vec<ConnectionSample>,
    mode: RunMode,
) -> Result<RunReport> {
    run_cycle_with_mode_and_flags(config, samples, mode, false)
}

fn resolve_mode_from_request(config: &Config, dry_run: bool) -> RunMode {
    if dry_run {
        return RunMode::Observe;
    }
    RunMode::from_config(&config.automation.mode)
}

pub fn run_cycle_with_samples(
    config: &Config,
    samples: Vec<ConnectionSample>,
    dry_run: bool,
) -> Result<RunReport> {
    run_cycle_with_mode_and_flags(
        config,
        samples,
        resolve_mode_from_request(config, dry_run),
        false,
    )
}

fn run_cycle_with_mode_and_flags(
    config: &Config,
    samples: Vec<ConnectionSample>,
    mode: RunMode,
    force_apply: bool,
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
    let mut autopilot = decide_autopilot(&classified, &feedback, mode == RunMode::Observe);

    if matches!(mode, RunMode::Observe) {
        feedback.clear_auto_observation();
    }

    let now = now_unix();
    let mut should_apply = matches!(mode, RunMode::Live);
    if !feedback.paused && matches!(mode, RunMode::Assist) {
        feedback.observe_for_assist(
            autopilot.profile,
            autopilot.confidence,
            config.automation.min_confidence,
            config.automation.observation_cycles,
        );
        should_apply = feedback.assist_should_apply(
            config.automation.min_confidence,
            config.automation.observation_cycles,
        ) || force_apply;
    } else if feedback.paused {
        feedback.clear_auto_observation();
    }

    let mut rolled_back = false;
    if feedback.last_applied_profile.is_some() && !force_apply {
        let stale_window = feedback.last_applied_unix != 0
            && now.saturating_sub(feedback.last_applied_unix)
                <= config.automation.auto_rollback_seconds;
        if stale_window {
            let regression = feedback.last_applied_confidence.is_sign_positive()
                && (feedback.last_applied_confidence - autopilot.confidence) > 0.2;
            let profile_shift = feedback.last_applied_profile != Some(autopilot.profile);
            if regression && profile_shift {
                rolled_back = rollback_last_applied_batch(config, &mut feedback, now)?;
            }
        }
    }
    if rolled_back {
        should_apply = false;
    }
    let effective_dry_run = !should_apply;
    autopilot.dry_run = effective_dry_run;

    let mut receipts = Vec::new();
    if !feedback.paused {
        let resolved_actions = resolve_policy_actions(&autopilot.actions, &classified);
        match apply_policy_actions(config, &resolved_actions, effective_dry_run) {
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
        now,
    );
    if should_apply {
        let applied_action_ids = receipts
            .iter()
            .filter(|receipt| !receipt.dry_run && receipt.status == ReceiptStatus::Applied)
            .map(|receipt| receipt.action.id.clone())
            .collect::<Vec<_>>();
        feedback.remember_live_apply(
            autopilot.profile,
            autopilot.confidence,
            applied_action_ids,
            now,
        );
    } else {
        let apply_tracking_expired = feedback.last_applied_unix != 0
            && now.saturating_sub(feedback.last_applied_unix)
                > config.automation.auto_rollback_seconds;
        if matches!(mode, RunMode::Observe) || apply_tracking_expired {
            feedback.clear_last_apply_tracking();
        }
        if !rolled_back && (matches!(mode, RunMode::Observe) || matches!(mode, RunMode::Assist)) {
            feedback.last_error = None;
        }
    }
    save_feedback_state(&config.policy_state_path, &feedback)?;

    let candidates = dedupe_candidates(
        classified
            .iter()
            .filter_map(|item| item.router_candidate.clone()),
    );
    let backend = if config.backends.routerqosd.enabled {
        match apply_router_candidates(config, &candidates, effective_dry_run, autopilot.profile) {
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
            dry_run: effective_dry_run,
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

fn rollback_last_applied_batch(
    config: &Config,
    feedback: &mut crate::feedback::FeedbackState,
    now: u64,
) -> Result<bool> {
    if feedback.last_applied_action_ids.is_empty() {
        feedback.clear_last_apply_tracking();
        return Ok(false);
    }

    let mut rolled = false;
    for action_id in feedback.last_applied_action_ids.clone() {
        let Some(receipt) = last_apply_receipt_for_action(&config.receipts_path, &action_id)?
        else {
            continue;
        };
        if receipt.dry_run || receipt.status != ReceiptStatus::Applied {
            continue;
        }
        if now.saturating_sub(receipt.created_unix) > config.automation.auto_rollback_seconds {
            continue;
        }
        let backend = backend_for_kind(config, receipt.action.backend, false);
        let rollback = backend.remove(&action_id)?;
        append_rollback_receipt(&config.receipts_path, &rollback)?;
        rolled = true;
    }

    if rolled {
        feedback.last_error = Some("auto-rollback triggered by unstable confidence".into());
    }
    feedback.clear_last_apply_tracking();
    Ok(rolled)
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
        let receipt = backend_for_kind(config, action.backend, dry_run).apply(action)?;
        append_receipt(&config.receipts_path, &receipt)?;
        receipts.push(receipt);
    }
    Ok(receipts)
}

fn resolve_policy_actions(
    actions: &[PolicyAction],
    classified: &[ClassifiedConnection],
) -> Vec<PolicyAction> {
    let mut resolved = Vec::new();
    for action in actions {
        if action.backend != BackendKind::LocalDscp || action.kind != PolicyActionKind::MarkDscp {
            resolved.push(action.clone());
            continue;
        }

        let ActionSelector::TrafficClass { class } = &action.selector else {
            resolved.push(action.clone());
            continue;
        };
        let class = *class;

        let mut selectors = BTreeMap::new();
        for item in classified.iter().filter(|item| item.class == class) {
            if let Some((key, selector)) = concrete_selector_for_process(item) {
                selectors.entry(key).or_insert(selector);
            }
        }

        if selectors.is_empty() {
            let mut action = action.clone();
            action.dry_run_only = true;
            action.reason = format!(
                "{}; unresolved {} class has no concrete process selector",
                action.reason,
                traffic_class_label(class)
            );
            resolved.push(action);
            continue;
        }

        for (key, selector) in selectors.into_iter().take(32) {
            let mut action = action.clone();
            action.id = format!("{}.{}", action.id, sanitize_action_id(&key));
            action.selector = selector;
            action.reason = format!(
                "{}; resolved {} class to {}",
                action.reason,
                traffic_class_label(class),
                key
            );
            resolved.push(action);
        }
    }
    resolved
}

fn concrete_selector_for_process(item: &ClassifiedConnection) -> Option<(String, ActionSelector)> {
    let path = item.sample.process_path.trim();
    if !path.is_empty() {
        return Some((
            path.into(),
            ActionSelector::ProcessPath { path: path.into() },
        ));
    }
    let name = item.sample.process_name.trim();
    if !name.is_empty() {
        return Some((
            name.into(),
            ActionSelector::ProcessName { name: name.into() },
        ));
    }
    None
}

fn apply_router_candidates(
    config: &Config,
    candidates: &[crate::model::RouterCandidate],
    dry_run: bool,
    active_profile: ProfileId,
) -> Result<(BackendReport, Vec<Receipt>)> {
    let mut receipts = Vec::new();
    for candidate in candidates {
        let action = PolicyAction::router_ipset(
            format!(
                "routerqosd.{}.{}",
                candidate.set_name,
                sanitize_action_id(&candidate.member)
            ),
            profile_for_router_candidate(candidate.class, active_profile),
            candidate.set_name.clone(),
            candidate.member.clone(),
            candidate.reason.clone(),
        );
        let receipt = backend_for_kind(config, action.backend, dry_run).apply(&action)?;
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

fn sanitize_action_id(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect()
}

fn profile_for_router_candidate(class: TrafficClass, active_profile: ProfileId) -> ProfileId {
    match active_profile {
        ProfileId::GameBoost
        | ProfileId::StreamGuard
        | ProfileId::RemoteControlLane
        | ProfileId::ProxySmart
        | ProfileId::AiWorkLane => active_profile,
        ProfileId::SteamSink if class == TrafficClass::Bulk => ProfileId::SteamSink,
        ProfileId::SteamSink | ProfileId::Normal | ProfileId::Paused => match class {
            TrafficClass::Realtime => ProfileId::RemoteControlLane,
            TrafficClass::Interactive => ProfileId::AiWorkLane,
            TrafficClass::Bulk => ProfileId::SteamSink,
            TrafficClass::Normal | TrafficClass::Ignore => ProfileId::Normal,
        },
    }
}

fn traffic_class_label(class: TrafficClass) -> &'static str {
    match class {
        TrafficClass::Realtime => "realtime",
        TrafficClass::Interactive => "interactive",
        TrafficClass::Normal => "normal",
        TrafficClass::Bulk => "bulk",
        TrafficClass::Ignore => "ignore",
    }
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

    fn sample_with(process: &str, path: &str, protocol: &str) -> ConnectionSample {
        ConnectionSample {
            pid: 2,
            process_name: process.into(),
            process_path: path.into(),
            protocol: protocol.into(),
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

    #[test]
    fn ai_work_actions_resolve_to_concrete_process_policy() {
        let (config, _guard) = temp_state_config();

        let report = run_cycle_with_samples(
            &config,
            vec![sample_with("ChatGPT", r"C:\\Apps\\ChatGPT.exe", "tcp")],
            true,
        )
        .unwrap();

        assert_eq!(
            report.autopilot.profile,
            crate::profile::ProfileId::AiWorkLane
        );
        assert!(report.receipts[0].action.id.contains("ChatGPT"));
        assert!(matches!(
            report.receipts[0].action.selector,
            ActionSelector::ProcessPath { .. }
        ));
        assert!(report.receipts[0].details["apply_script"].contains("-AppPathNameMatchCondition"));
    }

    #[test]
    fn remote_control_udp_resolves_to_realtime_policy_and_router_class() {
        let (config, _guard) = temp_state_config();

        let report = run_cycle_with_samples(
            &config,
            vec![sample_with("parsec", r"C:\\Parsec\\parsec.exe", "udp")],
            true,
        )
        .unwrap();

        assert_eq!(
            report.autopilot.profile,
            crate::profile::ProfileId::RemoteControlLane
        );
        assert_eq!(report.class_counts.get("realtime"), Some(&1));
        assert_eq!(report.candidates[0].class, TrafficClass::Realtime);
        assert_eq!(report.candidates[0].set_name, "rqosd_rt4");
        assert!(matches!(
            report.receipts[0].action.selector,
            ActionSelector::ProcessPath { .. }
        ));
    }

    #[test]
    fn assist_observation_preserves_recent_live_apply_tracking() {
        let (mut config, _guard) = temp_state_config();
        config.automation.mode = AutomationMode::Assist;
        config.automation.observation_cycles = 3;
        config.automation.auto_rollback_seconds = 120;
        let now = now_unix();
        let mut feedback = crate::feedback::FeedbackState::default();
        feedback.set_last_decision(
            crate::profile::ProfileId::RemoteControlLane,
            0.95,
            vec!["winqos-dscp-remote-control".into()],
            vec![],
            now.saturating_sub(1),
        );
        feedback.remember_live_apply(
            crate::profile::ProfileId::RemoteControlLane,
            0.95,
            vec!["winqos-dscp-remote-control".into()],
            now.saturating_sub(1),
        );
        save_feedback_state(&config.policy_state_path, &feedback).unwrap();

        let report = run_cycle_with_mode(
            &config,
            vec![sample_with("parsec", r"C:\\Parsec\\parsec.exe", "udp")],
            RunMode::Assist,
        )
        .unwrap();

        assert!(report.autopilot.dry_run);
        let saved = load_feedback_state(&config.policy_state_path).unwrap();
        assert_eq!(
            saved.last_applied_profile,
            Some(crate::profile::ProfileId::RemoteControlLane)
        );
        assert_eq!(
            saved.last_applied_action_ids,
            vec!["winqos-dscp-remote-control".to_string()]
        );
    }
}
