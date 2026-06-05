use crate::policy::{BackendKind, PolicyAction};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptStatus {
    DryRun,
    Applied,
    Failed,
    Removed,
    AlreadyClear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RollbackMethod {
    RemoveAction,
    RestorePrevious,
    AlreadyClear,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rollback {
    pub action_id: String,
    pub backend: BackendKind,
    pub method: RollbackMethod,
    pub ready: bool,
    pub details: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub id: String,
    pub action: PolicyAction,
    pub status: ReceiptStatus,
    pub dry_run: bool,
    pub created_unix: u64,
    pub rollback: Rollback,
    pub details: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RollbackReceipt {
    pub id: String,
    pub action_id: String,
    pub backend: BackendKind,
    pub status: ReceiptStatus,
    pub created_unix: u64,
    pub details: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "record")]
pub enum ReceiptRecord {
    Apply { receipt: Receipt },
    Rollback { receipt: RollbackReceipt },
}

impl Rollback {
    pub fn for_action(action: &PolicyAction, method: RollbackMethod) -> Self {
        Self {
            action_id: action.id.clone(),
            backend: action.backend,
            method,
            ready: action.reversible,
            details: BTreeMap::new(),
        }
    }
}

impl Receipt {
    pub fn dry_run(id: impl Into<String>, action: PolicyAction, created_unix: u64) -> Self {
        Self {
            id: id.into(),
            rollback: Rollback::for_action(&action, RollbackMethod::RemoveAction),
            action,
            status: ReceiptStatus::DryRun,
            dry_run: true,
            created_unix,
            details: BTreeMap::new(),
        }
    }

    pub fn applied(id: impl Into<String>, action: PolicyAction, created_unix: u64) -> Self {
        Self {
            id: id.into(),
            rollback: Rollback::for_action(&action, RollbackMethod::RemoveAction),
            action,
            status: ReceiptStatus::Applied,
            dry_run: false,
            created_unix,
            details: BTreeMap::new(),
        }
    }
}

impl RollbackReceipt {
    pub fn removed(
        id: impl Into<String>,
        action_id: impl Into<String>,
        backend: BackendKind,
        created_unix: u64,
    ) -> Self {
        Self {
            id: id.into(),
            action_id: action_id.into(),
            backend,
            status: ReceiptStatus::Removed,
            created_unix,
            details: BTreeMap::new(),
        }
    }
}

pub fn append_receipt(path: &Path, receipt: &Receipt) -> Result<()> {
    append_record(
        path,
        &ReceiptRecord::Apply {
            receipt: receipt.clone(),
        },
    )
}

pub fn append_rollback_receipt(path: &Path, receipt: &RollbackReceipt) -> Result<()> {
    append_record(
        path,
        &ReceiptRecord::Rollback {
            receipt: receipt.clone(),
        },
    )
}

pub fn load_receipt_records(path: &Path) -> Result<Vec<ReceiptRecord>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = fs::File::open(path)
        .with_context(|| format!("failed to open receipt log {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut records = Vec::new();
    for line in reader.lines() {
        let line = line.with_context(|| format!("failed to read {}", path.display()))?;
        if line.trim().is_empty() {
            continue;
        }
        records.push(
            serde_json::from_str(&line)
                .with_context(|| format!("failed to parse receipt in {}", path.display()))?,
        );
    }
    Ok(records)
}

pub fn last_apply_receipt(path: &Path) -> Result<Option<Receipt>> {
    Ok(load_receipt_records(path)?
        .into_iter()
        .rev()
        .find_map(|record| match record {
            ReceiptRecord::Apply { receipt } => Some(receipt),
            ReceiptRecord::Rollback { .. } => None,
        }))
}

pub fn last_live_apply_receipt(path: &Path) -> Result<Option<Receipt>> {
    Ok(load_receipt_records(path)?
        .into_iter()
        .rev()
        .find_map(|record| match record {
            ReceiptRecord::Apply { receipt }
                if !receipt.dry_run && receipt.status == ReceiptStatus::Applied =>
            {
                Some(receipt)
            }
            _ => None,
        }))
}

pub fn last_apply_receipt_for_action(path: &Path, action_id: &str) -> Result<Option<Receipt>> {
    Ok(load_receipt_records(path)?
        .into_iter()
        .rev()
        .find_map(|record| match record {
            ReceiptRecord::Apply { receipt } if receipt.action.id == action_id => Some(receipt),
            _ => None,
        }))
}

fn append_record(path: &Path, record: &ReceiptRecord) -> Result<()> {
    ensure_parent_dir(path)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open receipt log {}", path.display()))?;
    writeln!(file, "{}", serde_json::to_string(record)?)
        .with_context(|| format!("failed to append receipt log {}", path.display()))?;
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TrafficClass;
    use crate::policy::{ActionSelector, PolicyAction};
    use crate::profile::ProfileId;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn dry_run_receipt_carries_rollback_plan() {
        let action = PolicyAction::dscp_mark(
            "game-dscp",
            ProfileId::GameBoost,
            ActionSelector::TrafficClass {
                class: TrafficClass::Interactive,
            },
            46,
            "protect game flow",
        );

        let receipt = Receipt::dry_run("receipt-1", action, 123);

        assert_eq!(receipt.status, ReceiptStatus::DryRun);
        assert!(receipt.dry_run);
        assert!(receipt.rollback.ready);
        assert_eq!(receipt.rollback.method, RollbackMethod::RemoveAction);
    }

    #[test]
    fn receipt_log_round_trips_apply_and_rollback_records() {
        let action = PolicyAction::dscp_mark(
            "game-dscp",
            ProfileId::GameBoost,
            ActionSelector::TrafficClass {
                class: TrafficClass::Interactive,
            },
            46,
            "protect game flow",
        );
        let receipt = Receipt::dry_run("receipt-1", action, 123);
        let rollback =
            RollbackReceipt::removed("rollback-1", "game-dscp", BackendKind::LocalDscp, 124);
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("winqos-receipts-{unique}.jsonl"));

        append_receipt(&path, &receipt).unwrap();
        append_rollback_receipt(&path, &rollback).unwrap();
        let records = load_receipt_records(&path).unwrap();
        let last = last_apply_receipt(&path).unwrap().unwrap();
        let _ = fs::remove_file(&path);

        assert_eq!(records.len(), 2);
        assert_eq!(last.id, "receipt-1");
    }
}
