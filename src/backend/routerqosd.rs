use super::{
    ActionExplanation, ActionState, ActionStatus, Backend, BackendCapabilities, BackendStatus,
    push_routerqosd,
};
use crate::config::Config;
use crate::model::{RouterCandidate, TrafficClass};
use crate::policy::{ActionValue, BackendKind, PolicyAction, PolicyActionKind};
use crate::profile::ProfileId;
use crate::receipt::{Receipt, ReceiptStatus, RollbackReceipt};
use anyhow::{Result, anyhow};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct RouterQosdBackend {
    config: Config,
    dry_run: bool,
}

impl RouterQosdBackend {
    pub fn new(config: Config, dry_run: bool) -> Self {
        Self { config, dry_run }
    }
}

impl Backend for RouterQosdBackend {
    fn name(&self) -> &'static str {
        "routerqosd"
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            kind: BackendKind::RouterQosd,
            can_inspect: true,
            can_apply: true,
            can_remove: true,
            requires_admin: false,
            supports_dry_run: true,
            experimental: false,
        }
    }

    fn inspect(&self) -> Result<BackendStatus> {
        let router = &self.config.backends.routerqosd;
        Ok(BackendStatus {
            kind: BackendKind::RouterQosd,
            available: router.enabled,
            elevated: false,
            active_actions: Vec::new(),
            message: if router.enabled {
                format!("routerqosd target {}:{}", router.host, router.port)
            } else {
                "routerqosd backend disabled".into()
            },
        })
    }

    fn apply(&self, action: &PolicyAction) -> Result<Receipt> {
        let candidate = candidate_from_action(action)?;
        let report = push_routerqosd(&self.config, &[candidate], self.dry_run)?;
        let mut receipt = if report.executed {
            Receipt::applied(
                format!("routerqosd.{}", action.id),
                action.clone(),
                crate::learning::now_unix(),
            )
        } else {
            Receipt::dry_run(
                format!("routerqosd.{}", action.id),
                action.clone(),
                crate::learning::now_unix(),
            )
        };
        if !report.ok {
            receipt.status = ReceiptStatus::Failed;
        }
        receipt.details.insert("stdout".into(), report.stdout);
        receipt.details.insert("stderr".into(), report.stderr);
        Ok(receipt)
    }

    fn status(&self, action_id: &str) -> Result<ActionStatus> {
        Ok(ActionStatus {
            action_id: action_id.into(),
            state: if self.config.backends.routerqosd.enabled {
                ActionState::Unknown
            } else {
                ActionState::DryRun
            },
            message: "routerqosd uses timed ipset entries; inspect router state for live members"
                .into(),
        })
    }

    fn remove(&self, action_id: &str) -> Result<RollbackReceipt> {
        let mut receipt = RollbackReceipt::removed(
            format!("routerqosd.remove.{action_id}"),
            action_id,
            BackendKind::RouterQosd,
            crate::learning::now_unix(),
        );
        receipt.status = ReceiptStatus::AlreadyClear;
        receipt.details.insert(
            "reason".into(),
            "routerqosd members expire by timeout; explicit removal is a future router command"
                .into(),
        );
        Ok(receipt)
    }

    fn explain(&self, action_id: &str) -> Result<ActionExplanation> {
        let mut details = BTreeMap::new();
        details.insert("backend".into(), self.name().into());
        details.insert(
            "timeout_seconds".into(),
            self.config.candidate_timeout_seconds.to_string(),
        );
        Ok(ActionExplanation {
            action_id: action_id.into(),
            summary: "routerqosd maps selected endpoints into timed ipset members".into(),
            details,
        })
    }
}

fn candidate_from_action(action: &PolicyAction) -> Result<RouterCandidate> {
    if action.backend != BackendKind::RouterQosd || action.kind != PolicyActionKind::RouterIpSet {
        return Err(anyhow!(
            "routerqosd backend only accepts router_ip_set actions"
        ));
    }
    match &action.value {
        ActionValue::IpSet { set_name, member } => Ok(RouterCandidate {
            class: router_candidate_class(action.profile),
            set_name: set_name.clone(),
            member: member.clone(),
            reason: action.reason.clone(),
        }),
        _ => Err(anyhow!("routerqosd action requires an ipset value")),
    }
}

fn router_candidate_class(profile: ProfileId) -> TrafficClass {
    match profile {
        ProfileId::GameBoost | ProfileId::RemoteControlLane | ProfileId::StreamGuard => {
            TrafficClass::Realtime
        }
        ProfileId::AiWorkLane => TrafficClass::Interactive,
        ProfileId::SteamSink => TrafficClass::Bulk,
        ProfileId::ProxySmart | ProfileId::Normal | ProfileId::Paused => TrafficClass::Bulk,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::PolicyAction;
    use crate::profile::ProfileId;
    use crate::receipt::ReceiptStatus;

    #[test]
    fn disabled_router_inspect_is_available_false() {
        let backend = RouterQosdBackend::new(Config::default_for_current_user(), true);

        let status = backend.inspect().unwrap();

        assert!(!status.available);
        assert_eq!(status.message, "routerqosd backend disabled");
    }

    #[test]
    fn dry_run_router_apply_returns_script_receipt() {
        let mut config = Config::default_for_current_user();
        config.backends.routerqosd.enabled = true;
        let backend = RouterQosdBackend::new(config, true);
        let action = PolicyAction::router_ipset(
            "router.bulk",
            ProfileId::SteamSink,
            "rqosd_ele4",
            "8.8.8.8,tcp:443",
            "bulk",
        );

        let receipt = backend.apply(&action).unwrap();

        assert_eq!(receipt.status, ReceiptStatus::DryRun);
        assert!(receipt.details["stdout"].contains("ipset add rqosd_ele4"));
    }
}
