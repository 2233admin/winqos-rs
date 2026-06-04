use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand};
use regex::RegexSet;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DEFAULT_CONFIG: &str = "winqos.json";
const DEFAULT_STATE: &str = "winqos-state.json";

#[derive(Parser, Debug)]
#[command(name = "winqos")]
#[command(about = "Windows QoS learner and pluggable traffic-classification agent")]
struct Cli {
    #[arg(short, long, default_value = DEFAULT_CONFIG)]
    config: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Init {
        #[arg(long)]
        force: bool,
    },
    Sample {
        #[arg(long)]
        json: bool,
    },
    Run {
        #[arg(long)]
        once: bool,
        #[arg(long)]
        dry_run: bool,
    },
    Status,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Config {
    state_path: PathBuf,
    interval_seconds: u64,
    candidate_timeout_seconds: u32,
    learning: LearningConfig,
    classifier: ClassifierConfig,
    backends: BackendConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LearningConfig {
    enabled: bool,
    learn_bulk_after_score: i32,
    score_increment_for_bulk_hint: i32,
    score_increment_for_many_connections: i32,
    score_decrement_for_interactive_hint: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ClassifierConfig {
    bulk_process_patterns: Vec<String>,
    interactive_process_patterns: Vec<String>,
    ignore_process_patterns: Vec<String>,
    bulk_name_patterns: Vec<String>,
    bulk_ports: Vec<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BackendConfig {
    routerqosd: RouterQosdConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RouterQosdConfig {
    enabled: bool,
    host: String,
    port: u16,
    user: String,
    key_path: PathBuf,
    ssh_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ConnectionSample {
    pid: u32,
    process_name: String,
    process_path: String,
    protocol: String,
    remote_addr: String,
    remote_port: u16,
    state: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum TrafficClass {
    Realtime,
    Interactive,
    Normal,
    Bulk,
    Ignore,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ClassifiedConnection {
    sample: ConnectionSample,
    class: TrafficClass,
    reason: String,
    learned_score: i32,
    router_candidate: Option<RouterCandidate>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
struct RouterCandidate {
    set_name: String,
    member: String,
    reason: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct LearnerState {
    updated_unix: u64,
    processes: BTreeMap<String, ProcessLearning>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct ProcessLearning {
    seen: u64,
    bulk_score: i32,
    last_reason: String,
    last_seen_unix: u64,
    remote_ports: BTreeMap<u16, u64>,
}

#[derive(Debug, Serialize)]
struct RunReport {
    updated_unix: u64,
    sample_count: usize,
    class_counts: BTreeMap<String, usize>,
    candidate_count: usize,
    candidates: Vec<RouterCandidate>,
    backend: BackendReport,
}

#[derive(Debug, Serialize)]
struct BackendReport {
    name: String,
    dry_run: bool,
    executed: bool,
    ok: bool,
    stdout: String,
    stderr: String,
}

#[derive(Debug, Deserialize)]
struct PowershellConnection {
    #[serde(rename = "pid")]
    pid: Option<u32>,
    #[serde(rename = "process")]
    process_name: Option<String>,
    #[serde(rename = "path")]
    process_path: Option<String>,
    #[serde(rename = "protocol")]
    protocol: Option<String>,
    #[serde(rename = "remote_addr")]
    remote_addr: Option<String>,
    #[serde(rename = "remote_port")]
    remote_port: Option<u16>,
    #[serde(rename = "state")]
    state: Option<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Init { force } => init_config(&cli.config, force),
        Commands::Sample { json } => {
            let config = Config::load_or_default(&cli.config)?;
            let samples = collect_windows_tcp_connections()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&samples)?);
            } else {
                print_sample_table(&samples, &config);
            }
            Ok(())
        }
        Commands::Run { once, dry_run } => {
            let config = Config::load_or_default(&cli.config)?;
            loop {
                let report = run_cycle(&config, dry_run)?;
                println!("{}", serde_json::to_string_pretty(&report)?);
                if once {
                    break;
                }
                std::thread::sleep(Duration::from_secs(config.interval_seconds.max(2)));
            }
            Ok(())
        }
        Commands::Status => {
            let config = Config::load_or_default(&cli.config)?;
            let state = load_state(&config.state_path)?;
            println!("{}", serde_json::to_string_pretty(&state)?);
            Ok(())
        }
    }
}

impl Config {
    fn load_or_default(path: &Path) -> Result<Self> {
        if path.exists() {
            let text = fs::read_to_string(path)
                .with_context(|| format!("failed to read config {}", path.display()))?;
            serde_json::from_str(&text)
                .with_context(|| format!("failed to parse config {}", path.display()))
        } else {
            Ok(Self::default_for_current_user())
        }
    }

    fn default_for_current_user() -> Self {
        let home = std::env::var("USERPROFILE").unwrap_or_else(|_| ".".to_string());
        Self {
            state_path: PathBuf::from(DEFAULT_STATE),
            interval_seconds: 5,
            candidate_timeout_seconds: 30,
            learning: LearningConfig {
                enabled: true,
                learn_bulk_after_score: 8,
                score_increment_for_bulk_hint: 3,
                score_increment_for_many_connections: 1,
                score_decrement_for_interactive_hint: 4,
            },
            classifier: ClassifierConfig {
                bulk_process_patterns: vec![
                    "steam".into(),
                    "steamwebhelper".into(),
                    "synology".into(),
                    "onedrive".into(),
                    "aria2".into(),
                    "qbittorrent".into(),
                    "transmission".into(),
                    "epicgames".into(),
                    "battle.net".into(),
                    "docker".into(),
                    "ollama".into(),
                ],
                interactive_process_patterns: vec![
                    "weflow".into(),
                    "cursor".into(),
                    "code".into(),
                    "ssh".into(),
                    "windowsterminal".into(),
                    "terminal".into(),
                ],
                ignore_process_patterns: vec![
                    "verge-mihomo".into(),
                    "mihomo".into(),
                    "clash".into(),
                    "python".into(),
                    "pythonw".into(),
                    "pwsh".into(),
                    "powershell".into(),
                ],
                bulk_name_patterns: vec!["download".into(), "update".into(), "sync".into()],
                bulk_ports: vec![
                    80, 443, 6881, 51413, 27014, 27015, 27016, 27017, 27018, 27019, 27020,
                ],
            },
            backends: BackendConfig {
                routerqosd: RouterQosdConfig {
                    enabled: false,
                    host: "192.168.1.1".into(),
                    port: 22,
                    user: "root".into(),
                    key_path: PathBuf::from(format!("{home}\\.ssh\\id_ed25519")),
                    ssh_path: PathBuf::from("ssh.exe"),
                },
            },
        }
    }
}

fn init_config(path: &Path, force: bool) -> Result<()> {
    if path.exists() && !force {
        return Err(anyhow!(
            "{} already exists; pass --force to overwrite",
            path.display()
        ));
    }
    let config = Config::default_for_current_user();
    fs::write(path, serde_json::to_string_pretty(&config)? + "\n")
        .with_context(|| format!("failed to write {}", path.display()))?;
    println!("wrote {}", path.display());
    Ok(())
}

fn collect_windows_tcp_connections() -> Result<Vec<ConnectionSample>> {
    let script = r#"
$ErrorActionPreference = 'SilentlyContinue'
$procs = @{}
Get-Process | ForEach-Object {
  $procs[[int]$_.Id] = [pscustomobject]@{
    name = $_.ProcessName
    path = $_.Path
  }
}
Get-NetTCPConnection -State Established | ForEach-Object {
  $p = $procs[[int]$_.OwningProcess]
  [pscustomobject]@{
    pid = [int]$_.OwningProcess
    process = if ($p) { $p.name } else { "" }
    path = if ($p) { $p.path } else { "" }
    protocol = "tcp"
    remote_addr = $_.RemoteAddress
    remote_port = [int]$_.RemotePort
    state = $_.State.ToString()
  }
} | ConvertTo-Json -Compress
"#;
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", script])
        .output()
        .context("failed to run powershell connection collector")?;
    if !output.status.success() {
        return Err(anyhow!(
            "powershell collector failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        return Ok(Vec::new());
    }
    let raw: Vec<PowershellConnection> = if stdout.starts_with('[') {
        serde_json::from_str(&stdout).context("failed to parse connection array")?
    } else {
        vec![serde_json::from_str(&stdout).context("failed to parse connection object")?]
    };
    Ok(raw
        .into_iter()
        .filter_map(|item| {
            Some(ConnectionSample {
                pid: item.pid?,
                process_name: item.process_name.unwrap_or_default(),
                process_path: item.process_path.unwrap_or_default(),
                protocol: item.protocol.unwrap_or_else(|| "tcp".into()),
                remote_addr: item.remote_addr?,
                remote_port: item.remote_port?,
                state: item.state.unwrap_or_default(),
            })
        })
        .collect())
}

fn print_sample_table(samples: &[ConnectionSample], config: &Config) {
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

fn run_cycle(config: &Config, dry_run: bool) -> Result<RunReport> {
    let samples = collect_windows_tcp_connections()?;
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

struct Classifier {
    bulk_process: RegexSet,
    interactive_process: RegexSet,
    ignore_process: RegexSet,
    bulk_name: RegexSet,
    bulk_ports: BTreeSet<u16>,
    learn_bulk_after_score: i32,
}

impl Classifier {
    fn new(config: &Config) -> Result<Self> {
        Ok(Self {
            bulk_process: RegexSet::new(&config.classifier.bulk_process_patterns)?,
            interactive_process: RegexSet::new(&config.classifier.interactive_process_patterns)?,
            ignore_process: RegexSet::new(&config.classifier.ignore_process_patterns)?,
            bulk_name: RegexSet::new(&config.classifier.bulk_name_patterns)?,
            bulk_ports: config.classifier.bulk_ports.iter().copied().collect(),
            learn_bulk_after_score: config.learning.learn_bulk_after_score,
        })
    }

    fn classify(&self, sample: &ConnectionSample, state: &LearnerState) -> ClassifiedConnection {
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

fn router_candidate(sample: &ConnectionSample, reason: &str) -> Option<RouterCandidate> {
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

fn router_visible_ip(addr: IpAddr) -> bool {
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

fn process_key(sample: &ConnectionSample) -> String {
    if !sample.process_path.is_empty() {
        sample.process_path.to_lowercase()
    } else {
        sample.process_name.to_lowercase()
    }
}

fn update_learning(config: &Config, state: &mut LearnerState, item: &ClassifiedConnection) {
    if !config.learning.enabled {
        return;
    }
    let key = process_key(&item.sample);
    let entry = state.processes.entry(key).or_default();
    entry.seen = entry.seen.saturating_add(1);
    entry.last_seen_unix = now_unix();
    entry.last_reason = item.reason.clone();
    *entry
        .remote_ports
        .entry(item.sample.remote_port)
        .or_default() += 1;
    match item.class {
        TrafficClass::Bulk => entry.bulk_score += config.learning.score_increment_for_bulk_hint,
        TrafficClass::Interactive | TrafficClass::Realtime => {
            entry.bulk_score -= config.learning.score_decrement_for_interactive_hint
        }
        TrafficClass::Normal => {
            if entry.remote_ports.len() >= 8 {
                entry.bulk_score += config.learning.score_increment_for_many_connections;
            }
        }
        TrafficClass::Ignore => {}
    }
    entry.bulk_score = entry.bulk_score.clamp(-32, 64);
    state.updated_unix = now_unix();
}

fn load_state(path: &Path) -> Result<LearnerState> {
    if !path.exists() {
        return Ok(LearnerState::default());
    }
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read state {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("failed to parse state {}", path.display()))
}

fn save_state(path: &Path, state: &LearnerState) -> Result<()> {
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(state)? + "\n")
        .with_context(|| format!("failed to write {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("failed to replace {}", path.display()))?;
    Ok(())
}

fn dedupe_candidates(items: impl Iterator<Item = RouterCandidate>) -> Vec<RouterCandidate> {
    items.collect::<BTreeSet<_>>().into_iter().collect()
}

fn class_counts(items: &[ClassifiedConnection]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for item in items {
        *counts
            .entry(format!("{:?}", item.class).to_lowercase())
            .or_default() += 1;
    }
    counts
}

fn push_routerqosd(
    config: &Config,
    candidates: &[RouterCandidate],
    dry_run: bool,
) -> Result<BackendReport> {
    let backend = &config.backends.routerqosd;
    let mut script = String::from("set -eu\n");
    for item in candidates {
        let set_name = sanitize_set_name(&item.set_name)?;
        let member = sanitize_member(&item.member)?;
        script.push_str(&format!(
            "ipset add {set_name} {member} timeout {} -exist 2>/dev/null || true\n",
            config.candidate_timeout_seconds
        ));
    }
    script.push_str(&format!("echo ok updates={}\n", candidates.len()));

    if dry_run {
        return Ok(BackendReport {
            name: "routerqosd".into(),
            dry_run,
            executed: false,
            ok: true,
            stdout: script,
            stderr: String::new(),
        });
    }

    let target = format!("{}@{}", backend.user, backend.host);
    let mut child = Command::new(&backend.ssh_path)
        .arg("-p")
        .arg(backend.port.to_string())
        .arg("-i")
        .arg(&backend.key_path)
        .arg("-o")
        .arg("IdentitiesOnly=yes")
        .arg("-o")
        .arg("BatchMode=yes")
        .arg("-o")
        .arg("ConnectTimeout=8")
        .arg(target)
        .arg("sh -s")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to spawn ssh backend")?;
    child
        .stdin
        .as_mut()
        .context("failed to open ssh stdin")?
        .write_all(script.as_bytes())
        .context("failed to send backend script")?;
    let output = child
        .wait_with_output()
        .context("failed to wait for ssh backend")?;
    Ok(BackendReport {
        name: "routerqosd".into(),
        dry_run,
        executed: true,
        ok: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).trim().into(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().into(),
    })
}

fn sanitize_set_name(value: &str) -> Result<String> {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        Ok(value.into())
    } else {
        Err(anyhow!("unsafe set name: {value}"))
    }
}

fn sanitize_member(value: &str) -> Result<String> {
    if value.chars().all(|ch| {
        ch.is_ascii_hexdigit() || matches!(ch, ':' | '.' | ',' | 't' | 'c' | 'p' | 'u' | 'd')
    }) {
        Ok(value.into())
    } else {
        Err(anyhow!("unsafe ipset member: {value}"))
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
