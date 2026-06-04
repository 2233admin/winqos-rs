# Design System - winqos-rs

## Product Context

- **What this is:** An open-source Windows network optimization engine and traffic-shaping control plane.
- **Who it is for:** Gamers, streamers, Steam users, Tencent/Delta-style shooter players, developers, AI users, and Windows power users who want lower latency and fewer stalls.
- **Space/industry:** Network acceleration, game traffic optimization, livestream stability, download fairness, local QoS, Windows tuning.
- **Project type:** CLI today, local dashboard later.
- **Memorable thing:** A PETSCII overdrive console that squeezes network performance for games, streams, downloads, and work, while still showing receipts.

## Positioning

`winqos-rs` is not mainly a generic "performance control panel". That is too flat.
The sharper promise is:

> Make Windows traffic fight less, so games, streams, AI tools, and downloads can
> run at the same time without the network turning into mud.

The UI should feel like a small network acceleration machine. Users should see:

- game profile active
- Steam download demoted
- Tencent/Delta-style shooter traffic protected
- livestream upload guarded
- Clash/Mihomo proxy traffic understood
- every change visible and reversible

## Aesthetic Direction

- **Direction:** PETSCII Overdrive Console.
- **Decoration level:** Expressive, but system-bound. The style comes from character grids, block graphics, reverse-video strips, score/HUD zones, and queue maps.
- **Mood:** Sharp, retro-futuristic, technical, game-adjacent, and fast. It should feel like booting a custom cartridge that controls your packets.
- **Reference posture:** PETSCII-inspired network HUD, not nostalgic wallpaper.
- **Core rule:** The PETSCII layer must carry information. If a block, hatch, line, or glyph does not explain state, speed, risk, or priority, delete it.

## PETSCII Rules

PETSCII is useful here because it gives the product a native visual grammar:

- grid-first composition
- uppercase command language
- block graphics for meters, maps, queue lanes, and packet flow
- reverse-video states for active/selected/applied
- sparse color with strong meaning
- side HUD panels for inventory-like modules

Use the style as an interface system, not a costume.

Do:

- draw queue lanes as character-grid maps
- show game/stream/download profiles as HUD inventory slots
- use block meters for latency, throughput, and queue pressure
- use all-caps labels for machine state
- keep tables readable with modern spacing underneath the pixel shell

Do not:

- use random retro glyphs as filler
- make body copy hard to read
- turn every panel into a fake game screen
- bury safety, rollback, or backend status under decoration
- use purple gradients, soft SaaS cards, or cleaner-app gloss

## Typography

- **Display/Hero:** Silkscreen - use only for short PETSCII-style titles, HUD headers, score labels, and profile names.
- **Body:** IBM Plex Mono - primary product UI text, readable enough for dense technical surfaces.
- **Docs/Long copy:** Instrument Sans - use in README pages and documentation where paragraphs need air.
- **Data/Tables:** IBM Plex Mono with tabular numbers.
- **Code:** JetBrains Mono.
- **Future self-host option:** Cozette or C64-style bitmap font for dashboard chrome only, never for long body text.
- **Loading:** Public previews can use:
  `https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;500;600&family=Instrument+Sans:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500;600&family=Silkscreen:wght@400;700&display=swap`
- **Scale:**
  - `display`: 44px / 1.0, Silkscreen, uppercase
  - `h1`: 30px / 1.1, IBM Plex Mono 600
  - `h2`: 20px / 1.15, Silkscreen or IBM Plex Mono 600
  - `h3`: 16px / 1.25, IBM Plex Mono 600
  - `body`: 14px / 1.55, IBM Plex Mono
  - `small`: 12px / 1.45
  - `hud`: 11px / 1.2, uppercase, 0.08em letter spacing
  - `mono`: 13px / 1.45

## Color

- **Approach:** Phosphor-first, game-state second. Default to monochrome green-on-black, then add tiny alert colors.
- **CRT Black:** `#020403` main background.
- **Phosphor:** `#A7D36B` primary ink, active state, healthy acceleration.
- **Deep Phosphor:** `#6F9848` secondary glyphs, inactive meters, old-screen shadows.
- **Grid:** `#26351F` grid lines and low-contrast map dots.
- **Panel Black:** `#070B08` panel surface.
- **Reverse Video:** `#A7D36B` background with `#020403` text.
- **Boost Cyan:** `#59C7D6` game/stream protection and measured speed gain.
- **Warning Amber:** `#E6B450` experimental, queue pressure, attention.
- **Drop Red:** `#E05D48` failed backend, packet loss, destructive action.
- **Text Muted:** `#7FA35B`.
- **Light mode:** Not a first-class identity. If required, use a pale terminal print mode for docs only.
- **Semantic:** success `#A7D36B`, info `#59C7D6`, warning `#E6B450`, error `#E05D48`.

## Spacing

- **Base unit:** 4px.
- **Density:** Compact. This is a tactical console for quick decisions.
- **Grid:** 8px visual grid over a 4px spacing system.
- **Scale:** 2xs(2), xs(4), sm(8), md(16), lg(24), xl(32), 2xl(48).

## Layout

- **Approach:** Game HUD plus operator dashboard.
- **Primary dashboard:** Main packet map on the left, speed/weapon-style profile HUD on the right, command log at the bottom.
- **Grid:** 40-column PETSCII-inspired internal rhythm, mapped to responsive CSS grid.
- **Desktop:** 12 columns with fixed HUD rail.
- **Tablet:** 6 columns with HUD below map.
- **Mobile:** stacked panels, still dark, still character-grid.
- **Max content width:** 1440px for app dashboard, 900px for docs.
- **Border radius:** 0px for PETSCII panels, 4px maximum for modern controls. This project should not look bubbly.
- **Panel shape:** Use square frames, double-line borders, reverse-video headers, and dense gutters.

## Components

- **Packet map:** A character-grid panel showing app flows, queue lanes, bottleneck, and backend destination.
- **Boost HUD:** Current mode, gain estimate, protected apps, demoted bulk flows, rollback state.
- **Profile slots:** Game, Stream, Download, Work, AI, Custom. They behave like inventory items, not generic cards.
- **Weapon meter:** Use the "weapon" metaphor for active acceleration profile: game boost, stream guard, bulk sink.
- **Traffic rows:** Process, class, remote, confidence, action, rollback.
- **Receipt log:** Bottom terminal strip with exact command, timestamp, result, and undo path.
- **Risk badge:** SAFE, OPTIONAL, EXPERIMENTAL, BANNED. Use reverse-video labels.
- **Buttons:** Rectangular command blocks. Verbs: BOOST, GUARD, DEMOTE, INSPECT, APPLY, EXPLAIN, ROLLBACK.
- **Charts:** Block meters and stepped lines. Avoid smooth finance-style charts.

## Motion

- **Approach:** Intentional retro-machine motion.
- **Allowed:** cursor blink, scanline pass, meter fill, row flash after apply, packet blip along lane.
- **Avoid:** bouncy easing, decorative looping particles, smooth liquid charts.
- **Easing:** step-like for HUD state, `steps(4, end)` where it fits; otherwise `cubic-bezier(.16,1,.3,1)`.
- **Duration:** micro 80ms, short 180ms, medium 320ms.
- **Reduced motion:** Disable scanline and packet blips, keep instant state changes.

## Safe Choices

- **Dark console baseline:** Network optimization users expect a technical surface and tolerate density.
- **State-driven color:** Games, streams, downloads, and proxies need clear priority status.
- **Monospace-first UI:** Processes, ports, latencies, and queue IDs must align.

## Risks

- **PETSCII identity:** It will repel generic enterprise users, but it gives the project a face and a memory hook.
- **Game metaphor:** "Weapon/profile/HUD" can sound playful, but this product is literally fighting latency and bufferbloat. The metaphor fits if the data stays honest.
- **Mostly dark UI:** Great for gaming and operator feel, weaker for long docs. Solve with separate docs styling, not by watering down the dashboard.

## CSS Tokens

```css
:root {
  --font-display: "Silkscreen", "IBM Plex Mono", monospace;
  --font-ui: "IBM Plex Mono", monospace;
  --font-docs: "Instrument Sans", sans-serif;
  --font-code: "JetBrains Mono", monospace;

  --crt-black: #020403;
  --panel: #070b08;
  --phosphor: #a7d36b;
  --phosphor-dim: #6f9848;
  --grid: #26351f;
  --boost: #59c7d6;
  --warning: #e6b450;
  --drop: #e05d48;
  --reverse-bg: #a7d36b;
  --reverse-ink: #020403;

  --radius-panel: 0;
  --radius-control: 4px;
  --space-1: 4px;
  --space-2: 8px;
  --space-4: 16px;
  --space-6: 24px;
  --space-8: 32px;
}
```

## Decisions Log

| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-06-04 | Initial design system created | Created by design consultation from the winqos-rs README and roadmap. |
| 2026-06-04 | Repositioned from generic performance console to network optimization engine | The user clarified that the core promise is network speed and latency for games, livestreams, downloads, and work. |
| 2026-06-04 | Switched visual identity to PETSCII Overdrive Console | PETSCII gives the project a distinctive game-adjacent interface language with grid, HUD, block meters, and command-state clarity. |
| 2026-06-04 | Kept receipts and rollback as part of the visual language | The product can be aggressive about optimization only if every action stays visible and reversible. |
