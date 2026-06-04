use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_CONFIG: &str = "winqos.json";
pub const DEFAULT_STATE: &str = "winqos-state.json";
pub const DEFAULT_RECEIPTS: &str = "winqos-receipts.jsonl";
pub const DEFAULT_FEEDBACK: &str = "winqos-feedback.jsonl";
pub const DEFAULT_POLICY_STATE: &str = "winqos-policy-state.json";
pub const DEFAULT_LAB_HISTORY: &str = "winqos-lab-history.jsonl";
pub const DEFAULT_PROFILES_DIR: &str = "profiles";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_state_path")]
    pub state_path: PathBuf,
    #[serde(default = "default_receipts_path")]
    pub receipts_path: PathBuf,
    #[serde(default = "default_feedback_path")]
    pub feedback_path: PathBuf,
    #[serde(default = "default_policy_state_path")]
    pub policy_state_path: PathBuf,
    #[serde(default = "default_lab_history_path")]
    pub lab_history_path: PathBuf,
    #[serde(default = "default_profiles_dir")]
    pub profiles_dir: PathBuf,
    pub interval_seconds: u64,
    pub candidate_timeout_seconds: u32,
    pub learning: LearningConfig,
    pub classifier: ClassifierConfig,
    pub backends: BackendConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningConfig {
    pub enabled: bool,
    pub learn_bulk_after_score: i32,
    pub score_increment_for_bulk_hint: i32,
    pub score_increment_for_many_connections: i32,
    pub score_decrement_for_interactive_hint: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifierConfig {
    #[serde(default = "default_realtime_process_patterns")]
    pub realtime_process_patterns: Vec<String>,
    #[serde(default = "default_remote_control_process_patterns")]
    pub remote_control_process_patterns: Vec<String>,
    #[serde(default = "default_ai_work_process_patterns")]
    pub ai_work_process_patterns: Vec<String>,
    #[serde(default = "default_proxy_process_patterns")]
    pub proxy_process_patterns: Vec<String>,
    pub bulk_process_patterns: Vec<String>,
    pub interactive_process_patterns: Vec<String>,
    pub ignore_process_patterns: Vec<String>,
    pub bulk_name_patterns: Vec<String>,
    pub bulk_ports: Vec<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendConfig {
    pub routerqosd: RouterQosdConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterQosdConfig {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub key_path: PathBuf,
    pub ssh_path: PathBuf,
    #[serde(default)]
    pub class_sets: RouterClassSets,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouterClassSets {
    pub realtime_v4: String,
    pub interactive_v4: String,
    pub bulk_v4: String,
    pub realtime_v6: String,
    pub interactive_v6: String,
    pub bulk_v6: String,
}

impl Default for RouterClassSets {
    fn default() -> Self {
        Self {
            realtime_v4: "rqosd_rt4".into(),
            interactive_v4: "rqosd_hi4".into(),
            bulk_v4: "rqosd_ele4".into(),
            realtime_v6: "rqosd_rt6".into(),
            interactive_v6: "rqosd_hi6".into(),
            bulk_v6: "rqosd_ele6".into(),
        }
    }
}

impl RouterClassSets {
    pub fn set_name(&self, class: crate::model::TrafficClass, ipv6: bool) -> Option<&str> {
        match (class, ipv6) {
            (crate::model::TrafficClass::Realtime, false) => Some(&self.realtime_v4),
            (crate::model::TrafficClass::Realtime, true) => Some(&self.realtime_v6),
            (crate::model::TrafficClass::Interactive, false) => Some(&self.interactive_v4),
            (crate::model::TrafficClass::Interactive, true) => Some(&self.interactive_v6),
            (crate::model::TrafficClass::Bulk, false) => Some(&self.bulk_v4),
            (crate::model::TrafficClass::Bulk, true) => Some(&self.bulk_v6),
            _ => None,
        }
    }
}

impl Config {
    pub fn load_or_default(path: &Path) -> Result<Self> {
        if path.exists() {
            let text = fs::read_to_string(path)
                .with_context(|| format!("failed to read config {}", path.display()))?;
            serde_json::from_str(&text)
                .with_context(|| format!("failed to parse config {}", path.display()))
        } else {
            Ok(Self::default_for_current_user())
        }
    }

    pub fn default_for_current_user() -> Self {
        let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".to_string());
        Self {
            state_path: PathBuf::from(DEFAULT_STATE),
            receipts_path: PathBuf::from(DEFAULT_RECEIPTS),
            feedback_path: PathBuf::from(DEFAULT_FEEDBACK),
            policy_state_path: PathBuf::from(DEFAULT_POLICY_STATE),
            lab_history_path: PathBuf::from(DEFAULT_LAB_HISTORY),
            profiles_dir: PathBuf::from(DEFAULT_PROFILES_DIR),
            interval_seconds: 5,
            candidate_timeout_seconds: 30,
            learning: LearningConfig {
                enabled: true,
                learn_bulk_after_score: 8,
                score_increment_for_bulk_hint: 3,
                score_increment_for_many_connections: 1,
                score_decrement_for_interactive_hint: 4,
            },
            classifier: ClassifierConfig {
                realtime_process_patterns: default_realtime_process_patterns(),
                remote_control_process_patterns: default_remote_control_process_patterns(),
                ai_work_process_patterns: default_ai_work_process_patterns(),
                proxy_process_patterns: default_proxy_process_patterns(),
                bulk_process_patterns: vec![
                    "steam".into(),
                    "steamwebhelper".into(),
                    "synology".into(),
                    "onedrive".into(),
                    "aria2".into(),
                    "qbittorrent".into(),
                    "transmission".into(),
                    "epicgames".into(),
                    "battle.net".into(),
                    "docker".into(),
                ],
                interactive_process_patterns: vec![
                    "weflow".into(),
                    "cursor".into(),
                    "code".into(),
                    "ssh".into(),
                    "windowsterminal".into(),
                    "terminal".into(),
                ],
                ignore_process_patterns: vec![
                    "verge-mihomo".into(),
                    "mihomo".into(),
                    "clash".into(),
                    "python".into(),
                    "pythonw".into(),
                    "pwsh".into(),
                    "powershell".into(),
                ],
                bulk_name_patterns: vec!["download".into(), "update".into(), "sync".into()],
                bulk_ports: vec![
                    80, 443, 6881, 51413, 27014, 27015, 27016, 27017, 27018, 27019, 27020,
                ],
            },
            backends: BackendConfig {
                routerqosd: RouterQosdConfig {
                    enabled: false,
                    host: "192.168.1.1".into(),
                    port: 22,
                    user: "root".into(),
                    key_path: PathBuf::from(format!("{home}\\.ssh\\id_ed25519")),
                    ssh_path: PathBuf::from("ssh.exe"),
                    class_sets: RouterClassSets::default(),
                },
            },
        }
    }
}

fn default_realtime_process_patterns() -> Vec<String> {
    [
        "mstsc",
        "rdpclip",
        "parsec",
        "sunshine",
        "moonlight",
        "obs",
        "streamlabs",
        "discord",
        "teamspeak",
        "zoom",
        "voovmeeting",
        "todesk",
        "anydesk",
        "rustdesk",
        "raylink",
        "uuremote",
        "uu_remote",
        "sunlogin",
        "nomachine",
        "deltaforce",
        "delta force",
        "dfgame",
        "valorant",
        "cs2",
        "counter-strike",
        "crossfire",
        "leagueclient",
        "riotclientservices",
        "tcls",
        "wegame",
        "qqgame",
        "arena breakout",
        "marvelrivals",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn default_remote_control_process_patterns() -> Vec<String> {
    [
        "mstsc",
        "rdpclip",
        "parsec",
        "sunshine",
        "moonlight",
        "todesk",
        "anydesk",
        "rustdesk",
        "raylink",
        "uuremote",
        "uu_remote",
        "sunlogin",
        "nomachine",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn default_ai_work_process_patterns() -> Vec<String> {
    [
        "cursor",
        "code",
        "windsurf",
        "trae",
        "zed",
        "chatgpt",
        "openai",
        "claude",
        "codex",
        "ollama",
        "lmstudio",
        "lm studio",
        "aider",
        "continue",
        "gemini",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn default_proxy_process_patterns() -> Vec<String> {
    [
        "verge-mihomo",
        "mihomo",
        "clash",
        "sing-box",
        "singbox",
        "v2ray",
        "xray",
        "hysteria",
        "tuic",
        "naiveproxy",
        "nekoray",
        "shadowsocks",
        "tailscale",
        "zerotier",
        "wireguard",
        "openvpn",
        "sstap",
        "proxifier",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn default_state_path() -> PathBuf {
    PathBuf::from(DEFAULT_STATE)
}

fn default_receipts_path() -> PathBuf {
    PathBuf::from(DEFAULT_RECEIPTS)
}

fn default_feedback_path() -> PathBuf {
    PathBuf::from(DEFAULT_FEEDBACK)
}

fn default_policy_state_path() -> PathBuf {
    PathBuf::from(DEFAULT_POLICY_STATE)
}

fn default_lab_history_path() -> PathBuf {
    PathBuf::from(DEFAULT_LAB_HISTORY)
}

fn default_profiles_dir() -> PathBuf {
    PathBuf::from(DEFAULT_PROFILES_DIR)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_safe_for_public_use() {
        let config = Config::default_for_current_user();

        assert!(!config.backends.routerqosd.enabled);
        assert_eq!(config.state_path, PathBuf::from(DEFAULT_STATE));
        assert_eq!(config.receipts_path, PathBuf::from(DEFAULT_RECEIPTS));
        assert_eq!(config.feedback_path, PathBuf::from(DEFAULT_FEEDBACK));
        assert_eq!(
            config.policy_state_path,
            PathBuf::from(DEFAULT_POLICY_STATE)
        );
        assert_eq!(config.lab_history_path, PathBuf::from(DEFAULT_LAB_HISTORY));
        assert_eq!(config.profiles_dir, PathBuf::from(DEFAULT_PROFILES_DIR));
        assert_eq!(config.backends.routerqosd.host, "192.168.1.1");
        assert_eq!(config.backends.routerqosd.port, 22);
        assert_eq!(config.backends.routerqosd.user, "root");
        assert_eq!(config.backends.routerqosd.class_sets.bulk_v4, "rqosd_ele4");
        assert!(
            config
                .classifier
                .remote_control_process_patterns
                .iter()
                .any(|item| item == "mstsc")
        );
        assert!(
            config
                .classifier
                .ai_work_process_patterns
                .iter()
                .any(|item| item == "ollama")
        );
    }

    #[test]
    fn old_config_without_phase2_fields_still_loads() {
        let text = r#"{
  "state_path": "winqos-state.json",
  "receipts_path": "winqos-receipts.jsonl",
  "feedback_path": "winqos-feedback.jsonl",
  "policy_state_path": "winqos-policy-state.json",
  "lab_history_path": "winqos-lab-history.jsonl",
  "profiles_dir": "profiles",
  "interval_seconds": 5,
  "candidate_timeout_seconds": 30,
  "learning": {
    "enabled": true,
    "learn_bulk_after_score": 8,
    "score_increment_for_bulk_hint": 3,
    "score_increment_for_many_connections": 1,
    "score_decrement_for_interactive_hint": 4
  },
  "classifier": {
    "bulk_process_patterns": ["steam"],
    "interactive_process_patterns": ["cursor"],
    "ignore_process_patterns": ["mihomo"],
    "bulk_name_patterns": ["download"],
    "bulk_ports": [443]
  },
  "backends": {
    "routerqosd": {
      "enabled": false,
      "host": "192.168.1.1",
      "port": 22,
      "user": "root",
      "key_path": "id_ed25519",
      "ssh_path": "ssh.exe"
    }
  }
}"#;

        let config: Config = serde_json::from_str(text).unwrap();

        assert_eq!(config.backends.routerqosd.class_sets.bulk_v4, "rqosd_ele4");
        assert!(
            config
                .classifier
                .remote_control_process_patterns
                .contains(&"mstsc".into())
        );
        assert!(
            config
                .classifier
                .proxy_process_patterns
                .contains(&"clash".into())
        );
    }
}
