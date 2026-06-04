# Architecture

`winqos-rs` has four layers:

```text
collector -> classifier -> learner -> backend
```

## Collector

Collectors produce facts. They must not decide priority.

Current collector:

- Windows TCP connections via PowerShell `Get-NetTCPConnection`
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

Current backend:

- routerqosd over SSH
- `ipset add rqosd_ele4/rqosd_ele6 ...`

Planned backends:

- OpenWrt SSH/ubus
- Windows DSCP policy
- WFP marking
- WinDivert local scheduler
