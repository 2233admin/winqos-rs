use crate::backend::backend_for_kind;
use crate::config::Config;
use crate::model::RunReport;
use crate::profile::ProfileId;
use crate::receipt::{RollbackReceipt, append_rollback_receipt, last_apply_receipt};
use crate::runner::run_cycle;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabScenario {
    Baseline,
    Game,
    Stream,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabMetrics {
    pub latency_avg_ms: Option<f32>,
    pub latency_p95_ms: Option<f32>,
    pub jitter_ms: Option<f32>,
    pub packet_loss_pct: Option<f32>,
    pub download_active: bool,
    pub upload_pressure: bool,
    pub profile_confidence: f32,
    pub actions_applied: usize,
    pub rollback_ready: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabReport {
    pub id: String,
    pub created_unix: u64,
    pub scenario: LabScenario,
    pub profile: ProfileId,
    pub metrics: LabMetrics,
    pub score: f32,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabSummary {
    pub report_count: usize,
    pub latest: Option<LabReport>,
    pub best: Option<LabReport>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OptimizerDecision {
    pub profile: ProfileId,
    pub accepted: bool,
    pub candidate_score: f32,
    pub best_score: Option<f32>,
    pub rollback: Option<RollbackReceipt>,
    pub reason: String,
}

pub fn run_lab(config: &Config, scenario: LabScenario) -> Result<LabReport> {
    let run_report = run_cycle(config, true)?;
    let report = report_from_run(scenario, &run_report);
    append_lab_report(&config.lab_history_path, &report)?;
    Ok(report)
}

pub fn report_from_run(scenario: LabScenario, run_report: &RunReport) -> LabReport {
    let metrics = LabMetrics {
        latency_avg_ms: None,
        latency_p95_ms: None,
        jitter_ms: None,
        packet_loss_pct: None,
        download_active: run_report
            .class_counts
            .get("bulk")
            .copied()
            .unwrap_or_default()
            > 0,
        upload_pressure: false,
        profile_confidence: run_report.autopilot.confidence,
        actions_applied: run_report.receipts.len(),
        rollback_ready: run_report
            .receipts
            .iter()
            .all(|receipt| receipt.rollback.ready),
    };
    let score = score_metrics(&metrics);
    LabReport {
        id: format!("lab.{}", run_report.updated_unix),
        created_unix: run_report.updated_unix,
        scenario,
        profile: run_report.autopilot.profile,
        metrics,
        score,
        notes: vec![
            "latency probes are inconclusive in phase 1 unless explicit targets are added".into(),
            format!("profile {}", run_report.autopilot.profile.as_str()),
        ],
    }
}

pub fn score_metrics(metrics: &LabMetrics) -> f32 {
    let latency_score = metrics
        .latency_avg_ms
        .map(|value| (100.0 - value.min(100.0)).max(0.0))
        .unwrap_or(0.0);
    let jitter_score = metrics
        .jitter_ms
        .map(|value| (30.0 - value.min(30.0)).max(0.0))
        .unwrap_or(0.0);
    let loss_penalty = metrics.packet_loss_pct.unwrap_or(0.0) * 10.0;
    latency_score
        + jitter_score
        + metrics.profile_confidence * 20.0
        + metrics.actions_applied as f32 * 2.0
        + if metrics.rollback_ready { 5.0 } else { -15.0 }
        - loss_penalty
}

pub fn append_lab_report(path: &Path, report: &LabReport) -> Result<()> {
    ensure_parent_dir(path)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open lab history {}", path.display()))?;
    writeln!(file, "{}", serde_json::to_string(report)?)
        .with_context(|| format!("failed to append lab history {}", path.display()))?;
    Ok(())
}

pub fn load_lab_reports(path: &Path) -> Result<Vec<LabReport>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = fs::File::open(path)
        .with_context(|| format!("failed to open lab history {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut reports = Vec::new();
    for line in reader.lines() {
        let line = line.with_context(|| format!("failed to read {}", path.display()))?;
        if line.trim().is_empty() {
            continue;
        }
        reports.push(
            serde_json::from_str(&line)
                .with_context(|| format!("failed to parse lab report in {}", path.display()))?,
        );
    }
    Ok(reports)
}

pub fn summarize_lab(path: &Path) -> Result<LabSummary> {
    let reports = load_lab_reports(path)?;
    let latest = reports.last().cloned();
    let best = reports
        .iter()
        .cloned()
        .max_by(|left, right| compare_score(left.score, right.score));
    Ok(LabSummary {
        report_count: reports.len(),
        latest,
        best,
    })
}

pub fn optimize_latest(
    config: &Config,
    profile: ProfileId,
    dry_run: bool,
) -> Result<OptimizerDecision> {
    let latest = summarize_lab(&config.lab_history_path)?
        .latest
        .ok_or_else(|| {
            anyhow::anyhow!("no lab report available; run lab baseline or lab run first")
        })?;
    optimize_report(config, profile, latest, dry_run)
}

pub fn optimize_report(
    config: &Config,
    profile: ProfileId,
    candidate: LabReport,
    dry_run: bool,
) -> Result<OptimizerDecision> {
    let paths = OptimizerPaths::new(&config.profiles_dir, profile);
    ensure_parent_dir(&paths.current)?;
    fs::write(
        &paths.current,
        serde_json::to_string_pretty(&candidate)? + "\n",
    )
    .with_context(|| format!("failed to write {}", paths.current.display()))?;
    append_optimizer_history(&paths.history, &candidate)?;

    let best = load_best_report(&paths.best)?;
    let accepted = best
        .as_ref()
        .map(|best| candidate.score > best.score)
        .unwrap_or(true);
    if accepted {
        fs::write(
            &paths.best,
            serde_json::to_string_pretty(&candidate)? + "\n",
        )
        .with_context(|| format!("failed to write {}", paths.best.display()))?;
        Ok(OptimizerDecision {
            profile,
            accepted: true,
            candidate_score: candidate.score,
            best_score: best.map(|best| best.score),
            rollback: None,
            reason: "candidate kept because score improved or no best existed".into(),
        })
    } else {
        let rollback = rollback_last(config, dry_run)?;
        Ok(OptimizerDecision {
            profile,
            accepted: false,
            candidate_score: candidate.score,
            best_score: best.map(|best| best.score),
            rollback,
            reason: "candidate rejected and rollback attempted because score was not better".into(),
        })
    }
}

fn load_best_report(path: &Path) -> Result<Option<LabReport>> {
    if !path.exists() {
        return Ok(None);
    }
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok(Some(serde_json::from_str(&text).with_context(|| {
        format!("failed to parse {}", path.display())
    })?))
}

fn append_optimizer_history(path: &Path, report: &LabReport) -> Result<()> {
    ensure_parent_dir(path)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    writeln!(file, "{}", serde_json::to_string(report)?)
        .with_context(|| format!("failed to append {}", path.display()))?;
    Ok(())
}

fn rollback_last(config: &Config, dry_run: bool) -> Result<Option<RollbackReceipt>> {
    let Some(receipt) = last_apply_receipt(&config.receipts_path)? else {
        return Ok(None);
    };
    let rollback =
        backend_for_kind(config, receipt.action.backend, dry_run).remove(&receipt.action.id)?;
    append_rollback_receipt(&config.receipts_path, &rollback)?;
    Ok(Some(rollback))
}

fn compare_score(left: f32, right: f32) -> Ordering {
    left.partial_cmp(&right).unwrap_or(Ordering::Equal)
}

fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    Ok(())
}

struct OptimizerPaths {
    current: PathBuf,
    best: PathBuf,
    history: PathBuf,
}

impl OptimizerPaths {
    fn new(root: &Path, profile: ProfileId) -> Self {
        let profile = profile.as_str();
        Self {
            current: root.join(format!("{profile}.current.json")),
            best: root.join(format!("{profile}.best.json")),
            history: root.join(format!("{profile}.history.jsonl")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::{ActionSelector, PolicyAction};
    use crate::receipt::{Receipt, ReceiptStatus, append_receipt};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn report(id: &str, score: f32) -> LabReport {
        LabReport {
            id: id.into(),
            created_unix: 1,
            scenario: LabScenario::Game,
            profile: ProfileId::GameBoost,
            metrics: LabMetrics {
                latency_avg_ms: Some(20.0),
                latency_p95_ms: Some(30.0),
                jitter_ms: Some(4.0),
                packet_loss_pct: Some(0.0),
                download_active: true,
                upload_pressure: false,
                profile_confidence: 0.9,
                actions_applied: 1,
                rollback_ready: true,
            },
            score,
            notes: Vec::new(),
        }
    }

    fn temp_config() -> Config {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("winqos-lab-{unique}"));
        let mut config = Config::default_for_current_user();
        config.lab_history_path = root.join("lab.jsonl");
        config.profiles_dir = root.join("profiles");
        config.receipts_path = root.join("receipts.jsonl");
        config
    }

    #[test]
    fn lab_history_summarizes_latest_and_best() {
        let config = temp_config();
        append_lab_report(&config.lab_history_path, &report("a", 10.0)).unwrap();
        append_lab_report(&config.lab_history_path, &report("b", 15.0)).unwrap();

        let summary = summarize_lab(&config.lab_history_path).unwrap();

        assert_eq!(summary.report_count, 2);
        assert_eq!(summary.latest.unwrap().id, "b");
        assert_eq!(summary.best.unwrap().id, "b");
        let _ = fs::remove_dir_all(config.lab_history_path.parent().unwrap());
    }

    #[test]
    fn optimizer_keeps_better_candidate() {
        let config = temp_config();
        optimize_report(&config, ProfileId::GameBoost, report("a", 10.0), true).unwrap();

        let decision =
            optimize_report(&config, ProfileId::GameBoost, report("b", 20.0), true).unwrap();

        assert!(decision.accepted);
        assert_eq!(decision.best_score, Some(10.0));
        let _ = fs::remove_dir_all(config.profiles_dir.parent().unwrap());
    }

    #[test]
    fn optimizer_rejects_worse_candidate_and_rolls_back_last_receipt() {
        let config = temp_config();
        optimize_report(&config, ProfileId::GameBoost, report("best", 20.0), true).unwrap();
        let action = PolicyAction::dscp_mark(
            "candidate",
            ProfileId::GameBoost,
            ActionSelector::ProcessName {
                name: "game.exe".into(),
            },
            46,
            "candidate",
        );
        let receipt = Receipt::dry_run("candidate", action, 1);
        append_receipt(&config.receipts_path, &receipt).unwrap();

        let decision =
            optimize_report(&config, ProfileId::GameBoost, report("worse", 10.0), true).unwrap();

        assert!(!decision.accepted);
        assert_eq!(decision.rollback.unwrap().status, ReceiptStatus::DryRun);
        let _ = fs::remove_dir_all(config.profiles_dir.parent().unwrap());
    }
}
