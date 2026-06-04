# Phase 1 Plan - WQ Autopilot

Generated: 2026-06-04  
Mode: Selective Expansion  
Baseline decision: Profile Engine + Receipts Core  
Primary priority: local traffic shaping first  
Current commit when planned: `e5a6f52`

## Product Thesis

`winqos-rs` should not ask players to tune network rules.

It should install, observe what the user is doing, choose a safe profile, apply the
smallest useful network action, measure whether it helped, keep the better policy,
and leave a receipt plus rollback path.

Core promise:

> Squeeze every usable millisecond out of Windows networking, without hiding what
> changed.

## Accepted Scope

### D1 - Profile Engine + Receipts Core

Use a product-grade profile model instead of one-off classifier rules.

Profiles:

- `game_boost`
- `stream_guard`
- `steam_sink`
- `proxy_smart`
- `ai_work_lane`
- `normal`
- `paused`

### D2 - Autopilot Profile Pack

Profiles are automatic by default. The user should not have to choose a rule set
before getting value.

Built-in packs for Phase 1:

- Steam and launcher downloads
- Tencent/Delta-style shooter traffic
- OBS and livestream upload
- Discord and voice chat
- Clash/Mihomo proxy awareness
- AI and work tools

### D3 - Confidence System

Every automatic decision must carry confidence and explainable signals.

Example:

```json
{
  "profile": "game_boost",
  "confidence": 0.91,
  "signals": [
    "foreground_process:game",
    "udp_small_packet_flow",
    "steam_download_active",
    "mihomo_tunnel_active"
  ],
  "actions": [
    "protect_game_flow",
    "demote_steam_bulk",
    "leave_proxy_engine_unmarked"
  ]
}
```

### D4 - Receipts And Rollback

Every mutation needs a receipt and a rollback path.

Backend contract:

```text
inspect()
apply(action)
status(action_id)
remove(action_id)
explain(action_id)
capabilities()
```

No backend may mutate network state without producing a receipt.

### D5 - DSCP-First Local Backend

Phase 1 local shaping starts with Windows DSCP/QoS marking plus permission checks,
dry-run, receipts, and rollback.

WinDivert enters as an experimental lab backend only. WFP stays future-facing.

Router compatibility remains important, but router backends are assistive, not the
product core.

### D6 - Network Lab

Add a benchmark harness so "squeeze" is measurable.

Commands:

```powershell
winqos-rs lab baseline
winqos-rs lab run game
winqos-rs lab run stream
winqos-rs lab report
```

Metrics:

- `latency_avg_ms`
- `latency_p95_ms`
- `jitter_ms`
- `packet_loss_pct`
- `download_active`
- `upload_pressure`
- `profile_confidence`
- `actions_applied`
- `rollback_ready`

### D7 - PETSCII CLI Experience

Dashboard is not Phase 1. The CLI is the product surface.

The CLI should feel like a PETSCII network cartridge:

```text
+-------------------------- WQ OVERDRIVE --------------------------+
| PROFILE      GAME BOOST       CONF 0.91       BACKEND DSCP        |
| STEAM        DEMOTED          STREAM GUARD    READY               |
| ROLLBACK     ARMED            RECEIPTS        12                  |
+-------------------------------------------------------------------+

PACKET MAP
GAME    ########..  protected
STEAM   ##........  sink
STREAM  ######....  guarded

INFORMATION
15:42:01 GAME BOOST applied by foreground_process + udp_small_flow
15:42:03 STEAM SINK active because download collided with match
```

### D8 - Always-On Autopilot Daemon

Phase 1 should include an always-on agent:

- start at boot
- observe foreground/game/stream/download/proxy signals
- switch profiles automatically
- apply local backend actions
- write receipts
- expose CLI status and pause controls

### D9 - SkillOpt-Style Policy Optimizer

Add a minimal validation-gated optimizer:

```text
observe live session
  -> choose candidate policy
  -> apply safely
  -> measure score
  -> keep if better
  -> rollback if worse
  -> write receipt
```

This is not "AI magic". It is a score gate.

Policy artifacts:

```text
profiles/game_boost.current.json
profiles/game_boost.best.json
profiles/game_boost.history.jsonl
```

### D10 - Built-In Packs Now, Community Packs Later

Phase 1 uses built-in packs only, but the schema must be future-compatible with
community packs.

Not in Phase 1:

- marketplace
- remote auto-update
- unsigned external pack loading
- community pack execution by default

### D11 - User Feedback Learning

Users should rarely configure, but corrections must stick.

Commands:

```powershell
winqos-rs feedback good --last
winqos-rs feedback bad --last
winqos-rs feedback rollback --last
winqos-rs feedback ignore-process steam.exe --until game-exits
winqos-rs feedback prefer game_boost
```

Files:

```text
winqos-feedback.jsonl
winqos-policy-state.json
```

### D12 - CLI-Only Pause Switch

No tray app in Phase 1. But automation must have a brake.

Commands:

```powershell
winqos-rs pause
winqos-rs resume
winqos-rs status
```

## Not In Scope

- Full dashboard or tray app
- Remote profile marketplace
- External policy packs loaded by default
- WinDivert as default backend
- WFP production backend
- Cloud sync
- Telemetry upload
- Blanket Windows debloat
- Security bypasses
- Router-only product path

## What Already Exists

- `src/main.rs` already collects Windows TCP connections through PowerShell.
- `Classifier` already splits traffic into `interactive`, `normal`, `bulk`, and `ignore`.
- `LearnerState` already stores process learning in JSON.
- `routerqosd` backend already emits ipset updates over SSH.
- `RunReport` already contains counts, candidates, and backend result.

These should be reused, but the single-file MVP should be split into modules before
the scope grows.

## Architecture

```text
                          +---------------------+
                          |  Autopilot Daemon   |
                          +----------+----------+
                                     |
                                     v
+-------------+     +---------+   +----------+   +----------------+
| Collectors  +---->| Signals +-->| Profiles +-->| Policy Candidate|
+------+------+     +----+----+   +----+-----+   +--------+-------+
       |                 |             |                  |
       |                 |             v                  v
       |                 |       +------------+     +-------------+
       |                 +------>| Confidence |     | Lab Scoring |
       |                         +-----+------+     +------+------+
       |                               |                   |
       v                               v                   v
+------+-------+                +------+-------------------+------+
| Foreground   |                | Validation Gate                 |
| TCP/UDP      |                | keep if better, rollback if not |
| Process      |                +------+-------------------+------+
| Proxy State  |                       |
+--------------+                       v
                              +--------+---------+
                              | Backend Trait    |
                              +---+----+----+----+
                                  |    |    |
                                  v    v    v
                               DSCP Router WinDivert Lab
                                  |
                                  v
                           +------+------+
                           | Receipts    |
                           | Rollback    |
                           | Feedback    |
                           +-------------+
```

## Module Plan

```text
src/
  main.rs
  config.rs
  collector/
    mod.rs
    windows_tcp.rs
    foreground.rs
  classify/
    mod.rs
    signals.rs
  profile/
    mod.rs
    packs.rs
    confidence.rs
  policy/
    mod.rs
    optimizer.rs
    scoring.rs
  backend/
    mod.rs
    dscp.rs
    routerqosd.rs
    windivert_lab.rs
  receipt/
    mod.rs
    store.rs
  feedback/
    mod.rs
  cli/
    petscii.rs
```

## Backend Trait

```rust
trait Backend {
    fn name(&self) -> &'static str;
    fn capabilities(&self) -> BackendCapabilities;
    fn inspect(&self) -> Result<BackendStatus>;
    fn apply(&self, action: &PolicyAction) -> Result<ApplyReceipt>;
    fn status(&self, action_id: &ActionId) -> Result<ActionStatus>;
    fn remove(&self, action_id: &ActionId) -> Result<RollbackReceipt>;
    fn explain(&self, action_id: &ActionId) -> Result<ActionExplanation>;
}
```

## Data Files

Runtime files should stay local and ignored by git:

```text
winqos.json
winqos-state.json
winqos-receipts.jsonl
winqos-feedback.jsonl
winqos-policy-state.json
profiles/*.current.json
profiles/*.best.json
profiles/*.history.jsonl
```

Consider moving defaults to `%ProgramData%\winqos-rs\` for installed daemon mode,
while keeping repo-local files for development.

## Error And Rescue Map

| Codepath | What Can Go Wrong | Rescue Action | User Sees |
|---|---|---|---|
| collector windows tcp | PowerShell unavailable or slow | timeout, return degraded collector status | `collector degraded` |
| foreground collector | foreground process inaccessible | continue with lower confidence | `signal missing` |
| profile decision | conflicting profiles | choose highest confidence above threshold | chosen profile plus competing signals |
| DSCP backend apply | not elevated | dry-run or request admin setup | `backend needs admin` |
| DSCP backend remove | policy missing | mark rollback already clear | `rollback already clear` |
| router backend apply | SSH timeout | no mutation, receipt failure | `router backend failed` |
| lab scoring | target unreachable | score marked inconclusive | `lab inconclusive` |
| optimizer candidate | after score worse | rollback previous action | `candidate rejected` |
| feedback write | file locked | retry then report failure | `feedback not saved` |
| daemon loop | panic in one cycle | catch at cycle boundary, log, continue paused | `daemon paused after error` |

Any action that mutates network state must fail closed. No silent apply.

## Security Boundaries

- No external policy pack execution in Phase 1.
- No remote auto-update of packs.
- No driver installation by default.
- WinDivert lab requires explicit experimental mode.
- DSCP backend must detect admin rights before mutation.
- Router backend must keep shell argument sanitization.
- Receipt files must not store secrets.
- Config must never include router password fields.

## Test Plan

Unit tests:

- profile selection with no signals
- profile selection with game plus Steam download
- profile selection with OBS upload plus download
- proxy process ignored but proxy context retained
- confidence scoring thresholds
- receipt serialization
- rollback receipt path
- feedback good/bad/rollback weight changes
- router candidate sanitization
- DSCP backend dry-run

Integration tests:

- `sample --json` with mocked collector JSON
- `run --once --dry-run` with built-in pack
- `lab baseline` with mocked probes
- optimizer accepts better candidate
- optimizer rejects worse candidate and rolls back

CLI snapshot tests:

- `status` PETSCII output
- paused daemon state
- profile explanation output
- lab report output

## Implementation Sequence

1. Split `src/main.rs` into modules without changing behavior.
2. Add tests around current classifier, learning, sanitization, and reports.
3. Add core types: `Signal`, `Profile`, `PolicyAction`, `Receipt`, `Backend`.
4. Add built-in pack schema and hardcoded Phase 1 packs.
5. Add confidence engine and profile decision output.
6. Add receipt store and rollback model.
7. Implement DSCP backend in dry-run first, then apply/remove.
8. Keep routerqosd backend behind the backend trait.
9. Add daemon loop with pause/resume/status.
10. Add Network Lab scoring with mocked tests first.
11. Add policy optimizer validation gate.
12. Add user feedback learning.
13. Add PETSCII CLI formatting.
14. Add WinDivert lab stub, disabled by default.

## Success Criteria

Phase 1 is successful when:

- `winqos-rs run --once --dry-run` explains a chosen profile and actions.
- `winqos-rs status` shows PETSCII status, current profile, confidence, receipts, and pause state.
- `winqos-rs pause` stops automatic mutation.
- DSCP backend can apply and remove at least one safe local policy with receipt.
- Router backend still works through the new backend trait.
- Network Lab can produce before/after reports.
- Policy optimizer can accept a better mocked candidate and reject a worse one.
- User feedback changes future decisions in a visible way.
- All mutation paths have rollback receipts.

## Open Questions For Engineering Review

- Which Windows API should own foreground process detection in Rust?
- Should DSCP policies be managed through PowerShell `New-NetQosPolicy`, Windows APIs, or both?
- Where should installed daemon state live: repo local for dev, `%ProgramData%` for install?
- How strict should the first confidence threshold be before auto-apply?
- Which lab targets are safe defaults without leaking user data?

## Next Review

Run `/plan-eng-review` before implementation. This scope now has enough moving
parts that architecture and error-path review are not optional.
