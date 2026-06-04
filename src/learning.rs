use crate::config::Config;
use crate::model::{ClassifiedConnection, ConnectionSample, TrafficClass};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct LearnerState {
    pub updated_unix: u64,
    pub processes: BTreeMap<String, ProcessLearning>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ProcessLearning {
    pub seen: u64,
    pub bulk_score: i32,
    pub last_reason: String,
    pub last_seen_unix: u64,
    pub remote_ports: BTreeMap<u16, u64>,
}

pub fn process_key(sample: &ConnectionSample) -> String {
    if !sample.process_path.is_empty() {
        sample.process_path.to_lowercase()
    } else {
        sample.process_name.to_lowercase()
    }
}

pub fn update_learning(config: &Config, state: &mut LearnerState, item: &ClassifiedConnection) {
    if !config.learning.enabled {
        return;
    }
    let key = process_key(&item.sample);
    let entry = state.processes.entry(key).or_default();
    entry.seen = entry.seen.saturating_add(1);
    entry.last_seen_unix = now_unix();
    entry.last_reason = item.reason.clone();
    *entry
        .remote_ports
        .entry(item.sample.remote_port)
        .or_default() += 1;
    match item.class {
        TrafficClass::Bulk => entry.bulk_score += config.learning.score_increment_for_bulk_hint,
        TrafficClass::Interactive | TrafficClass::Realtime => {
            entry.bulk_score -= config.learning.score_decrement_for_interactive_hint
        }
        TrafficClass::Normal => {
            if entry.remote_ports.len() >= 8 {
                entry.bulk_score += config.learning.score_increment_for_many_connections;
            }
        }
        TrafficClass::Ignore => {}
    }
    entry.bulk_score = entry.bulk_score.clamp(-32, 64);
    state.updated_unix = now_unix();
}

pub fn load_state(path: &Path) -> Result<LearnerState> {
    if !path.exists() {
        return Ok(LearnerState::default());
    }
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read state {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("failed to parse state {}", path.display()))
}

pub fn save_state(path: &Path, state: &LearnerState) -> Result<()> {
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(state)? + "\n")
        .with_context(|| format!("failed to write {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("failed to replace {}", path.display()))?;
    Ok(())
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ClassifiedConnection, ConnectionSample, TrafficClass};

    fn sample(process: &str, port: u16) -> ConnectionSample {
        ConnectionSample {
            pid: 7,
            process_name: process.into(),
            process_path: String::new(),
            protocol: "tcp".into(),
            remote_addr: "203.0.113.10".into(),
            remote_port: port,
            state: "Established".into(),
        }
    }

    #[test]
    fn bulk_learning_increases_score_and_tracks_ports() {
        let config = Config::default_for_current_user();
        let item = ClassifiedConnection {
            sample: sample("steam", 443),
            class: TrafficClass::Bulk,
            reason: "bulk_process".into(),
            learned_score: 0,
            router_candidate: None,
        };
        let mut state = LearnerState::default();

        update_learning(&config, &mut state, &item);

        let entry = state.processes.get("steam").unwrap();
        assert_eq!(entry.seen, 1);
        assert_eq!(
            entry.bulk_score,
            config.learning.score_increment_for_bulk_hint
        );
        assert_eq!(entry.remote_ports.get(&443), Some(&1));
    }

    #[test]
    fn interactive_learning_reduces_score() {
        let config = Config::default_for_current_user();
        let item = ClassifiedConnection {
            sample: sample("cursor", 443),
            class: TrafficClass::Interactive,
            reason: "interactive_process".into(),
            learned_score: 0,
            router_candidate: None,
        };
        let mut state = LearnerState::default();

        update_learning(&config, &mut state, &item);

        assert_eq!(
            state.processes.get("cursor").unwrap().bulk_score,
            -config.learning.score_decrement_for_interactive_hint
        );
    }
}
