# winqos-rs

Open-source Windows QoS learner and traffic-classification agent.

`winqos-rs` is not a cFosSpeed clone yet. It is the control plane for one:

- observe Windows process connections
- classify traffic as interactive, normal, bulk, or ignored
- learn repeat behavior over time in an auditable JSON state file
- push routing/shaping hints to pluggable backends

The first backend targets `router-qosd` on ASUSWRT/koolshare by refreshing dynamic
`ipset` members. Other backends can target OpenWrt, Windows DSCP policies, WFP, or
WinDivert.

## Why

cFosSpeed-style traffic shaping works because classification and queueing happen
near the bottleneck. On a Windows client, that usually means a kernel driver. This
project starts with a safer open-source shape:

```text
Windows process observer -> learner -> backend hints -> router tc/ipset queues
```

It can later grow a deeper Windows backend:

```text
WFP/WinDivert -> local DSCP/throttle/queue -> router or local egress control
```

## Status

Current MVP:

- Windows TCP connection sampling through PowerShell
- process-name/path classifier
- local online learning state
- routerqosd SSH backend
- dry-run mode
- JSON reports
- safe public defaults: no backend is enabled until configured

Not implemented yet:

- Windows driver-level packet scheduling
- UDP remote-flow attribution
- signed service installer
- WFP/WinDivert backend
- GUI

## Quick Start

```powershell
cargo build
target\debug\winqos-rs.exe init --force
target\debug\winqos-rs.exe sample
target\debug\winqos-rs.exe run --once --dry-run
target\debug\winqos-rs.exe run --once
```

The default config is `winqos.json`. The learner state is `winqos-state.json`.
New configs start with all mutating backends disabled. Enable a backend only
after checking the generated config.

For a routerqosd backend, edit:

```json
{
  "backends": {
    "routerqosd": {
      "enabled": true,
      "host": "192.168.1.1",
      "port": 22,
      "user": "root"
    }
  }
}
```

## Traffic Classes

- `interactive`: terminals, editors, AI clients, SSH
- `normal`: default traffic
- `bulk`: Steam, sync tools, downloaders, repeated learned bulk processes
- `ignore`: proxy engines and local helper processes that should not be marked directly

The classifier is intentionally transparent. A connection report includes the
reason for every decision.

## Backend Contract

Backends should accept generic `RouterCandidate`-style hints:

```json
{
  "set_name": "rqosd_ele4",
  "member": "203.0.113.10,tcp:443",
  "reason": "bulk_process:steam"
}
```

The backend decides how to turn that into a queueing primitive.

The current routerqosd backend runs:

```sh
ipset add rqosd_ele4 203.0.113.10,tcp:443 timeout 30 -exist
```

The router must already have rules that map dynamic ipsets into `tc` classes.

## Learning Model

The first learner is deliberately simple:

- known bulk process: score increases
- known interactive process: score decreases
- process with many repeated remote ports can become bulk over time
- decisions and scores are stored in JSON

This is not an opaque model. It is a policy learner with receipts.

Future learning work:

- per-process byte delta
- remote ASN/domain features
- congestion-event feedback from queue counters
- automatic threshold tuning
- user-approved rule promotion

## Roadmap

The bigger target is a Windows PC optimization suite:

- network QoS
- startup and service hygiene
- power and latency profiles
- proxy and DNS sanity checks
- storage cache hygiene
- receipts, status, and rollback
- local dashboard

See [docs/ROADMAP.md](docs/ROADMAP.md).

## Safety

`winqos-rs` should default to dry-run for new backends. Anything that modifies
packet flow must be visible, reversible, and scoped.

## License

MIT
