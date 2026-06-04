use crate::model::TrafficClass;
use crate::profile::ProfileId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendKind {
    LocalDscp,
    RouterQosd,
    WinDivertLab,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyActionKind {
    MarkDscp,
    ProtectFlow,
    DemoteBulk,
    RouterIpSet,
    ObserveOnly,
    PauseAutomation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "selector")]
pub enum ActionSelector {
    TrafficClass {
        class: TrafficClass,
    },
    ProcessName {
        name: String,
    },
    ProcessPath {
        path: String,
    },
    RemoteEndpoint {
        protocol: String,
        remote_addr: String,
        remote_port: u16,
    },
    Profile {
        profile: ProfileId,
    },
    All,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ActionValue {
    Dscp { value: u8 },
    IpSet { set_name: String, member: String },
    Text { value: String },
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyAction {
    pub id: String,
    pub profile: ProfileId,
    pub backend: BackendKind,
    pub kind: PolicyActionKind,
    pub selector: ActionSelector,
    pub value: ActionValue,
    pub reversible: bool,
    pub dry_run_only: bool,
    pub reason: String,
}

impl PolicyAction {
    pub fn new(
        id: impl Into<String>,
        profile: ProfileId,
        backend: BackendKind,
        kind: PolicyActionKind,
        selector: ActionSelector,
        value: ActionValue,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            profile,
            backend,
            kind,
            selector,
            value,
            reversible: true,
            dry_run_only: false,
            reason: reason.into(),
        }
    }

    pub fn dscp_mark(
        id: impl Into<String>,
        profile: ProfileId,
        selector: ActionSelector,
        dscp: u8,
        reason: impl Into<String>,
    ) -> Self {
        Self::new(
            id,
            profile,
            BackendKind::LocalDscp,
            PolicyActionKind::MarkDscp,
            selector,
            ActionValue::Dscp { value: dscp },
            reason,
        )
    }

    pub fn router_ipset(
        id: impl Into<String>,
        profile: ProfileId,
        set_name: impl Into<String>,
        member: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self::new(
            id,
            profile,
            BackendKind::RouterQosd,
            PolicyActionKind::RouterIpSet,
            ActionSelector::All,
            ActionValue::IpSet {
                set_name: set_name.into(),
                member: member.into(),
            },
            reason,
        )
    }

    pub fn observe_only(
        id: impl Into<String>,
        profile: ProfileId,
        backend: BackendKind,
        reason: impl Into<String>,
    ) -> Self {
        let mut action = Self::new(
            id,
            profile,
            backend,
            PolicyActionKind::ObserveOnly,
            ActionSelector::Profile { profile },
            ActionValue::None,
            reason,
        );
        action.reversible = false;
        action.dry_run_only = true;
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dscp_action_is_reversible_by_default() {
        let action = PolicyAction::dscp_mark(
            "game-dscp",
            ProfileId::GameBoost,
            ActionSelector::TrafficClass {
                class: TrafficClass::Interactive,
            },
            46,
            "protect game flow",
        );

        assert_eq!(action.backend, BackendKind::LocalDscp);
        assert_eq!(action.value, ActionValue::Dscp { value: 46 });
        assert!(action.reversible);
        assert!(!action.dry_run_only);
    }
}
