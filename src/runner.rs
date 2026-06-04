use crate::backend::{dedupe_candidates, push_routerqosd};
use crate::classifier::Classifier;
use crate::collector::collect_windows_tcp_connections;
use crate::config::Config;
use crate::learning::{load_state, now_unix, save_state, update_learning};
use crate::model::{BackendReport, ClassifiedConnection, ConnectionSample, RunReport};
use anyhow::{Context, Result, anyhow};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

pub fn init_config(path: &Path, force: bool) -> Result<()> {
    if path.exists() && !force {
        return Err(anyhow!(
            "{} already exists; pass --force to overwrite",
            path.display()
        ));
    }
    let config = Config::default_for_current_user();
    fs::write(path, serde_json::to_string_pretty(&config)? + "\n")
        .with_context(|| format!("failed to write config {}", path.display()))?;
    println!("wrote {}", path.display());
    Ok(())
}

pub fn print_sample_table(samples: &[ConnectionSample], config: &Config) {
    let mut state = load_state(&config.state_path).unwrap_or_default();
    let classifier = Classifier::new(config).expect("invalid classifier regex");
    for conn in samples.iter().take(80) {
        let classified = classifier.classify(conn, &state);
        println!(
            "{:<18} {:<6} {:<5} {:<22} {:<12} {}",
            conn.process_name,
            conn.pid,
            conn.remote_port,
            conn.remote_addr,
            format!("{:?}", classified.class).to_lowercase(),
            classified.reason
        );
        update_learning(config, &mut state, &classified);
    }
}

pub fn run_cycle(config: &Config, dry_run: bool) -> Result<RunReport> {
    run_cycle_with_samples(config, collect_windows_tcp_connections()?, dry_run)
}

pub fn run_cycle_with_samples(
    config: &Config,
    samples: Vec<ConnectionSample>,
    dry_run: bool,
) -> Result<RunReport> {
    let mut state = load_state(&config.state_path)?;
    let classifier = Classifier::new(config)?;
    let classified: Vec<_> = samples
        .iter()
        .map(|sample| classifier.classify(sample, &state))
        .collect();
    for item in &classified {
        update_learning(config, &mut state, item);
    }
    save_state(&config.state_path, &state)?;

    let candidates = dedupe_candidates(
        classified
            .iter()
            .filter_map(|item| item.router_candidate.clone()),
    );
    let backend = if config.backends.routerqosd.enabled {
        push_routerqosd(config, &candidates, dry_run)?
    } else {
        BackendReport {
            name: "routerqosd".into(),
            dry_run,
            executed: false,
            ok: true,
            stdout: "backend disabled".into(),
            stderr: String::new(),
        }
    };
    Ok(RunReport {
        updated_unix: now_unix(),
        sample_count: samples.len(),
        class_counts: class_counts(&classified),
        candidate_count: candidates.len(),
        candidates,
        backend,
    })
}

pub fn class_counts(items: &[ClassifiedConnection]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for item in items {
        *counts
            .entry(format!("{:?}", item.class).to_lowercase())
            .or_default() += 1;
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempState {
        path: std::path::PathBuf,
    }

    impl Drop for TempState {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
        }
    }

    fn temp_state_config() -> (Config, TempState) {
        let mut config = Config::default_for_current_user();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        config.state_path = std::env::temp_dir().join(format!("winqos-test-{unique}.json"));
        let guard = TempState {
            path: config.state_path.clone(),
        };
        (config, guard)
    }

    fn sample(process: &str) -> ConnectionSample {
        ConnectionSample {
            pid: 1,
            process_name: process.into(),
            process_path: String::new(),
            protocol: "tcp".into(),
            remote_addr: "8.8.8.8".into(),
            remote_port: 443,
            state: "Established".into(),
        }
    }

    #[test]
    fn run_cycle_reports_disabled_backend_without_executing() {
        let (config, _guard) = temp_state_config();

        let report = run_cycle_with_samples(&config, vec![sample("steam")], true).unwrap();

        assert_eq!(report.sample_count, 1);
        assert_eq!(report.candidate_count, 1);
        assert_eq!(report.class_counts.get("bulk"), Some(&1));
        assert_eq!(report.backend.stdout, "backend disabled");
        assert!(!report.backend.executed);
    }
}
