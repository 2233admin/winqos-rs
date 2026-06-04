# winqos-rs

Open-source Windows network optimization engine and traffic-classification agent.

Squeeze every usable millisecond out of Windows networking, without hiding what
changed.

`winqos-rs` is not a cFosSpeed clone yet. It is the control plane for one:

- observe Windows process connections
- classify traffic as interactive, normal, bulk, or ignored
- learn repeat behavior over time in an auditable JSON state file
- push routing/shaping hints to pluggable backends

The goal is simple: squeeze the network until games, livestreams, AI tools,
browsers, and downloads stop ruining each other. Steam can download without
bulldozing your match. Tencent/Delta-style shooters can stay protected.
Streaming upload can stay guarded while the rest of the machine keeps working.

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

Current Phase 1 CLI:

- Windows TCP connection sampling through PowerShell
- process-name/path classifier
- built-in autopilot profiles: `game_boost`, `stream_guard`, `steam_sink`,
  `proxy_smart`, `ai_work_lane`, `normal`, `paused`
- confidence-scored decisions with explainable signals
- local online learning and user feedback state
- PETSCII-style `status` and `explain`
- DSCP-first local backend in dry-run by default, with live apply guarded by
  admin checks and concrete process/path selectors
- routerqosd backend behind the backend trait
- disabled WinDivert lab stub
- daemon loop, pause/resume, receipts, and rollback
- Network Lab baseline/run/report plus validation-gated optimizer
- safe public defaults: no backend is enabled until configured

Not implemented yet:

- Windows driver-level packet scheduling
- UDP remote-flow attribution
- signed service installer
- WFP production backend
- WinDivert production backend
- GUI

## Quick Start

```powershell
cargo build
target\debug\winqos-rs.exe init --force
target\debug\winqos-rs.exe sample
target\debug\winqos-rs.exe run --once --dry-run
target\debug\winqos-rs.exe status
target\debug\winqos-rs.exe explain
```

The default config is `winqos.json`. New configs start with all mutating backends
disabled. Enable a backend only after checking the generated config.

Runtime files stay local and are ignored by git:

- `winqos-state.json`
- `winqos-receipts.jsonl`
- `winqos-feedback.jsonl`
- `winqos-policy-state.json`
- `winqos-lab-history.jsonl`
- `profiles/*.current.json`
- `profiles/*.best.json`
- `profiles/*.history.jsonl`

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

## Autopilot

`run --once --dry-run` observes current traffic, selects a profile, explains the
signals, and writes dry-run receipts for planned actions. It does not need a user
to pick rules first.

Useful controls:

```powershell
target\debug\winqos-rs.exe feedback prefer game_boost
target\debug\winqos-rs.exe feedback bad --last
target\debug\winqos-rs.exe pause --reason match
target\debug\winqos-rs.exe resume
target\debug\winqos-rs.exe rollback --last
```

## Backend Contract

Backends implement:

```text
inspect
apply
status
remove
explain
capabilities
```

The DSCP backend is the default local direction. Dry-run is the default:

```powershell
target\debug\winqos-rs.exe backend dscp inspect
target\debug\winqos-rs.exe backend dscp apply-dscp manual.game --dscp 46 --process-path C:\Games\game.exe
target\debug\winqos-rs.exe backend dscp remove manual.game
```

Live DSCP apply requires `--live`, elevation, and a concrete selector. Broad
traffic-class selectors remain dry-run only until resolved to specific processes.

The routerqosd backend still accepts dynamic ipset hints:

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

WinDivert is present only as an explicit disabled lab backend.

## Network Lab

Lab commands record local reports and feed the policy optimizer:

```powershell
target\debug\winqos-rs.exe lab baseline
target\debug\winqos-rs.exe lab run game
target\debug\winqos-rs.exe lab run stream
target\debug\winqos-rs.exe lab report
target\debug\winqos-rs.exe lab optimize steam_sink
```

The optimizer keeps a candidate only when its score improves. Equal or worse
candidates are rejected and rollback is attempted from the last receipt.

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

The bigger target is a Windows network optimization suite:

- network QoS
- game and livestream boost profiles
- Steam/download demotion
- startup and service hygiene
- power and latency profiles
- proxy and DNS sanity checks
- storage cache hygiene
- receipts, status, and rollback
- PETSCII-style local dashboard

See [docs/ROADMAP.md](docs/ROADMAP.md).

Phase 1 product plan: [docs/designs/phase1-autopilot.md](docs/designs/phase1-autopilot.md).

## Safety

`winqos-rs` should default to dry-run for new backends. Anything that modifies
packet flow must be visible, reversible, and scoped.

## License

MIT
