use crate::config::Config;
use crate::profile::ProfileId;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeedbackState {
    #[serde(default)]
    pub updated_unix: u64,
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub pause_reason: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub profile_bias: BTreeMap<String, i32>,
    #[serde(default)]
    pub ignored_processes: BTreeMap<String, String>,
    #[serde(default)]
    pub last_profile: Option<ProfileId>,
    #[serde(default)]
    pub last_confidence: f32,
    #[serde(default)]
    pub last_action_ids: Vec<String>,
    #[serde(default)]
    pub auto_observation_count: u32,
    #[serde(default)]
    pub last_applied_profile: Option<ProfileId>,
    #[serde(default)]
    pub last_applied_confidence: f32,
    #[serde(default)]
    pub last_applied_unix: u64,
    #[serde(default)]
    pub last_applied_action_ids: Vec<String>,
    #[serde(default)]
    pub last_explanation: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedbackEvent {
    pub created_unix: u64,
    pub kind: FeedbackEventKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum FeedbackEventKind {
    GoodLast,
    BadLast,
    RollbackLast,
    Prefer { profile: ProfileId },
    IgnoreProcess { process_name: String, until: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedbackApplyReport {
    pub updated: bool,
    pub message: String,
    pub profile_bias: BTreeMap<String, i32>,
}

impl FeedbackState {
    pub fn profile_bias(&self, profile: ProfileId) -> i32 {
        self.profile_bias
            .get(profile.as_str())
            .copied()
            .unwrap_or_default()
    }

    pub fn is_process_ignored(&self, process_name: &str) -> bool {
        self.ignored_processes
            .contains_key(&normalize_process(process_name))
    }

    pub fn set_last_decision(
        &mut self,
        profile: ProfileId,
        confidence: f32,
        action_ids: Vec<String>,
        explanation: Vec<String>,
        updated_unix: u64,
    ) {
        self.updated_unix = updated_unix;
        self.last_profile = Some(profile);
        self.last_confidence = confidence;
        self.last_action_ids = action_ids;
        self.last_explanation = explanation;
    }

    pub fn observe_for_assist(
        &mut self,
        profile: ProfileId,
        confidence: f32,
        min_confidence: f32,
        observation_cycles: u32,
    ) {
        if confidence < min_confidence {
            self.auto_observation_count = 0;
            return;
        }

        if self.last_profile == Some(profile) {
            self.auto_observation_count = self.auto_observation_count.saturating_add(1);
        } else {
            self.auto_observation_count = 1;
        }

        self.auto_observation_count = self.auto_observation_count.min(observation_cycles.max(1));
    }

    pub fn assist_should_apply(&self, min_confidence: f32, observation_cycles: u32) -> bool {
        self.last_profile.is_some()
            && self.last_confidence >= min_confidence
            && self.auto_observation_count >= observation_cycles.max(1)
    }

    pub fn clear_auto_observation(&mut self) {
        self.auto_observation_count = 0;
    }

    pub fn remember_live_apply(
        &mut self,
        profile: ProfileId,
        confidence: f32,
        action_ids: Vec<String>,
        updated_unix: u64,
    ) {
        self.last_applied_profile = Some(profile);
        self.last_applied_confidence = confidence;
        self.last_applied_unix = updated_unix;
        self.last_applied_action_ids = action_ids;
    }

    pub fn clear_last_apply_tracking(&mut self) {
        self.last_applied_profile = None;
        self.last_applied_confidence = 0.0;
        self.last_applied_unix = 0;
        self.last_applied_action_ids.clear();
    }

    pub fn pause(&mut self, reason: impl Into<String>, updated_unix: u64) {
        self.updated_unix = updated_unix;
        self.paused = true;
        self.pause_reason = Some(reason.into());
    }

    pub fn resume(&mut self, updated_unix: u64) {
        self.updated_unix = updated_unix;
        self.paused = false;
        self.pause_reason = None;
        self.last_error = None;
    }

    pub fn fail_closed(&mut self, error: impl Into<String>, updated_unix: u64) {
        self.updated_unix = updated_unix;
        self.paused = true;
        self.pause_reason = Some("fail_closed".into());
        self.last_error = Some(error.into());
    }
}

impl FeedbackEvent {
    pub fn good_last(created_unix: u64) -> Self {
        Self {
            created_unix,
            kind: FeedbackEventKind::GoodLast,
        }
    }

    pub fn bad_last(created_unix: u64) -> Self {
        Self {
            created_unix,
            kind: FeedbackEventKind::BadLast,
        }
    }

    pub fn rollback_last(created_unix: u64) -> Self {
        Self {
            created_unix,
            kind: FeedbackEventKind::RollbackLast,
        }
    }

    pub fn prefer(profile: ProfileId, created_unix: u64) -> Self {
        Self {
            created_unix,
            kind: FeedbackEventKind::Prefer { profile },
        }
    }

    pub fn ignore_process(
        process_name: impl Into<String>,
        until: impl Into<String>,
        created_unix: u64,
    ) -> Self {
        Self {
            created_unix,
            kind: FeedbackEventKind::IgnoreProcess {
                process_name: process_name.into(),
                until: until.into(),
            },
        }
    }
}

pub fn load_feedback_state(path: &Path) -> Result<FeedbackState> {
    if !path.exists() {
        return Ok(FeedbackState::default());
    }
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read policy state {}", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("failed to parse policy state {}", path.display()))
}

pub fn save_feedback_state(path: &Path, state: &FeedbackState) -> Result<()> {
    ensure_parent_dir(path)?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(state)? + "\n")
        .with_context(|| format!("failed to write {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("failed to replace {}", path.display()))?;
    Ok(())
}

pub fn record_feedback_event(config: &Config, event: FeedbackEvent) -> Result<FeedbackApplyReport> {
    append_feedback_event(&config.feedback_path, &event)?;
    let mut state = load_feedback_state(&config.policy_state_path)?;
    let report = apply_feedback_event(&mut state, &event);
    if report.updated {
        state.updated_unix = event.created_unix;
    }
    save_feedback_state(&config.policy_state_path, &state)?;
    Ok(report)
}

pub fn append_feedback_event(path: &Path, event: &FeedbackEvent) -> Result<()> {
    ensure_parent_dir(path)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open feedback log {}", path.display()))?;
    writeln!(file, "{}", serde_json::to_string(event)?)
        .with_context(|| format!("failed to append feedback log {}", path.display()))?;
    Ok(())
}

pub fn apply_feedback_event(
    state: &mut FeedbackState,
    event: &FeedbackEvent,
) -> FeedbackApplyReport {
    match &event.kind {
        FeedbackEventKind::GoodLast => bump_last_profile(state, 2, "last decision marked good"),
        FeedbackEventKind::BadLast => bump_last_profile(state, -2, "last decision marked bad"),
        FeedbackEventKind::RollbackLast => {
            bump_last_profile(state, -4, "last decision rolled back by user")
        }
        FeedbackEventKind::Prefer { profile } => bump_profile(
            state,
            *profile,
            5,
            format!("preferred {}", profile.as_str()),
        ),
        FeedbackEventKind::IgnoreProcess {
            process_name,
            until,
        } => {
            state
                .ignored_processes
                .insert(normalize_process(process_name), until.clone());
            FeedbackApplyReport {
                updated: true,
                message: format!("ignored process {process_name} until {until}"),
                profile_bias: state.profile_bias.clone(),
            }
        }
    }
}

fn bump_last_profile(state: &mut FeedbackState, delta: i32, message: &str) -> FeedbackApplyReport {
    if let Some(profile) = state.last_profile {
        bump_profile(state, profile, delta, message.to_string())
    } else {
        FeedbackApplyReport {
            updated: false,
            message: "no last autopilot decision to update".into(),
            profile_bias: state.profile_bias.clone(),
        }
    }
}

fn bump_profile(
    state: &mut FeedbackState,
    profile: ProfileId,
    delta: i32,
    message: String,
) -> FeedbackApplyReport {
    let entry = state
        .profile_bias
        .entry(profile.as_str().into())
        .or_default();
    *entry = (*entry + delta).clamp(-12, 12);
    FeedbackApplyReport {
        updated: true,
        message,
        profile_bias: state.profile_bias.clone(),
    }
}

fn normalize_process(process_name: &str) -> String {
    process_name.trim().to_lowercase()
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

    #[test]
    fn prefer_feedback_increases_profile_bias() {
        let mut state = FeedbackState::default();
        let event = FeedbackEvent::prefer(ProfileId::GameBoost, 1);

        let report = apply_feedback_event(&mut state, &event);

        assert!(report.updated);
        assert_eq!(state.profile_bias(ProfileId::GameBoost), 5);
    }

    #[test]
    fn good_last_requires_last_decision() {
        let mut state = FeedbackState::default();

        let report = apply_feedback_event(&mut state, &FeedbackEvent::good_last(1));

        assert!(!report.updated);
        assert_eq!(report.message, "no last autopilot decision to update");
    }

    #[test]
    fn ignore_process_is_case_insensitive() {
        let mut state = FeedbackState::default();

        apply_feedback_event(
            &mut state,
            &FeedbackEvent::ignore_process("Steam.exe", "game-exits", 1),
        );

        assert!(state.is_process_ignored("steam.EXE"));
    }

    #[test]
    fn pause_resume_and_fail_closed_update_state() {
        let mut state = FeedbackState::default();

        state.pause("manual", 1);
        assert!(state.paused);
        assert_eq!(state.pause_reason.as_deref(), Some("manual"));

        state.fail_closed("backend failed", 2);
        assert!(state.paused);
        assert_eq!(state.last_error.as_deref(), Some("backend failed"));

        state.resume(3);
        assert!(!state.paused);
        assert!(state.pause_reason.is_none());
        assert!(state.last_error.is_none());
    }
}
