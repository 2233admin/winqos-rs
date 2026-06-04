use crate::autopilot::AutopilotDecision;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionSample {
    pub pid: u32,
    pub process_name: String,
    pub process_path: String,
    pub protocol: String,
    pub remote_addr: String,
    pub remote_port: u16,
    pub state: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrafficClass {
    Realtime,
    Interactive,
    Normal,
    Bulk,
    Ignore,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifiedConnection {
    pub sample: ConnectionSample,
    pub class: TrafficClass,
    pub reason: String,
    pub learned_score: i32,
    pub router_candidate: Option<RouterCandidate>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RouterCandidate {
    pub set_name: String,
    pub member: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct RunReport {
    pub updated_unix: u64,
    pub sample_count: usize,
    pub class_counts: BTreeMap<String, usize>,
    pub autopilot: AutopilotDecision,
    pub candidate_count: usize,
    pub candidates: Vec<RouterCandidate>,
    pub backend: BackendReport,
}

#[derive(Debug, Serialize)]
pub struct BackendReport {
    pub name: String,
    pub dry_run: bool,
    pub executed: bool,
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}
