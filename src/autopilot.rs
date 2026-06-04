use crate::feedback::FeedbackState;
use crate::learning::now_unix;
use crate::model::{ClassifiedConnection, TrafficClass};
use crate::policy::PolicyAction;
use crate::profile::{Profile, ProfileId, ProfilePack, builtin_profile_pack};
use crate::signal::{Signal, SignalKind};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutopilotDecision {
    pub profile: ProfileId,
    pub profile_title: String,
    pub confidence: f32,
    pub confidence_floor: f32,
    pub dry_run: bool,
    pub signals: Vec<Signal>,
    pub scores: Vec<ProfileScore>,
    pub actions: Vec<PolicyAction>,
    pub information: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileScore {
    pub profile: ProfileId,
    pub confidence: f32,
    pub raw_score: f32,
    pub feedback_bias: i32,
    pub matched_signals: Vec<SignalKind>,
    pub missing_required: Vec<SignalKind>,
}

pub fn decide_autopilot(
    classified: &[ClassifiedConnection],
    feedback: &FeedbackState,
    dry_run: bool,
) -> AutopilotDecision {
    decide_with_pack(
        classified,
        feedback,
        &builtin_profile_pack(),
        dry_run,
        now_unix(),
    )
}

pub fn decide_with_pack(
    classified: &[ClassifiedConnection],
    feedback: &FeedbackState,
    pack: &ProfilePack,
    dry_run: bool,
    observed_unix: u64,
) -> AutopilotDecision {
    let mut signals = signals_from_classified(classified, feedback, observed_unix);
    if feedback.paused {
        signals.push(
            Signal::new(SignalKind::PauseFlag, "policy_state", observed_unix)
                .with_confidence(1.0)
                .with_weight(1.0),
        );
    }
    let scores: Vec<_> = pack
        .profiles
        .iter()
        .map(|profile| score_profile(profile, &signals, feedback))
        .collect();
    let winner = choose_profile(pack, &scores);
    let actions = winner.actions.clone();
    let confidence = scores
        .iter()
        .find(|score| score.profile == winner.id)
        .map(|score| score.confidence)
        .unwrap_or_default();
    let mut information = Vec::new();
    information.push(format!(
        "selected {} confidence {:.2}",
        winner.id.as_str(),
        confidence
    ));
    information.extend(
        signals
            .iter()
            .take(8)
            .map(|signal| format!("{:?} from {}", signal.kind, signal.source)),
    );
    information.extend(
        actions
            .iter()
            .map(|action| format!("action {} via {:?}", action.id, action.backend)),
    );

    AutopilotDecision {
        profile: winner.id,
        profile_title: winner.title.clone(),
        confidence,
        confidence_floor: winner.confidence_floor,
        dry_run,
        signals,
        scores,
        actions,
        information,
    }
}

pub fn signals_from_classified(
    classified: &[ClassifiedConnection],
    feedback: &FeedbackState,
    observed_unix: u64,
) -> Vec<Signal> {
    let mut counts: BTreeMap<SignalKind, u32> = BTreeMap::new();
    let mut examples: BTreeMap<SignalKind, BTreeSet<String>> = BTreeMap::new();

    for item in classified {
        if feedback.is_process_ignored(&item.sample.process_name) {
            continue;
        }
        let label = format!(
            "{} {}",
            item.sample.process_name.to_lowercase(),
            item.sample.process_path.to_lowercase()
        );

        if item.sample.protocol == "tcp" {
            add_signal(
                &mut counts,
                &mut examples,
                SignalKind::TcpConnection,
                &label,
            );
        }
        if item.sample.protocol == "udp" {
            add_signal(&mut counts, &mut examples, SignalKind::UdpFlow, &label);
        }
        if item.class == TrafficClass::Bulk {
            add_signal(&mut counts, &mut examples, SignalKind::BulkDownload, &label);
        }
        if contains_any(
            &label,
            &[
                "deltaforce",
                "delta force",
                "dfgame",
                "valorant",
                "cs2",
                "counter-strike",
                "crossfire",
                "leagueclient",
                "riot",
                "tcls",
                "wegame",
                "qqgame",
                "arena breakout",
                "marvelrivals",
            ],
        ) {
            add_signal(&mut counts, &mut examples, SignalKind::GameProcess, &label);
        }
        if contains_any(&label, &["obs", "streamlabs", "twitch studio"]) {
            add_signal(
                &mut counts,
                &mut examples,
                SignalKind::StreamProcess,
                &label,
            );
        }
        if contains_any(&label, &["discord", "teamspeak", "weflow", "voice"]) {
            add_signal(&mut counts, &mut examples, SignalKind::VoiceProcess, &label);
        }
        if contains_any(&label, &["verge-mihomo", "mihomo", "clash"]) {
            add_signal(&mut counts, &mut examples, SignalKind::ProxyProcess, &label);
        }
        if contains_any(
            &label,
            &[
                "cursor", "code", "ssh", "terminal", "ollama", "claude", "codex",
            ],
        ) {
            add_signal(
                &mut counts,
                &mut examples,
                SignalKind::AiWorkProcess,
                &label,
            );
        }
    }

    counts
        .into_iter()
        .map(|(kind, count)| {
            let confidence = (0.7 + (count as f32 * 0.05)).min(1.0);
            let mut signal = Signal::new(kind, "connection_classifier", observed_unix)
                .with_confidence(confidence)
                .with_weight(count as f32)
                .with_label("count", count.to_string());
            if let Some(items) = examples.get(&kind) {
                signal = signal.with_label(
                    "examples",
                    items.iter().take(3).cloned().collect::<Vec<_>>().join(","),
                );
            }
            signal
        })
        .collect()
}

fn score_profile(profile: &Profile, signals: &[Signal], feedback: &FeedbackState) -> ProfileScore {
    let mut best_by_kind = BTreeMap::new();
    for signal in signals {
        best_by_kind
            .entry(signal.kind)
            .and_modify(|current: &mut f32| *current = current.max(signal.confidence))
            .or_insert(signal.confidence);
    }

    let mut raw_score = 0.0;
    let mut matched_signals = Vec::new();
    let mut missing_required = Vec::new();
    for rule in &profile.signal_rules {
        if let Some(confidence) = best_by_kind.get(&rule.kind) {
            raw_score += confidence * rule.weight;
            matched_signals.push(rule.kind);
        } else if rule.required {
            missing_required.push(rule.kind);
        }
    }

    let feedback_bias = feedback.profile_bias(profile.id);
    if !missing_required.is_empty() {
        raw_score = 0.0;
    } else {
        raw_score += feedback_bias as f32 * 0.03;
    }

    let confidence = if profile.id == ProfileId::Normal && profile.signal_rules.is_empty() {
        0.2
    } else {
        raw_score.clamp(0.0, 1.0)
    };

    ProfileScore {
        profile: profile.id,
        confidence,
        raw_score,
        feedback_bias,
        matched_signals,
        missing_required,
    }
}

fn choose_profile<'a>(pack: &'a ProfilePack, scores: &[ProfileScore]) -> &'a Profile {
    let normal = pack
        .profiles
        .iter()
        .find(|profile| profile.id == ProfileId::Normal)
        .expect("built-in pack must include normal profile");
    pack.profiles
        .iter()
        .filter_map(|profile| {
            let score = scores.iter().find(|score| score.profile == profile.id)?;
            (score.confidence >= profile.confidence_floor).then_some((profile, score))
        })
        .max_by(|(left_profile, left_score), (right_profile, right_score)| {
            compare_score(left_score.confidence, right_score.confidence)
                .then_with(|| left_profile.priority.cmp(&right_profile.priority))
        })
        .map(|(profile, _)| profile)
        .unwrap_or(normal)
}

fn compare_score(left: f32, right: f32) -> Ordering {
    left.partial_cmp(&right).unwrap_or(Ordering::Equal)
}

fn add_signal(
    counts: &mut BTreeMap<SignalKind, u32>,
    examples: &mut BTreeMap<SignalKind, BTreeSet<String>>,
    kind: SignalKind,
    label: &str,
) {
    *counts.entry(kind).or_default() += 1;
    examples
        .entry(kind)
        .or_default()
        .insert(label.chars().take(48).collect());
}

fn contains_any(label: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| label.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ClassifiedConnection, ConnectionSample, RouterCandidate};

    fn classified(process: &str, class: TrafficClass) -> ClassifiedConnection {
        ClassifiedConnection {
            sample: ConnectionSample {
                pid: 7,
                process_name: process.into(),
                process_path: String::new(),
                protocol: "tcp".into(),
                remote_addr: "8.8.8.8".into(),
                remote_port: 443,
                state: "Established".into(),
            },
            class,
            reason: format!("{class:?}"),
            learned_score: 0,
            router_candidate: if class == TrafficClass::Bulk {
                Some(RouterCandidate {
                    set_name: "rqosd_ele4".into(),
                    member: "8.8.8.8,tcp:443".into(),
                    reason: "bulk".into(),
                })
            } else {
                None
            },
        }
    }

    #[test]
    fn game_plus_download_selects_game_boost() {
        let decision = decide_with_pack(
            &[
                classified("DeltaForceClient", TrafficClass::Interactive),
                classified("steam", TrafficClass::Bulk),
            ],
            &FeedbackState::default(),
            &builtin_profile_pack(),
            true,
            1,
        );

        assert_eq!(decision.profile, ProfileId::GameBoost);
        assert!(decision.confidence >= decision.confidence_floor);
        assert!(
            decision
                .actions
                .iter()
                .any(|action| action.id == "game_boost.demote_bulk")
        );
    }

    #[test]
    fn steam_download_selects_steam_sink() {
        let decision = decide_with_pack(
            &[classified("steam", TrafficClass::Bulk)],
            &FeedbackState::default(),
            &builtin_profile_pack(),
            true,
            1,
        );

        assert_eq!(decision.profile, ProfileId::SteamSink);
    }

    #[test]
    fn feedback_preference_can_select_ai_work_lane() {
        let mut feedback = FeedbackState::default();
        feedback.profile_bias.insert("ai_work_lane".into(), 12);

        let decision = decide_with_pack(
            &[classified("cursor", TrafficClass::Interactive)],
            &feedback,
            &builtin_profile_pack(),
            true,
            1,
        );

        assert_eq!(decision.profile, ProfileId::AiWorkLane);
        assert_eq!(
            decision
                .scores
                .iter()
                .find(|score| score.profile == ProfileId::AiWorkLane)
                .unwrap()
                .feedback_bias,
            12
        );
    }

    #[test]
    fn ignored_process_does_not_emit_signals() {
        let mut feedback = FeedbackState::default();
        feedback
            .ignored_processes
            .insert("steam".into(), "game-exits".into());

        let signals =
            signals_from_classified(&[classified("steam", TrafficClass::Bulk)], &feedback, 1);

        assert!(signals.is_empty());
    }

    #[test]
    fn paused_state_selects_paused_profile() {
        let feedback = FeedbackState {
            paused: true,
            ..FeedbackState::default()
        };

        let decision = decide_with_pack(
            &[classified("steam", TrafficClass::Bulk)],
            &feedback,
            &builtin_profile_pack(),
            true,
            1,
        );

        assert_eq!(decision.profile, ProfileId::Paused);
        assert!(
            decision
                .signals
                .iter()
                .any(|signal| signal.kind == SignalKind::PauseFlag)
        );
    }
}
