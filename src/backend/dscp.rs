use super::{
    ActionExplanation, ActionState, ActionStatus, Backend, BackendCapabilities, BackendStatus,
};
use crate::model::TrafficClass;
use crate::policy::{ActionSelector, ActionValue, BackendKind, PolicyAction, PolicyActionKind};
use crate::receipt::{Receipt, ReceiptStatus, RollbackReceipt};
use crate::security_paths::powershell_path;
use anyhow::{Result, anyhow};
use std::collections::BTreeMap;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct LocalDscpBackend {
    dry_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DscpPlan {
    pub policy_name: String,
    pub apply_script: String,
    pub remove_script: String,
    pub live_supported: bool,
    pub unsupported_reason: Option<String>,
}

impl LocalDscpBackend {
    pub fn new(dry_run: bool) -> Self {
        Self { dry_run }
    }

    pub fn dry_run() -> Self {
        Self::new(true)
    }

    pub fn live() -> Self {
        Self::new(false)
    }

    pub fn build_plan(&self, action: &PolicyAction) -> Result<DscpPlan> {
        build_dscp_plan(action)
    }
}

impl Default for LocalDscpBackend {
    fn default() -> Self {
        Self::dry_run()
    }
}

impl Backend for LocalDscpBackend {
    fn name(&self) -> &'static str {
        "local_dscp"
    }

    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities {
            kind: BackendKind::LocalDscp,
            can_inspect: true,
            can_apply: true,
            can_remove: true,
            requires_admin: true,
            supports_dry_run: true,
            experimental: false,
        }
    }

    fn inspect(&self) -> Result<BackendStatus> {
        Ok(BackendStatus {
            kind: BackendKind::LocalDscp,
            available: powershell_available(),
            elevated: is_elevated().unwrap_or(false),
            active_actions: Vec::new(),
            message: if self.dry_run {
                "local DSCP backend dry-run mode".into()
            } else {
                "local DSCP backend live mode".into()
            },
        })
    }

    fn apply(&self, action: &PolicyAction) -> Result<Receipt> {
        let plan = self.build_plan(action)?;
        if self.dry_run || action.dry_run_only {
            let mut receipt = Receipt::dry_run(
                format!("dry-run.{}", action.id),
                action.clone(),
                crate::learning::now_unix(),
            );
            receipt
                .details
                .insert("apply_script".into(), plan.apply_script);
            receipt
                .details
                .insert("remove_script".into(), plan.remove_script);
            return Ok(receipt);
        }
        if !plan.live_supported {
            return Err(anyhow!(
                "{}",
                plan.unsupported_reason
                    .unwrap_or_else(|| "action is not supported for live DSCP apply".into())
            ));
        }
        if !is_elevated().unwrap_or(false) {
            return Err(anyhow!("local DSCP apply requires an elevated shell"));
        }
        run_powershell(&plan.apply_script)?;
        let mut receipt = Receipt::applied(
            format!("applied.{}", action.id),
            action.clone(),
            crate::learning::now_unix(),
        );
        receipt
            .details
            .insert("policy_name".into(), plan.policy_name);
        receipt
            .details
            .insert("remove_script".into(), plan.remove_script);
        Ok(receipt)
    }

    fn status(&self, action_id: &str) -> Result<ActionStatus> {
        let policy_name = policy_name(action_id);
        if self.dry_run {
            return Ok(ActionStatus {
                action_id: action_id.into(),
                state: ActionState::DryRun,
                message: format!("would inspect NetQosPolicy {policy_name}"),
            });
        }
        let script = format!(
            "if (Get-NetQosPolicy -Name '{}' -ErrorAction SilentlyContinue) {{ 'applied' }} else {{ 'unknown' }}",
            ps_quote(&policy_name)
        );
        let output = run_powershell(&script)?;
        Ok(ActionStatus {
            action_id: action_id.into(),
            state: if output.contains("applied") {
                ActionState::Applied
            } else {
                ActionState::Unknown
            },
            message: output,
        })
    }

    fn remove(&self, action_id: &str) -> Result<RollbackReceipt> {
        let policy_name = policy_name(action_id);
        if self.dry_run {
            return Ok(RollbackReceipt {
                id: format!("dry-run-remove.{action_id}"),
                action_id: action_id.into(),
                backend: BackendKind::LocalDscp,
                status: ReceiptStatus::DryRun,
                created_unix: crate::learning::now_unix(),
                details: BTreeMap::from([
                    ("policy_name".into(), policy_name.clone()),
                    ("remove_script".into(), remove_script(&policy_name)),
                ]),
            });
        }
        if !is_elevated().unwrap_or(false) {
            return Err(anyhow!("local DSCP remove requires an elevated shell"));
        }
        run_powershell(&remove_script(&policy_name))?;
        let mut receipt = RollbackReceipt::removed(
            format!("removed.{action_id}"),
            action_id,
            BackendKind::LocalDscp,
            crate::learning::now_unix(),
        );
        receipt.details.insert("policy_name".into(), policy_name);
        Ok(receipt)
    }

    fn explain(&self, action_id: &str) -> Result<ActionExplanation> {
        let policy_name = policy_name(action_id);
        let mut details = BTreeMap::new();
        details.insert("policy_name".into(), policy_name.clone());
        details.insert("backend".into(), self.name().into());
        details.insert("dry_run".into(), self.dry_run.to_string());
        Ok(ActionExplanation {
            action_id: action_id.into(),
            summary: format!("local DSCP policy {policy_name} managed by winqos-rs"),
            details,
        })
    }
}

pub fn build_dscp_plan(action: &PolicyAction) -> Result<DscpPlan> {
    if action.backend != BackendKind::LocalDscp || action.kind != PolicyActionKind::MarkDscp {
        return Err(anyhow!("local DSCP backend only accepts mark_dscp actions"));
    }
    let dscp = match action.value {
        ActionValue::Dscp { value } if value <= 63 => value,
        ActionValue::Dscp { value } => return Err(anyhow!("invalid DSCP value {value}")),
        _ => return Err(anyhow!("mark_dscp action requires a DSCP value")),
    };
    let policy_name = policy_name(&action.id);
    let remove_script = remove_script(&policy_name);
    let (selector_script, live_supported, unsupported_reason) = selector_script(&action.selector);
    let apply_script = if live_supported {
        format!(
            "New-NetQosPolicy -Name '{}' -DSCPAction {} {} -PolicyStore ActiveStore -ErrorAction Stop",
            ps_quote(&policy_name),
            dscp,
            selector_script
        )
    } else {
        format!(
            "# dry-run only: {}\n# DSCP {} for {}",
            unsupported_reason
                .clone()
                .unwrap_or_else(|| "unsupported selector".into()),
            dscp,
            selector_label(&action.selector)
        )
    };
    Ok(DscpPlan {
        policy_name,
        apply_script,
        remove_script,
        live_supported,
        unsupported_reason,
    })
}

fn selector_script(selector: &ActionSelector) -> (String, bool, Option<String>) {
    match selector {
        ActionSelector::ProcessPath { path } => (
            format!("-AppPathNameMatchCondition '{}'", ps_quote(path)),
            true,
            None,
        ),
        ActionSelector::ProcessName { name } => (
            format!("-AppPathNameMatchCondition '{}'", ps_quote(name)),
            true,
            None,
        ),
        ActionSelector::RemoteEndpoint { .. } => (
            String::new(),
            false,
            Some("remote endpoint selectors are dry-run only until exact Windows QoS destination matching is implemented".into()),
        ),
        ActionSelector::TrafficClass { class } => (
            String::new(),
            false,
            Some(format!(
                "traffic class {} must be resolved to concrete processes before live apply",
                traffic_class_label(*class)
            )),
        ),
        ActionSelector::Profile { .. } | ActionSelector::All => (
            String::new(),
            false,
            Some("broad selectors are inspect/dry-run only for local DSCP".into()),
        ),
    }
}

fn selector_label(selector: &ActionSelector) -> String {
    match selector {
        ActionSelector::TrafficClass { class } => traffic_class_label(*class).into(),
        ActionSelector::ProcessName { name } => name.clone(),
        ActionSelector::ProcessPath { path } => path.clone(),
        ActionSelector::RemoteEndpoint {
            protocol,
            remote_addr,
            remote_port,
        } => format!("{protocol}:{remote_addr}:{remote_port}"),
        ActionSelector::Profile { profile } => profile.as_str().into(),
        ActionSelector::All => "all".into(),
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

fn policy_name(action_id: &str) -> String {
    let suffix: String = action_id
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect();
    format!("WinQoS_{suffix}")
}

fn remove_script(policy_name: &str) -> String {
    format!(
        "Remove-NetQosPolicy -Name '{}' -PolicyStore ActiveStore -Confirm:$false -ErrorAction SilentlyContinue",
        ps_quote(policy_name)
    )
}

fn ps_quote(value: &str) -> String {
    value.replace('\'', "''")
}

fn powershell_available() -> bool {
    Command::new(powershell_path())
        .args(["-NoProfile", "-Command", "$PSVersionTable.PSVersion.Major"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn is_elevated() -> Result<bool> {
    let script = "[Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent(); $p = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent()); $p.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)";
    let output = run_powershell(script)?;
    Ok(output.trim().eq_ignore_ascii_case("true"))
}

fn run_powershell(script: &str) -> Result<String> {
    let output = Command::new(powershell_path())
        .args(["-NoProfile", "-Command", script])
        .output()
        .map_err(|err| anyhow!("failed to run powershell: {err}"))?;
    if !output.status.success() {
        return Err(anyhow!(
            "powershell failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::{ActionSelector, PolicyAction};
    use crate::profile::ProfileId;
    use crate::receipt::ReceiptStatus;

    #[test]
    fn traffic_class_dscp_action_is_dry_run_only() {
        let action = PolicyAction::dscp_mark(
            "game_boost.demote_bulk",
            ProfileId::GameBoost,
            ActionSelector::TrafficClass {
                class: TrafficClass::Bulk,
            },
            8,
            "sink bulk",
        );

        let plan = build_dscp_plan(&action).unwrap();

        assert!(!plan.live_supported);
        assert!(plan.apply_script.contains("dry-run only"));
        assert!(plan.remove_script.contains("Remove-NetQosPolicy"));
    }

    #[test]
    fn process_path_dscp_action_builds_live_policy_script() {
        let action = PolicyAction::dscp_mark(
            "game.exe",
            ProfileId::GameBoost,
            ActionSelector::ProcessPath {
                path: r"C:\Games\game.exe".into(),
            },
            46,
            "protect game",
        );

        let plan = build_dscp_plan(&action).unwrap();

        assert!(plan.live_supported);
        assert!(plan.apply_script.contains("New-NetQosPolicy"));
        assert!(plan.apply_script.contains("-DSCPAction 46"));
        assert!(plan.apply_script.contains("-AppPathNameMatchCondition"));
    }

    #[test]
    fn remote_endpoint_dscp_action_is_dry_run_only() {
        let action = PolicyAction::dscp_mark(
            "remote",
            ProfileId::GameBoost,
            ActionSelector::RemoteEndpoint {
                protocol: "tcp".into(),
                remote_addr: "8.8.8.8".into(),
                remote_port: 443,
            },
            46,
            "protect remote",
        );

        let plan = build_dscp_plan(&action).unwrap();

        assert!(!plan.live_supported);
        assert!(
            plan.unsupported_reason
                .unwrap()
                .contains("remote endpoint selectors are dry-run only")
        );
    }

    #[test]
    fn dry_run_apply_returns_receipt_with_scripts() {
        let backend = LocalDscpBackend::dry_run();
        let action = PolicyAction::dscp_mark(
            "game.exe",
            ProfileId::GameBoost,
            ActionSelector::ProcessName {
                name: "game.exe".into(),
            },
            46,
            "protect game",
        );

        let receipt = backend.apply(&action).unwrap();

        assert_eq!(receipt.status, ReceiptStatus::DryRun);
        assert!(receipt.details["apply_script"].contains("New-NetQosPolicy"));
        assert!(receipt.rollback.ready);
    }

    #[test]
    fn dry_run_remove_returns_dry_run_status() {
        let backend = LocalDscpBackend::dry_run();

        let receipt = backend.remove("game.exe").unwrap();

        assert_eq!(receipt.status, ReceiptStatus::DryRun);
        assert!(receipt.details["remove_script"].contains("Remove-NetQosPolicy"));
    }

    #[test]
    fn rejects_invalid_dscp_value() {
        let mut action = PolicyAction::dscp_mark(
            "bad",
            ProfileId::GameBoost,
            ActionSelector::ProcessName {
                name: "game.exe".into(),
            },
            46,
            "bad",
        );
        action.value = ActionValue::Dscp { value: 64 };

        assert!(build_dscp_plan(&action).is_err());
    }
}
