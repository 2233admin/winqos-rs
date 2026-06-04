use crate::autopilot::AutopilotDecision;
use crate::feedback::FeedbackState;
use crate::profile::ProfileId;

pub fn render_decision(decision: &AutopilotDecision) -> String {
    let profile = profile_label(decision.profile);
    let mode = if decision.dry_run { "DRY-RUN" } else { "LIVE" };
    let mut lines = vec![
        "+-------------------------- WQ OVERDRIVE --------------------------+".into(),
        format!(
            "| PROFILE {:<16} CONF {:>5.2}       MODE {:<10} |",
            profile, decision.confidence, mode
        ),
        format!(
            "| ACTIONS {:<16} SIGNALS {:<5}       ROLLBACK ARMED |",
            decision.actions.len(),
            decision.signals.len()
        ),
        "+------------------------------------------------------------------+".into(),
        String::new(),
        "PACKET MAP".into(),
    ];

    for score in decision.scores.iter().take(5) {
        lines.push(format!(
            "{:<12} {} {:.2}",
            profile_label(score.profile),
            confidence_bar(score.confidence),
            score.confidence
        ));
    }

    lines.push(String::new());
    lines.push("INFORMATION".into());
    lines.extend(decision.information.iter().take(8).cloned());
    lines.join("\n")
}

pub fn render_status(state: &FeedbackState) -> String {
    let profile = state.last_profile.unwrap_or(ProfileId::Normal);
    let mut lines = vec![
        "+-------------------------- WQ OVERDRIVE --------------------------+".into(),
        format!(
            "| PROFILE {:<16} CONF {:>5.2}       FEEDBACK {:<8} |",
            profile_label(profile),
            state.last_confidence,
            state.profile_bias(profile)
        ),
        format!(
            "| ACTIONS {:<16} IGNORED {:<7}       STATUS READY  |",
            state.last_action_ids.len(),
            state.ignored_processes.len()
        ),
        "+------------------------------------------------------------------+".into(),
        String::new(),
        "INFORMATION".into(),
    ];

    if state.last_explanation.is_empty() {
        lines.push("no autopilot decision recorded yet".into());
    } else {
        lines.extend(state.last_explanation.iter().take(8).cloned());
    }
    lines.join("\n")
}

pub fn render_explain(state: &FeedbackState) -> String {
    let mut lines = vec![
        "+--------------------------- WQ EXPLAIN ---------------------------+".into(),
        format!(
            "| LAST PROFILE {:<14} CONF {:>5.2}                    |",
            profile_label(state.last_profile.unwrap_or(ProfileId::Normal)),
            state.last_confidence
        ),
        "+------------------------------------------------------------------+".into(),
        String::new(),
        "INFORMATION".into(),
    ];

    if state.last_explanation.is_empty() {
        lines.push("run once first: winqos-rs run --once --dry-run".into());
    } else {
        lines.extend(state.last_explanation.iter().cloned());
    }
    lines.join("\n")
}

fn confidence_bar(confidence: f32) -> String {
    let filled = (confidence.clamp(0.0, 1.0) * 10.0).round() as usize;
    format!("{}{}", "#".repeat(filled), ".".repeat(10 - filled))
}

fn profile_label(profile: ProfileId) -> String {
    profile.as_str().replace('_', " ").to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autopilot::decide_with_pack;
    use crate::feedback::FeedbackState;
    use crate::model::{ClassifiedConnection, ConnectionSample, TrafficClass};
    use crate::profile::builtin_profile_pack;

    #[test]
    fn status_renders_petscii_console() {
        let mut state = FeedbackState::default();
        state.set_last_decision(
            ProfileId::GameBoost,
            0.91,
            vec!["a".into()],
            vec!["selected game_boost confidence 0.91".into()],
            1,
        );

        let rendered = render_status(&state);

        assert!(rendered.contains("WQ OVERDRIVE"));
        assert!(rendered.contains("GAME BOOST"));
        assert!(rendered.contains("selected game_boost"));
    }

    #[test]
    fn decision_render_includes_packet_map() {
        let item = ClassifiedConnection {
            sample: ConnectionSample {
                pid: 1,
                process_name: "steam".into(),
                process_path: String::new(),
                protocol: "tcp".into(),
                remote_addr: "8.8.8.8".into(),
                remote_port: 443,
                state: "Established".into(),
            },
            class: TrafficClass::Bulk,
            reason: "bulk_process".into(),
            learned_score: 0,
            router_candidate: None,
        };
        let decision = decide_with_pack(
            &[item],
            &FeedbackState::default(),
            &builtin_profile_pack(),
            true,
            1,
        );

        let rendered = render_decision(&decision);

        assert!(rendered.contains("PACKET MAP"));
        assert!(rendered.contains("STEAM SINK"));
    }
}
