use crate::config::Config;
use crate::learning::{LearnerState, process_key};
use crate::model::{ClassifiedConnection, ConnectionSample, RouterCandidate, TrafficClass};
use anyhow::Result;
use regex::RegexSet;
use std::collections::BTreeSet;
use std::net::IpAddr;

pub struct Classifier {
    bulk_process: RegexSet,
    interactive_process: RegexSet,
    ignore_process: RegexSet,
    bulk_name: RegexSet,
    bulk_ports: BTreeSet<u16>,
    learn_bulk_after_score: i32,
}

impl Classifier {
    pub fn new(config: &Config) -> Result<Self> {
        Ok(Self {
            bulk_process: RegexSet::new(&config.classifier.bulk_process_patterns)?,
            interactive_process: RegexSet::new(&config.classifier.interactive_process_patterns)?,
            ignore_process: RegexSet::new(&config.classifier.ignore_process_patterns)?,
            bulk_name: RegexSet::new(&config.classifier.bulk_name_patterns)?,
            bulk_ports: config.classifier.bulk_ports.iter().copied().collect(),
            learn_bulk_after_score: config.learning.learn_bulk_after_score,
        })
    }

    pub fn classify(
        &self,
        sample: &ConnectionSample,
        state: &LearnerState,
    ) -> ClassifiedConnection {
        let label = format!("{} {}", sample.process_name, sample.process_path).to_lowercase();
        let process_key = process_key(sample);
        let learned_score = state
            .processes
            .get(&process_key)
            .map(|item| item.bulk_score)
            .unwrap_or_default();

        let (class, reason) = if self.ignore_process.is_match(&label) {
            (TrafficClass::Ignore, "ignore_process")
        } else if self.interactive_process.is_match(&label) {
            (TrafficClass::Interactive, "interactive_process")
        } else if self.bulk_process.is_match(&label) {
            (TrafficClass::Bulk, "bulk_process")
        } else if learned_score >= self.learn_bulk_after_score {
            (TrafficClass::Bulk, "learned_bulk_process")
        } else if self.bulk_ports.contains(&sample.remote_port) && self.bulk_name.is_match(&label) {
            (TrafficClass::Bulk, "bulk_name_port")
        } else {
            (TrafficClass::Normal, "default_normal")
        };

        let router_candidate = if class == TrafficClass::Bulk {
            router_candidate(sample, reason)
        } else {
            None
        };

        ClassifiedConnection {
            sample: sample.clone(),
            class,
            reason: reason.into(),
            learned_score,
            router_candidate,
        }
    }
}

pub fn router_candidate(sample: &ConnectionSample, reason: &str) -> Option<RouterCandidate> {
    if sample.protocol != "tcp" && sample.protocol != "udp" {
        return None;
    }
    let addr: IpAddr = sample.remote_addr.parse().ok()?;
    if !router_visible_ip(addr) {
        return None;
    }
    let suffix = if addr.is_ipv6() { "6" } else { "4" };
    Some(RouterCandidate {
        set_name: format!("rqosd_ele{suffix}"),
        member: format!(
            "{},{}:{}",
            sample.remote_addr, sample.protocol, sample.remote_port
        ),
        reason: format!("{}:{}", reason, sample.process_name),
    })
}

pub fn router_visible_ip(addr: IpAddr) -> bool {
    if addr.is_loopback() || addr.is_multicast() || addr.is_unspecified() {
        return false;
    }
    match addr {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            if octets[0] == 10 || octets[0] == 127 || octets[0] == 0 {
                return false;
            }
            if octets[0] == 172 && (16..=31).contains(&octets[1]) {
                return false;
            }
            if octets[0] == 192 && octets[1] == 168 {
                return false;
            }
            if octets[0] == 198 && (18..=19).contains(&octets[1]) {
                return false;
            }
            true
        }
        IpAddr::V6(v6) => !v6.is_unique_local() && !v6.is_unicast_link_local(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learning::{LearnerState, ProcessLearning};
    use std::collections::BTreeMap;

    fn sample(process: &str, path: &str, remote_addr: &str, remote_port: u16) -> ConnectionSample {
        ConnectionSample {
            pid: 42,
            process_name: process.into(),
            process_path: path.into(),
            protocol: "tcp".into(),
            remote_addr: remote_addr.into(),
            remote_port,
            state: "Established".into(),
        }
    }

    #[test]
    fn steam_is_bulk_and_router_candidate_for_public_remote() {
        let config = Config::default_for_current_user();
        let classifier = Classifier::new(&config).unwrap();

        let classified = classifier.classify(
            &sample("steam", "C:\\Steam\\steam.exe", "8.8.8.8", 443),
            &LearnerState::default(),
        );

        assert_eq!(classified.class, TrafficClass::Bulk);
        assert_eq!(classified.reason, "bulk_process");
        let candidate = classified.router_candidate.unwrap();
        assert_eq!(candidate.set_name, "rqosd_ele4");
        assert_eq!(candidate.member, "8.8.8.8,tcp:443");
        assert_eq!(candidate.reason, "bulk_process:steam");
    }

    #[test]
    fn proxy_engine_is_ignored_and_never_router_candidate() {
        let config = Config::default_for_current_user();
        let classifier = Classifier::new(&config).unwrap();

        let classified = classifier.classify(
            &sample("verge-mihomo", "", "8.8.8.8", 443),
            &LearnerState::default(),
        );

        assert_eq!(classified.class, TrafficClass::Ignore);
        assert!(classified.router_candidate.is_none());
    }

    #[test]
    fn learned_bulk_score_promotes_unknown_process() {
        let config = Config::default_for_current_user();
        let classifier = Classifier::new(&config).unwrap();
        let sample = sample("RayLinkService", "", "8.8.8.8", 443);
        let mut state = LearnerState::default();
        state.processes.insert(
            process_key(&sample),
            ProcessLearning {
                bulk_score: config.learning.learn_bulk_after_score,
                ..ProcessLearning::default()
            },
        );

        let classified = classifier.classify(&sample, &state);

        assert_eq!(classified.class, TrafficClass::Bulk);
        assert_eq!(classified.reason, "learned_bulk_process");
        assert!(classified.router_candidate.is_some());
    }

    #[test]
    fn private_remote_is_not_router_visible() {
        assert!(!router_visible_ip("192.168.1.10".parse().unwrap()));
        assert!(!router_visible_ip("10.0.0.1".parse().unwrap()));
        assert!(!router_visible_ip("172.16.0.1".parse().unwrap()));
        assert!(router_visible_ip("8.8.8.8".parse().unwrap()));
    }

    #[test]
    fn bulk_candidate_ignores_private_remote() {
        let candidate = router_candidate(&sample("steam", "", "192.168.1.10", 443), "bulk");

        assert!(candidate.is_none());
    }

    #[test]
    fn ipv6_candidate_uses_v6_set() {
        let mut sample = sample("steam", "", "2001:4860:4860::8888", 443);
        sample.protocol = "udp".into();

        let candidate = router_candidate(&sample, "bulk").unwrap();

        assert_eq!(candidate.set_name, "rqosd_ele6");
        assert_eq!(candidate.member, "2001:4860:4860::8888,udp:443");
    }

    #[test]
    fn state_map_can_be_empty_without_panicking() {
        let state = LearnerState {
            updated_unix: 0,
            processes: BTreeMap::new(),
        };
        let config = Config::default_for_current_user();
        let classifier = Classifier::new(&config).unwrap();

        let classified = classifier.classify(&sample("browser", "", "8.8.8.8", 443), &state);

        assert_eq!(classified.class, TrafficClass::Normal);
    }
}
