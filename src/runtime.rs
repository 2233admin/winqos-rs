use crate::config::{
    DEFAULT_CONFIG, DEFAULT_FEEDBACK, DEFAULT_POLICY_STATE, DEFAULT_PROFILES_DIR, DEFAULT_RECEIPTS,
    DEFAULT_STATE,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimePaths {
    pub root: PathBuf,
    pub config: PathBuf,
    pub learner_state: PathBuf,
    pub receipts: PathBuf,
    pub feedback: PathBuf,
    pub policy_state: PathBuf,
    pub profiles_dir: PathBuf,
}

impl RuntimePaths {
    pub fn repo_local() -> Self {
        Self::from_root(".")
    }

    pub fn program_data_default() -> Self {
        let root = std::env::var("ProgramData")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(r"C:\ProgramData"))
            .join("winqos-rs");
        Self::from_root(root)
    }

    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            config: root.join(DEFAULT_CONFIG),
            learner_state: root.join(DEFAULT_STATE),
            receipts: root.join(DEFAULT_RECEIPTS),
            feedback: root.join(DEFAULT_FEEDBACK),
            policy_state: root.join(DEFAULT_POLICY_STATE),
            profiles_dir: root.join(DEFAULT_PROFILES_DIR),
            root,
        }
    }

    pub fn policy_profile_current(&self, profile_id: &str) -> PathBuf {
        self.profiles_dir.join(format!("{profile_id}.current.json"))
    }

    pub fn policy_profile_best(&self, profile_id: &str) -> PathBuf {
        self.profiles_dir.join(format!("{profile_id}.best.json"))
    }

    pub fn policy_profile_history(&self, profile_id: &str) -> PathBuf {
        self.profiles_dir
            .join(format!("{profile_id}.history.jsonl"))
    }

    pub fn persistent_files(&self) -> [&Path; 5] {
        [
            &self.config,
            &self.learner_state,
            &self.receipts,
            &self.feedback,
            &self.policy_state,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_local_runtime_boundaries_use_phase1_names() {
        let paths = RuntimePaths::from_root("runtime");

        assert_eq!(paths.config, PathBuf::from("runtime").join(DEFAULT_CONFIG));
        assert_eq!(
            paths.learner_state,
            PathBuf::from("runtime").join(DEFAULT_STATE)
        );
        assert_eq!(
            paths.receipts,
            PathBuf::from("runtime").join(DEFAULT_RECEIPTS)
        );
        assert_eq!(
            paths.feedback,
            PathBuf::from("runtime").join(DEFAULT_FEEDBACK)
        );
        assert_eq!(
            paths.policy_state,
            PathBuf::from("runtime").join(DEFAULT_POLICY_STATE)
        );
        assert_eq!(
            paths.policy_profile_history("game_boost"),
            PathBuf::from("runtime")
                .join(DEFAULT_PROFILES_DIR)
                .join("game_boost.history.jsonl")
        );
    }

    #[test]
    fn persistent_files_stay_under_root() {
        let paths = RuntimePaths::from_root("runtime");

        for file in paths.persistent_files() {
            assert!(file.starts_with("runtime"));
        }
    }
}
