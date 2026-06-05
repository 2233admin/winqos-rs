use crate::model::ConnectionSample;
use crate::security_paths::powershell_path;
use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use std::process::Command;

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

pub fn collect_windows_connections() -> Result<Vec<ConnectionSample>> {
    let script = r#"
$ErrorActionPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$OutputEncoding = [Console]::OutputEncoding
$procs = @{}
Get-Process | ForEach-Object {
  $procs[[int]$_.Id] = [pscustomobject]@{
    name = $_.ProcessName
    path = $_.Path
  }
}
$rows = @()
$rows += Get-NetTCPConnection -State Established | ForEach-Object {
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
}
$rows += Get-NetUDPEndpoint | ForEach-Object {
  $p = $procs[[int]$_.OwningProcess]
  [pscustomobject]@{
    pid = [int]$_.OwningProcess
    process = if ($p) { $p.name } else { "" }
    path = if ($p) { $p.path } else { "" }
    protocol = "udp"
    remote_addr = "0.0.0.0"
    remote_port = 0
    state = "Bound"
  }
}
$rows | ConvertTo-Json -Compress
"#;
    let output = Command::new(powershell_path())
        .args(["-NoProfile", "-Command", script])
        .output()
        .context("failed to run powershell connection collector")?;
    if !output.status.success() {
        return Err(anyhow!(
            "powershell collector failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    parse_powershell_connections(String::from_utf8_lossy(&output.stdout).trim())
}

pub fn collect_windows_tcp_connections() -> Result<Vec<ConnectionSample>> {
    Ok(collect_windows_connections()?
        .into_iter()
        .filter(|sample| sample.protocol == "tcp")
        .collect())
}

pub fn parse_powershell_connections(stdout: &str) -> Result<Vec<ConnectionSample>> {
    if stdout.is_empty() {
        return Ok(Vec::new());
    }
    let raw: Vec<PowershellConnection> = if stdout.starts_with('[') {
        serde_json::from_str(stdout).context("failed to parse connection array")?
    } else {
        vec![serde_json::from_str(stdout).context("failed to parse connection object")?]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_powershell_object() {
        let samples = parse_powershell_connections(
            r#"{"pid":42,"process":"steam","path":"C:\\Steam\\steam.exe","protocol":"tcp","remote_addr":"203.0.113.10","remote_port":443,"state":"Established"}"#,
        )
        .unwrap();

        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].pid, 42);
        assert_eq!(samples[0].process_name, "steam");
        assert_eq!(samples[0].remote_port, 443);
    }

    #[test]
    fn drops_incomplete_powershell_rows() {
        let samples = parse_powershell_connections(
            r#"[{"pid":42,"process":"steam","remote_addr":"203.0.113.10","remote_port":443},{"process":"missing_pid","remote_addr":"203.0.113.11","remote_port":443}]"#,
        )
        .unwrap();

        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].process_name, "steam");
    }

    #[test]
    fn parses_udp_endpoint_rows() {
        let samples = parse_powershell_connections(
            r#"[{"pid":7,"process":"parsec","path":"C:\\Parsec\\parsec.exe","protocol":"udp","remote_addr":"0.0.0.0","remote_port":0,"state":"Bound"}]"#,
        )
        .unwrap();

        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].protocol, "udp");
        assert_eq!(samples[0].process_name, "parsec");
        assert_eq!(samples[0].remote_addr, "0.0.0.0");
    }
}
