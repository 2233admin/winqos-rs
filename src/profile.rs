use crate::model::TrafficClass;
use crate::policy::{ActionSelector, BackendKind, PolicyAction};
use crate::signal::SignalKind;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

pub const BUILTIN_PACK_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileId {
    GameBoost,
    StreamGuard,
    SteamSink,
    ProxySmart,
    AiWorkLane,
    Normal,
    Paused,
}

impl ProfileId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GameBoost => "game_boost",
            Self::StreamGuard => "stream_guard",
            Self::SteamSink => "steam_sink",
            Self::ProxySmart => "proxy_smart",
            Self::AiWorkLane => "ai_work_lane",
            Self::Normal => "normal",
            Self::Paused => "paused",
        }
    }
}

impl fmt::Display for ProfileId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for ProfileId {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "game_boost" | "game" => Ok(Self::GameBoost),
            "stream_guard" | "stream" => Ok(Self::StreamGuard),
            "steam_sink" | "steam" => Ok(Self::SteamSink),
            "proxy_smart" | "proxy" => Ok(Self::ProxySmart),
            "ai_work_lane" | "ai" | "work" => Ok(Self::AiWorkLane),
            "normal" => Ok(Self::Normal),
            "paused" | "pause" => Ok(Self::Paused),
            _ => Err(format!("unknown profile: {value}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignalRule {
    pub kind: SignalKind,
    pub weight: f32,
    pub required: bool,
}

impl SignalRule {
    pub fn weighted(kind: SignalKind, weight: f32) -> Self {
        Self {
            kind,
            weight,
            required: false,
        }
    }

    pub fn required(kind: SignalKind, weight: f32) -> Self {
        Self {
            kind,
            weight,
            required: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub id: ProfileId,
    pub title: String,
    pub intent: String,
    pub priority: u16,
    pub confidence_floor: f32,
    pub signal_rules: Vec<SignalRule>,
    pub actions: Vec<PolicyAction>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfilePack {
    pub schema_version: u16,
    pub pack_id: String,
    pub version: String,
    pub built_in: bool,
    pub profiles: Vec<Profile>,
}

pub fn builtin_profile_pack() -> ProfilePack {
    ProfilePack {
        schema_version: BUILTIN_PACK_SCHEMA_VERSION,
        pack_id: "winqos.phase1.builtin".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        built_in: true,
        profiles: vec![
            profile(
                ProfileId::GameBoost,
                "Game Boost",
                "Protect active game and voice flows while demoting bulk transfer pressure.",
                900,
                0.52,
                vec![
                    SignalRule::required(SignalKind::GameProcess, 0.7),
                    SignalRule::weighted(SignalKind::UdpFlow, 0.2),
                    SignalRule::weighted(SignalKind::BulkDownload, 0.15),
                    SignalRule::weighted(SignalKind::ProxyProcess, 0.1),
                ],
                vec![
                    PolicyAction::dscp_mark(
                        "game_boost.protect_interactive",
                        ProfileId::GameBoost,
                        ActionSelector::TrafficClass {
                            class: TrafficClass::Interactive,
                        },
                        46,
                        "protect active game and voice flows",
                    ),
                    PolicyAction::dscp_mark(
                        "game_boost.demote_bulk",
                        ProfileId::GameBoost,
                        ActionSelector::TrafficClass {
                            class: TrafficClass::Bulk,
                        },
                        8,
                        "keep downloads from colliding with match traffic",
                    ),
                ],
            ),
            profile(
                ProfileId::StreamGuard,
                "Stream Guard",
                "Protect livestream upload and voice while keeping downloads behind it.",
                820,
                0.5,
                vec![
                    SignalRule::required(SignalKind::StreamProcess, 0.45),
                    SignalRule::weighted(SignalKind::UploadPressure, 0.35),
                    SignalRule::weighted(SignalKind::VoiceProcess, 0.15),
                ],
                vec![
                    PolicyAction::dscp_mark(
                        "stream_guard.protect_upload",
                        ProfileId::StreamGuard,
                        ActionSelector::TrafficClass {
                            class: TrafficClass::Interactive,
                        },
                        34,
                        "guard livestream upload and call traffic",
                    ),
                    PolicyAction::dscp_mark(
                        "stream_guard.demote_bulk",
                        ProfileId::StreamGuard,
                        ActionSelector::TrafficClass {
                            class: TrafficClass::Bulk,
                        },
                        8,
                        "reduce download pressure while streaming",
                    ),
                ],
            ),
            profile(
                ProfileId::SteamSink,
                "Steam Sink",
                "Treat Steam and launcher download traffic as background pressure.",
                620,
                0.48,
                vec![
                    SignalRule::required(SignalKind::BulkDownload, 0.6),
                    SignalRule::weighted(SignalKind::TcpConnection, 0.2),
                ],
                vec![PolicyAction::dscp_mark(
                    "steam_sink.demote_bulk",
                    ProfileId::SteamSink,
                    ActionSelector::TrafficClass {
                        class: TrafficClass::Bulk,
                    },
                    8,
                    "sink launcher downloads below interactive work",
                )],
            ),
            profile(
                ProfileId::ProxySmart,
                "Proxy Smart",
                "Keep proxy engines unmarked while preserving visible app intent.",
                540,
                0.45,
                vec![SignalRule::required(SignalKind::ProxyProcess, 0.65)],
                vec![PolicyAction::observe_only(
                    "proxy_smart.observe_proxy",
                    ProfileId::ProxySmart,
                    BackendKind::LocalDscp,
                    "do not blindly mark tunnel engine traffic",
                )],
            ),
            profile(
                ProfileId::AiWorkLane,
                "AI Work Lane",
                "Protect coding and local AI work from bulk transfer contention.",
                500,
                0.45,
                vec![
                    SignalRule::required(SignalKind::AiWorkProcess, 0.5),
                    SignalRule::weighted(SignalKind::TcpConnection, 0.2),
                ],
                vec![PolicyAction::dscp_mark(
                    "ai_work_lane.protect_work",
                    ProfileId::AiWorkLane,
                    ActionSelector::TrafficClass {
                        class: TrafficClass::Interactive,
                    },
                    26,
                    "protect coding, ssh, and model interaction flows",
                )],
            ),
            profile(
                ProfileId::Normal,
                "Normal",
                "Observe and keep the current network state unchanged.",
                100,
                0.0,
                vec![],
                vec![PolicyAction::observe_only(
                    "normal.observe",
                    ProfileId::Normal,
                    BackendKind::LocalDscp,
                    "no confident optimization profile is active",
                )],
            ),
            profile(
                ProfileId::Paused,
                "Paused",
                "Disable automatic mutation while still allowing status inspection.",
                1000,
                1.0,
                vec![SignalRule::required(SignalKind::PauseFlag, 1.0)],
                vec![PolicyAction::observe_only(
                    "paused.noop",
                    ProfileId::Paused,
                    BackendKind::LocalDscp,
                    "automation paused by user",
                )],
            ),
        ],
    }
}

fn profile(
    id: ProfileId,
    title: &str,
    intent: &str,
    priority: u16,
    confidence_floor: f32,
    signal_rules: Vec<SignalRule>,
    actions: Vec<PolicyAction>,
) -> Profile {
    Profile {
        id,
        title: title.into(),
        intent: intent.into(),
        priority,
        confidence_floor,
        signal_rules,
        actions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn builtin_pack_contains_phase1_profiles() {
        let pack = builtin_profile_pack();
        let ids: BTreeSet<_> = pack.profiles.iter().map(|profile| profile.id).collect();

        assert_eq!(pack.schema_version, BUILTIN_PACK_SCHEMA_VERSION);
        assert!(pack.built_in);
        assert_eq!(ids.len(), 7);
        assert!(ids.contains(&ProfileId::GameBoost));
        assert!(ids.contains(&ProfileId::StreamGuard));
        assert!(ids.contains(&ProfileId::SteamSink));
        assert!(ids.contains(&ProfileId::ProxySmart));
        assert!(ids.contains(&ProfileId::AiWorkLane));
        assert!(ids.contains(&ProfileId::Normal));
        assert!(ids.contains(&ProfileId::Paused));
    }

    #[test]
    fn builtin_action_ids_are_unique() {
        let pack = builtin_profile_pack();
        let mut ids = BTreeSet::new();

        for action in pack
            .profiles
            .iter()
            .flat_map(|profile| profile.actions.iter())
        {
            assert!(ids.insert(action.id.clone()), "duplicate {}", action.id);
        }
    }

    #[test]
    fn paused_profile_has_highest_priority() {
        let pack = builtin_profile_pack();
        let paused = pack
            .profiles
            .iter()
            .find(|profile| profile.id == ProfileId::Paused)
            .unwrap();
        let max_priority = pack
            .profiles
            .iter()
            .map(|profile| profile.priority)
            .max()
            .unwrap();

        assert_eq!(paused.priority, max_priority);
    }

    #[test]
    fn profile_id_parses_cli_aliases() {
        assert_eq!(
            "game_boost".parse::<ProfileId>().unwrap(),
            ProfileId::GameBoost
        );
        assert_eq!("steam".parse::<ProfileId>().unwrap(), ProfileId::SteamSink);
        assert!("wat".parse::<ProfileId>().is_err());
    }
}
