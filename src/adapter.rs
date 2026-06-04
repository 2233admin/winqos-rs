use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkAdapter {
    pub name: String,
    pub interface_description: String,
    pub status: String,
    pub link_speed: String,
    pub link_speed_mbps: Option<u64>,
    pub mac_address: String,
    pub media_type: String,
    pub physical_media_type: String,
    pub if_index: Option<u32>,
    pub wireless: bool,
    pub virtual_adapter: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterTier {
    LocalFullBlood,
    RouterLinked,
    WifiLatencyGuard,
    ProxyTunnelGuard,
    Conservative,
    ObserveOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterRecommendation {
    pub adapter: String,
    pub tier: AdapterTier,
    pub local_strategy: String,
    pub router_strategy: String,
    pub proxy_strategy: String,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterPlan {
    pub primary_adapter: Option<String>,
    pub adapters: Vec<NetworkAdapter>,
    pub recommendations: Vec<AdapterRecommendation>,
    pub summary: String,
}

#[derive(Debug, Deserialize)]
struct RawAdapter {
    name: Option<String>,
    interface_description: Option<String>,
    status: Option<String>,
    link_speed: Option<String>,
    mac_address: Option<String>,
    media_type: Option<String>,
    physical_media_type: Option<String>,
    if_index: Option<u32>,
}

pub fn collect_adapters() -> Result<Vec<NetworkAdapter>> {
    let script = r#"
$ErrorActionPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$OutputEncoding = [Console]::OutputEncoding
Get-NetAdapter | Sort-Object -Property ifIndex | ForEach-Object {
  [pscustomobject]@{
    name = $_.Name
    interface_description = $_.InterfaceDescription
    status = $_.Status.ToString()
    link_speed = $_.LinkSpeed
    mac_address = $_.MacAddress
    media_type = if ($_.MediaType) { $_.MediaType.ToString() } else { "" }
    physical_media_type = if ($_.PhysicalMediaType) { $_.PhysicalMediaType.ToString() } else { "" }
    if_index = [int]$_.ifIndex
  }
} | ConvertTo-Json -Compress
"#;
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-Command", script])
        .output()
        .context("failed to run powershell adapter collector")?;
    if !output.status.success() {
        return Err(anyhow!(
            "powershell adapter collector failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    parse_adapters(String::from_utf8_lossy(&output.stdout).trim())
}

pub fn parse_adapters(stdout: &str) -> Result<Vec<NetworkAdapter>> {
    if stdout.is_empty() {
        return Ok(Vec::new());
    }
    let raw: Vec<RawAdapter> = if stdout.starts_with('[') {
        serde_json::from_str(stdout).context("failed to parse adapter array")?
    } else {
        vec![serde_json::from_str(stdout).context("failed to parse adapter object")?]
    };
    Ok(raw
        .into_iter()
        .map(|item| {
            let name = item.name.unwrap_or_default();
            let interface_description = item.interface_description.unwrap_or_default();
            let media_type = item.media_type.unwrap_or_default();
            let physical_media_type = item.physical_media_type.unwrap_or_default();
            let link_speed = item.link_speed.unwrap_or_default();
            let label = format!(
                "{} {} {} {}",
                name, interface_description, media_type, physical_media_type
            )
            .to_lowercase();
            NetworkAdapter {
                name,
                interface_description,
                status: item.status.unwrap_or_default(),
                link_speed_mbps: parse_link_speed_mbps(&link_speed),
                link_speed,
                mac_address: item.mac_address.unwrap_or_default(),
                media_type,
                physical_media_type,
                if_index: item.if_index,
                wireless: contains_any(&label, &["wi-fi", "wifi", "wireless", "802.11"]),
                virtual_adapter: contains_any(
                    &label,
                    &[
                        "virtual",
                        "vpn",
                        "tap",
                        "tun",
                        "wintun",
                        "wireguard",
                        "tailscale",
                        "zerotier",
                        "hyper-v",
                        "clash",
                        "mihomo",
                    ],
                ),
            }
        })
        .collect())
}

pub fn plan_for_adapters(adapters: Vec<NetworkAdapter>) -> AdapterPlan {
    let primary_adapter = adapters
        .iter()
        .filter(|adapter| adapter.status.eq_ignore_ascii_case("up"))
        .max_by_key(|adapter| {
            (
                if adapter.virtual_adapter { 0 } else { 1 },
                if adapter.wireless { 0 } else { 1 },
                adapter.link_speed_mbps.unwrap_or_default(),
            )
        })
        .map(|adapter| adapter.name.clone());
    let recommendations = adapters.iter().map(recommend_adapter).collect::<Vec<_>>();
    let summary = if let Some(primary) = &primary_adapter {
        format!("{primary} selected as primary adapter for local QoS planning")
    } else {
        "no active adapter found; stay observe-only".into()
    };

    AdapterPlan {
        primary_adapter,
        adapters,
        recommendations,
        summary,
    }
}

pub fn parse_link_speed_mbps(value: &str) -> Option<u64> {
    let normalized = value.trim().replace(',', "");
    let mut parts = normalized.split_whitespace();
    let amount = parts.next()?.parse::<f64>().ok()?;
    let unit = parts.next().unwrap_or("mbps").to_lowercase();
    let mbps = if unit.starts_with('g') {
        amount * 1000.0
    } else if unit.starts_with('m') {
        amount
    } else if unit.starts_with('k') {
        amount / 1000.0
    } else {
        return None;
    };
    Some(mbps.round().max(0.0) as u64)
}

fn recommend_adapter(adapter: &NetworkAdapter) -> AdapterRecommendation {
    if !adapter.status.eq_ignore_ascii_case("up") {
        return AdapterRecommendation {
            adapter: adapter.name.clone(),
            tier: AdapterTier::ObserveOnly,
            local_strategy: "inspect only; adapter is not active".into(),
            router_strategy: "do not push router classes for inactive adapters".into(),
            proxy_strategy: "ignore for proxy shaping until link is up".into(),
            notes: vec!["adapter is not up".into()],
        };
    }

    if adapter.virtual_adapter {
        return AdapterRecommendation {
            adapter: adapter.name.clone(),
            tier: AdapterTier::ProxyTunnelGuard,
            local_strategy: "avoid marking the tunnel engine directly; classify visible apps first"
                .into(),
            router_strategy:
                "router sees the proxy endpoint, so use app-side DSCP plus proxy endpoint reports"
                    .into(),
            proxy_strategy:
                "split proxy traffic by visible task: realtime, AI work, normal, and bulk".into(),
            notes: vec!["virtual or tunnel adapter detected".into()],
        };
    }

    if adapter.wireless {
        return AdapterRecommendation {
            adapter: adapter.name.clone(),
            tier: AdapterTier::WifiLatencyGuard,
            local_strategy:
                "protect realtime and AI flows; keep bulk conservative during queue pressure".into(),
            router_strategy:
                "router cooperation is useful, but Wi-Fi airtime remains the bottleneck".into(),
            proxy_strategy: "keep proxy engines unmarked unless a visible realtime app is active"
                .into(),
            notes: vec!["wireless adapter detected".into()],
        };
    }

    match adapter.link_speed_mbps.unwrap_or_default() {
        speed if speed >= 2500 => AdapterRecommendation {
            adapter: adapter.name.clone(),
            tier: AdapterTier::LocalFullBlood,
            local_strategy:
                "enable full local DSCP plan for realtime, AI, remote control, and bulk sink".into(),
            router_strategy:
                "pair local DSCP with routerqosd class ipsets for bottleneck-side queueing".into(),
            proxy_strategy:
                "shape proxy by visible app intent; never boost the whole tunnel blindly".into(),
            notes: vec![format!("high-speed wired link: {speed} Mbps")],
        },
        speed if speed >= 1000 => AdapterRecommendation {
            adapter: adapter.name.clone(),
            tier: AdapterTier::RouterLinked,
            local_strategy:
                "use local DSCP for process intent and leave scheduling to the bottleneck".into(),
            router_strategy:
                "routerqosd class ipsets should be enabled when the router is the WAN bottleneck"
                    .into(),
            proxy_strategy: "proxy endpoint reports matter because router visibility is compressed"
                .into(),
            notes: vec![format!("wired link: {speed} Mbps")],
        },
        speed if speed > 0 => AdapterRecommendation {
            adapter: adapter.name.clone(),
            tier: AdapterTier::Conservative,
            local_strategy:
                "prefer realtime protection and bulk demotion; avoid aggressive experiments".into(),
            router_strategy: "router cooperation should be dry-run verified before live use".into(),
            proxy_strategy: "only protect proxy traffic when the visible task is known".into(),
            notes: vec![format!("lower-speed link: {speed} Mbps")],
        },
        _ => AdapterRecommendation {
            adapter: adapter.name.clone(),
            tier: AdapterTier::Conservative,
            local_strategy: "use safe DSCP dry-runs until link speed is known".into(),
            router_strategy: "inspect router path before applying class ipsets".into(),
            proxy_strategy: "keep proxy shaping in observe mode".into(),
            notes: vec!["link speed unknown".into()],
        },
    }
}

fn contains_any(label: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| label.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_link_speed_units() {
        assert_eq!(parse_link_speed_mbps("2.5 Gbps"), Some(2500));
        assert_eq!(parse_link_speed_mbps("1 Gbps"), Some(1000));
        assert_eq!(parse_link_speed_mbps("100 Mbps"), Some(100));
        assert_eq!(parse_link_speed_mbps("Unknown"), None);
    }

    #[test]
    fn parses_adapter_json_and_flags_wireless() {
        let adapters = parse_adapters(
            r#"[{"name":"Wi-Fi","interface_description":"Intel Wi-Fi 7","status":"Up","link_speed":"1.2 Gbps","mac_address":"00-11","media_type":"Native 802.11","physical_media_type":"Wireless LAN","if_index":7}]"#,
        )
        .unwrap();

        assert_eq!(adapters.len(), 1);
        assert_eq!(adapters[0].link_speed_mbps, Some(1200));
        assert!(adapters[0].wireless);
        assert!(!adapters[0].virtual_adapter);
    }

    #[test]
    fn high_speed_wired_gets_full_blood_plan() {
        let adapter = NetworkAdapter {
            name: "Ethernet".into(),
            interface_description: "Realtek 2.5GbE".into(),
            status: "Up".into(),
            link_speed: "2.5 Gbps".into(),
            link_speed_mbps: Some(2500),
            mac_address: String::new(),
            media_type: "802.3".into(),
            physical_media_type: "802.3".into(),
            if_index: Some(1),
            wireless: false,
            virtual_adapter: false,
        };

        let plan = plan_for_adapters(vec![adapter]);

        assert_eq!(plan.primary_adapter, Some("Ethernet".into()));
        assert_eq!(plan.recommendations[0].tier, AdapterTier::LocalFullBlood);
    }

    #[test]
    fn virtual_adapter_gets_proxy_tunnel_plan() {
        let adapter = NetworkAdapter {
            name: "Wintun".into(),
            interface_description: "WireGuard Tunnel".into(),
            status: "Up".into(),
            link_speed: "1 Gbps".into(),
            link_speed_mbps: Some(1000),
            mac_address: String::new(),
            media_type: String::new(),
            physical_media_type: String::new(),
            if_index: Some(3),
            wireless: false,
            virtual_adapter: true,
        };

        let plan = plan_for_adapters(vec![adapter]);

        assert_eq!(plan.recommendations[0].tier, AdapterTier::ProxyTunnelGuard);
    }

    #[test]
    fn primary_prefers_physical_adapter_over_fast_virtual_tunnel() {
        let physical = NetworkAdapter {
            name: "Ethernet".into(),
            interface_description: "Marvell 10G".into(),
            status: "Up".into(),
            link_speed: "1 Gbps".into(),
            link_speed_mbps: Some(1000),
            mac_address: String::new(),
            media_type: "802.3".into(),
            physical_media_type: "802.3".into(),
            if_index: Some(1),
            wireless: false,
            virtual_adapter: false,
        };
        let tunnel = NetworkAdapter {
            name: "wt0".into(),
            interface_description: "WireGuard Tunnel".into(),
            status: "Up".into(),
            link_speed: "100 Gbps".into(),
            link_speed_mbps: Some(100000),
            mac_address: String::new(),
            media_type: "IP".into(),
            physical_media_type: "Unspecified".into(),
            if_index: Some(2),
            wireless: false,
            virtual_adapter: true,
        };

        let plan = plan_for_adapters(vec![physical, tunnel]);

        assert_eq!(plan.primary_adapter, Some("Ethernet".into()));
    }
}
