pub mod adapter;
pub mod autopilot;
pub mod backend;
pub mod classifier;
pub mod cli;
pub mod collector;
pub mod config;
pub mod daemon;
pub mod feedback;
pub mod lab;
pub mod learning;
pub mod model;
pub mod petscii;
pub mod policy;
pub mod profile;
pub mod receipt;
pub mod runner;
pub mod runtime;
pub mod signal;

pub use adapter::{AdapterPlan, AdapterRecommendation, AdapterTier, NetworkAdapter};
pub use autopilot::{AutopilotDecision, ProfileScore, decide_autopilot};
pub use config::{Config, DEFAULT_CONFIG, DEFAULT_STATE};
pub use daemon::{DaemonOptions, DaemonReport, InstallPlan};
pub use feedback::{FeedbackEvent, FeedbackEventKind, FeedbackState};
pub use lab::{LabMetrics, LabReport, LabScenario, OptimizerDecision};
pub use model::{
    BackendReport, ClassifiedConnection, ConnectionSample, RouterCandidate, RunReport, TrafficClass,
};
pub use policy::{ActionSelector, ActionValue, BackendKind, PolicyAction, PolicyActionKind};
pub use profile::{Profile, ProfileId, ProfilePack, SignalRule, builtin_profile_pack};
pub use receipt::{Receipt, ReceiptStatus, Rollback, RollbackMethod, RollbackReceipt};
pub use runtime::RuntimePaths;
pub use signal::{Signal, SignalKind};
