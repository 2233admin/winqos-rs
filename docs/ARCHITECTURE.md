# Architecture

`winqos-rs` has four layers:

```text
collector -> classifier -> learner -> autopilot -> backend
```

## Collector

Collectors produce facts. They must not decide priority.

Current collector:

- Windows TCP connections via PowerShell `Get-NetTCPConnection`
- Windows UDP endpoints via PowerShell `Get-NetUDPEndpoint`
- Windows adapter facts via PowerShell `Get-NetAdapter`
- process name/path via `Get-Process`

Planned collectors:

- ETW/WPR counters
- WFP flow metadata
- WinDivert packet stream
- browser/proxy controller metadata

## Classifier

Classifiers convert a connection into a class and reason:

- `realtime`
- `interactive`
- `normal`
- `bulk`
- `ignore`

The reason is part of the API. No silent magic.

## Learner

The learner updates process scores in `winqos-state.json`.

The initial learner is not ML. It is online scoring with transparent thresholds.
That is intentional: network control needs receipts before cleverness.

## Backend

Backends consume candidates and mutate an external policy surface.

Current backends:

- Windows DSCP policy through `New-NetQosPolicy`
- routerqosd over SSH with class-specific dynamic ipsets
- disabled WinDivert lab backend

The runner resolves broad traffic-class policy actions into concrete process
path/name selectors before local DSCP apply. If it cannot resolve a safe selector,
the action remains dry-run only.

Planned backends:

- OpenWrt SSH/ubus
- WFP marking
- WinDivert local scheduler
