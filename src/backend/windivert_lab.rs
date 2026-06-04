use super::{
    ActionExplanation, ActionState, ActionStatus, Backend, BackendCapabilities, BackendStatus,
};
use crate::policy::{BackendKind, PolicyAction};
use crate::receipt::{Receipt, RollbackReceipt};
use anyhow::{Result, anyhow};
use std::collections::BTreeMap;

#[derive(Debug, Default, Clone)]
pub struct WinDivertLabBackend;

impl Backend for WinDivertLabBackend {
    fn name(&self) -> &'static str {
        "windivert_lab"
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            kind: BackendKind::WinDivertLab,
            can_inspect: true,
            can_apply: false,
            can_remove: false,
            requires_admin: true,
            supports_dry_run: true,
            experimental: true,
        }
    }

    fn inspect(&self) -> Result<BackendStatus> {
        Ok(BackendStatus {
            kind: BackendKind::WinDivertLab,
            available: false,
            elevated: false,
            active_actions: Vec::new(),
            message: "WinDivert lab backend is disabled by default".into(),
        })
    }

    fn apply(&self, _action: &PolicyAction) -> Result<Receipt> {
        Err(anyhow!(
            "WinDivert lab backend is disabled; enable experimental lab mode first"
        ))
    }

    fn status(&self, action_id: &str) -> Result<ActionStatus> {
        Ok(ActionStatus {
            action_id: action_id.into(),
            state: ActionState::Unknown,
            message: "WinDivert lab backend disabled".into(),
        })
    }

    fn remove(&self, _action_id: &str) -> Result<RollbackReceipt> {
        Err(anyhow!("WinDivert lab backend has no active lab actions"))
    }

    fn explain(&self, action_id: &str) -> Result<ActionExplanation> {
        let mut details = BTreeMap::new();
        details.insert("experimental".into(), "true".into());
        details.insert("default_enabled".into(), "false".into());
        Ok(ActionExplanation {
            action_id: action_id.into(),
            summary: "WinDivert is reserved for explicit lab experiments, never default apply"
                .into(),
            details,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::{ActionSelector, PolicyAction};
    use crate::profile::ProfileId;

    #[test]
    fn windivert_lab_is_disabled_and_experimental() {
        let backend = WinDivertLabBackend;
        let caps = backend.capabilities();

        assert!(caps.experimental);
        assert!(!caps.can_apply);
        assert!(!backend.inspect().unwrap().available);
    }

    #[test]
    fn windivert_apply_errors_until_explicitly_enabled() {
        let backend = WinDivertLabBackend;
        let action = PolicyAction::dscp_mark(
            "lab",
            ProfileId::GameBoost,
            ActionSelector::ProcessName {
                name: "game.exe".into(),
            },
            46,
            "lab",
        );

        assert!(backend.apply(&action).is_err());
    }
}
