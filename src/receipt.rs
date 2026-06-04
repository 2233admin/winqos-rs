use crate::policy::{BackendKind, PolicyAction};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TrafficClass;
    use crate::policy::{ActionSelector, PolicyAction};
    use crate::profile::ProfileId;

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
}
