# Roadmap

`winqos-rs` starts as a network QoS learner. The larger goal is a Windows network
optimization suite for games, livestreams, downloads, AI tools, and daily work,
with the same discipline as a good playbook: visible changes, scoped risk,
rollback, and receipts.

It should not become a pile of registry myths.

## Product Shape

The suite should have four policy tiers:

- `safe`: read-only checks, reports, reversible user-level settings
- `optional`: system settings with clear rollback
- `experimental`: driver, packet, and scheduler work behind explicit flags
- `banned`: destructive debloat, security bypasses, blanket service killing

Every module should expose:

- `inspect`: show current state
- `apply`: make the smallest useful change
- `status`: show what changed
- `remove`: rollback
- `explain`: show why a rule exists

## Modules

### Network QoS

Current focus.

- process connection sampling
- traffic classification
- online learner
- routerqosd backend
- dry-run reports

Next:

- UDP attribution
- per-process byte deltas
- congestion feedback from queue counters
- OpenWrt backend
- Windows DSCP backend
- WFP marking backend
- WinDivert local scheduling experiment

### Game And Streaming Profiles

- protect latency-sensitive game flows
- guard livestream upload from bulk traffic
- demote Steam and launcher downloads when a match or stream is active
- detect proxy-routed game traffic without marking the proxy engine itself as bulk
- expose per-profile receipts: what was boosted, what was demoted, and why
- support Steam, launcher downloads, Tencent/Delta-style shooters, voice chat, and browser streams as first-class scenarios

### Startup And Services

- measure startup impact
- flag high-cost background apps
- preserve vendor updaters by default
- never disable security services silently
- provide one-click rollback

### Power And Latency

- inspect active power plan
- expose timer resolution and power throttling state
- safe presets for desktop, laptop, game, and workstation
- record battery and thermals before changing anything

### Proxy And DNS

- detect Clash/Mihomo/TUN/system proxy state
- verify DNS leak and IPv6 mismatch
- report latency by endpoint and rule class
- never rewrite user profiles without backup

### Storage Hygiene

- report cache/temp sizes
- clean only known reversible cache paths
- preserve package caches unless user chooses otherwise
- never touch documents, projects, or game saves

### Observability

- local JSON receipts
- before/after command output
- latency samples
- queue counters
- module health
- changelog per apply/remove action

### Dashboard

- local web UI
- PETSCII-inspired network HUD
- read-only by default
- module cards with status, apply, rollback, and details
- game/stream/download profile slots
- block meters for latency, queue pressure, and candidate flow count
- no remote telemetry

## GitHub Milestones

1. Public MVP: current CLI, docs, license, safe defaults
2. Installer: Windows service, scheduled task fallback, uninstall path
3. Backend SDK: routerqosd, OpenWrt, DSCP adapters
4. Network Lab: reproducible latency and throughput benchmarks
5. Suite Core: module registry, receipts, rollback contracts
6. UI: PETSCII local dashboard
7. Experimental Driver Track: WFP/WinDivert prototypes with hard opt-in

## Non-Goals

- no closed-source-only control plane
- no fake "one click faster PC" claims
- no blanket debloat
- no disabling Windows Defender, firewall, or updates by default
- no driver install without explicit experimental mode
- no hidden telemetry
