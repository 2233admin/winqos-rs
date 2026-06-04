use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalKind {
    ForegroundProcess,
    GameProcess,
    StreamProcess,
    VoiceProcess,
    BulkDownload,
    UploadPressure,
    ProxyProcess,
    AiWorkProcess,
    TcpConnection,
    UdpFlow,
    UserFeedback,
    PauseFlag,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Signal {
    pub kind: SignalKind,
    pub source: String,
    pub confidence: f32,
    pub weight: f32,
    pub observed_unix: u64,
    pub labels: BTreeMap<String, String>,
}

impl Signal {
    pub fn new(kind: SignalKind, source: impl Into<String>, observed_unix: u64) -> Self {
        Self {
            kind,
            source: source.into(),
            confidence: 1.0,
            weight: 1.0,
            observed_unix,
            labels: BTreeMap::new(),
        }
    }

    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence.clamp(0.0, 1.0);
        self
    }

    pub fn with_weight(mut self, weight: f32) -> Self {
        self.weight = weight.max(0.0);
        self
    }

    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    pub fn score_contribution(&self) -> f32 {
        self.confidence * self.weight
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_kind_serializes_as_snake_case() {
        let text = serde_json::to_string(&SignalKind::BulkDownload).unwrap();

        assert_eq!(text, "\"bulk_download\"");
    }

    #[test]
    fn signal_clamps_confidence_and_weight() {
        let signal = Signal::new(SignalKind::GameProcess, "test", 1)
            .with_confidence(2.0)
            .with_weight(-1.0);

        assert_eq!(signal.confidence, 1.0);
        assert_eq!(signal.weight, 0.0);
        assert_eq!(signal.score_contribution(), 0.0);
    }
}
