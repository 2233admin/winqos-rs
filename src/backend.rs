use crate::config::Config;
use crate::model::{BackendReport, RouterCandidate};
use crate::policy::{BackendKind, PolicyAction};
use crate::receipt::{Receipt, RollbackReceipt};
use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::io::Write;
use std::process::{Command, Stdio};

pub mod dscp;
pub mod routerqosd;
pub mod windivert_lab;

pub use dscp::LocalDscpBackend;
pub use routerqosd::RouterQosdBackend;
pub use windivert_lab::WinDivertLabBackend;

pub trait Backend {
    fn name(&self) -> &'static str;
    fn capabilities(&self) -> BackendCapabilities;
    fn inspect(&self) -> Result<BackendStatus>;
    fn apply(&self, action: &PolicyAction) -> Result<Receipt>;
    fn status(&self, action_id: &str) -> Result<ActionStatus>;
    fn remove(&self, action_id: &str) -> Result<RollbackReceipt>;
    fn explain(&self, action_id: &str) -> Result<ActionExplanation>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackendCapabilities {
    pub kind: BackendKind,
    pub can_inspect: bool,
    pub can_apply: bool,
    pub can_remove: bool,
    pub requires_admin: bool,
    pub supports_dry_run: bool,
    pub experimental: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackendStatus {
    pub kind: BackendKind,
    pub available: bool,
    pub elevated: bool,
    pub active_actions: Vec<String>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionState {
    Planned,
    Applied,
    DryRun,
    Removed,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionStatus {
    pub action_id: String,
    pub state: ActionState,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionExplanation {
    pub action_id: String,
    pub summary: String,
    pub details: BTreeMap<String, String>,
}

pub fn dedupe_candidates(items: impl Iterator<Item = RouterCandidate>) -> Vec<RouterCandidate> {
    items.collect::<BTreeSet<_>>().into_iter().collect()
}

pub fn build_routerqosd_script(config: &Config, candidates: &[RouterCandidate]) -> Result<String> {
    let mut script = String::from("set -eu\n");
    for item in candidates {
        let set_name = sanitize_set_name(&item.set_name)?;
        let member = sanitize_member(&item.member)?;
        script.push_str(&format!(
            "ipset add {set_name} {member} timeout {} -exist 2>/dev/null || true\n",
            config.candidate_timeout_seconds
        ));
    }
    script.push_str(&format!("echo ok updates={}\n", candidates.len()));
    Ok(script)
}

pub fn push_routerqosd(
    config: &Config,
    candidates: &[RouterCandidate],
    dry_run: bool,
) -> Result<BackendReport> {
    let backend = &config.backends.routerqosd;
    let script = build_routerqosd_script(config, candidates)?;

    if dry_run {
        return Ok(BackendReport {
            name: "routerqosd".into(),
            dry_run,
            executed: false,
            ok: true,
            stdout: script,
            stderr: String::new(),
        });
    }

    let target = format!("{}@{}", backend.user, backend.host);
    let mut child = Command::new(&backend.ssh_path)
        .arg("-p")
        .arg(backend.port.to_string())
        .arg("-i")
        .arg(&backend.key_path)
        .arg("-o")
        .arg("IdentitiesOnly=yes")
        .arg("-o")
        .arg("BatchMode=yes")
        .arg("-o")
        .arg("ConnectTimeout=8")
        .arg(target)
        .arg("sh -s")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to spawn ssh backend")?;
    child
        .stdin
        .as_mut()
        .context("failed to open ssh stdin")?
        .write_all(script.as_bytes())
        .context("failed to send backend script")?;
    let output = child
        .wait_with_output()
        .context("failed to wait for ssh backend")?;
    Ok(BackendReport {
        name: "routerqosd".into(),
        dry_run,
        executed: true,
        ok: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).trim().into(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().into(),
    })
}

pub fn sanitize_set_name(value: &str) -> Result<String> {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        Ok(value.into())
    } else {
        Err(anyhow!("unsafe set name: {value}"))
    }
}

pub fn sanitize_member(value: &str) -> Result<String> {
    if value.chars().all(|ch| {
        ch.is_ascii_hexdigit() || matches!(ch, ':' | '.' | ',' | 't' | 'c' | 'p' | 'u' | 'd')
    }) {
        Ok(value.into())
    } else {
        Err(anyhow!("unsafe ipset member: {value}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TrafficClass;
    use crate::policy::{ActionSelector, PolicyAction};
    use crate::profile::ProfileId;
    use crate::receipt::{ReceiptStatus, RollbackReceipt};

    #[test]
    fn dedupe_candidates_sorts_and_deduplicates() {
        let items = vec![
            RouterCandidate {
                set_name: "rqosd_ele4".into(),
                member: "8.8.8.8,tcp:443".into(),
                reason: "a".into(),
            },
            RouterCandidate {
                set_name: "rqosd_ele4".into(),
                member: "8.8.8.8,tcp:443".into(),
                reason: "a".into(),
            },
        ];

        assert_eq!(dedupe_candidates(items.into_iter()).len(), 1);
    }

    #[test]
    fn sanitize_rejects_shell_injection() {
        assert!(sanitize_set_name("rqosd_ele4;rm").is_err());
        assert!(sanitize_member("8.8.8.8,tcp:443;rm").is_err());
    }

    #[test]
    fn dry_run_script_contains_timeout_and_update_count() {
        let config = Config::default_for_current_user();
        let script = build_routerqosd_script(
            &config,
            &[RouterCandidate {
                set_name: "rqosd_ele4".into(),
                member: "8.8.8.8,tcp:443".into(),
                reason: "bulk".into(),
            }],
        )
        .unwrap();

        assert!(script.contains("ipset add rqosd_ele4 8.8.8.8,tcp:443 timeout 30 -exist"));
        assert!(script.contains("echo ok updates=1"));
    }

    struct FakeBackend;

    impl Backend for FakeBackend {
        fn name(&self) -> &'static str {
            "fake"
        }

        fn capabilities(&self) -> BackendCapabilities {
            BackendCapabilities {
                kind: BackendKind::LocalDscp,
                can_inspect: true,
                can_apply: true,
                can_remove: true,
                requires_admin: false,
                supports_dry_run: true,
                experimental: false,
            }
        }

        fn inspect(&self) -> Result<BackendStatus> {
            Ok(BackendStatus {
                kind: BackendKind::LocalDscp,
                available: true,
                elevated: true,
                active_actions: Vec::new(),
                message: "ok".into(),
            })
        }

        fn apply(&self, action: &PolicyAction) -> Result<Receipt> {
            Ok(Receipt::dry_run("fake-receipt", action.clone(), 1))
        }

        fn status(&self, action_id: &str) -> Result<ActionStatus> {
            Ok(ActionStatus {
                action_id: action_id.into(),
                state: ActionState::DryRun,
                message: "planned".into(),
            })
        }

        fn remove(&self, action_id: &str) -> Result<RollbackReceipt> {
            Ok(RollbackReceipt::removed(
                "rollback-1",
                action_id,
                BackendKind::LocalDscp,
                2,
            ))
        }

        fn explain(&self, action_id: &str) -> Result<ActionExplanation> {
            Ok(ActionExplanation {
                action_id: action_id.into(),
                summary: "fake explanation".into(),
                details: BTreeMap::new(),
            })
        }
    }

    #[test]
    fn backend_trait_contract_supports_apply_status_remove_explain() {
        let backend: Box<dyn Backend> = Box::new(FakeBackend);
        let action = PolicyAction::dscp_mark(
            "game-dscp",
            ProfileId::GameBoost,
            ActionSelector::TrafficClass {
                class: TrafficClass::Interactive,
            },
            46,
            "protect game flow",
        );

        assert_eq!(backend.name(), "fake");
        assert!(backend.capabilities().can_apply);
        assert!(backend.inspect().unwrap().available);
        assert_eq!(
            backend.apply(&action).unwrap().status,
            ReceiptStatus::DryRun
        );
        assert_eq!(
            backend.status(&action.id).unwrap().state,
            ActionState::DryRun
        );
        assert_eq!(
            backend.remove(&action.id).unwrap().status,
            ReceiptStatus::Removed
        );
        assert_eq!(
            backend.explain(&action.id).unwrap().summary,
            "fake explanation"
        );
    }
}
