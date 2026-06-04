# Design System - winqos-rs

## Product Context

- **What this is:** An open-source Windows QoS learner and PC optimization suite core.
- **Who it is for:** Windows power users, network operators, gamers, developers, and builders who want measurable latency and traffic control.
- **Space/industry:** Local performance tooling, traffic shaping, network observability, Windows optimization.
- **Project type:** CLI today, local dashboard later.
- **Memorable thing:** This is not a one-click myth optimizer. It is an explainable, reversible Windows performance control console.

## Aesthetic Direction

- **Direction:** Industrial telemetry console.
- **Decoration level:** Intentional. Use thin grid lines, receipt strips, status marks, and queue bands. No decorative blobs, no gradients as identity.
- **Mood:** Serious, technical, local-first, and inspectable. The interface should feel like it belongs next to packet counters and shell output.
- **Reference posture:** Less consumer cleaner app, more network control room.

## Typography

- **Display/Hero:** Archivo - compact, mechanical, and strong enough for command surfaces.
- **Body:** Instrument Sans - readable for docs and settings without feeling generic.
- **UI/Labels:** Instrument Sans Medium with small uppercase labels for machine state.
- **Data/Tables:** IBM Plex Mono - clear tabular data, commands, counters, ports, and receipts.
- **Code:** JetBrains Mono.
- **Loading:** Use Google Fonts for public web previews:
  `https://fonts.googleapis.com/css2?family=Archivo:wght@500;600;700&family=IBM+Plex+Mono:wght@400;500;600&family=Instrument+Sans:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500;600&display=swap`
- **Scale:**
  - `display`: 40px / 1.0
  - `h1`: 32px / 1.1
  - `h2`: 24px / 1.16
  - `h3`: 18px / 1.25
  - `body`: 15px / 1.55
  - `small`: 13px / 1.45
  - `label`: 11px / 1.2, uppercase, 0.08em letter spacing
  - `mono`: 13px / 1.45

## Color

- **Approach:** Restrained, functional, and state-driven. Color means network state, not decoration.
- **Primary:** `#1FA463` circuit green. Use for healthy, applied, optimized, active.
- **Secondary:** `#168AAD` meter cyan. Use for measurement, samples, graph focus, neutral telemetry.
- **Accent:** `#E0A11B` warning amber. Use for optional, experimental, and attention states.
- **Danger:** `#D84C3F` hard red. Use only for destructive, banned, failed, or rollback-needed states.
- **Background:** `#F6F8F5` lab white.
- **Surface:** `#FFFFFF` primary panel.
- **Surface 2:** `#EEF2ED` secondary panel and table header.
- **Ink:** `#151A17` primary text.
- **Muted:** `#626D66` secondary text.
- **Line:** `#CBD4CD` borders and grid lines.
- **Dark mode background:** `#0F1311`.
- **Dark mode surface:** `#171D19`.
- **Dark mode surface 2:** `#202820`.
- **Dark mode ink:** `#E7EEE9`.
- **Dark mode muted:** `#98A69D`.
- **Dark mode line:** `#313B34`.
- **Semantic:** success `#1FA463`, warning `#E0A11B`, error `#D84C3F`, info `#168AAD`.
- **Dark mode strategy:** Redesign surfaces, not just invert. Keep saturation 10-15% lower except for live status dots.

## Spacing

- **Base unit:** 4px.
- **Density:** Compact. This product is for repeated scanning, not hero-page lounging.
- **Scale:** 2xs(2), xs(4), sm(8), md(16), lg(24), xl(32), 2xl(48), 3xl(64).

## Layout

- **Approach:** Grid-disciplined for app surfaces, editorial only for docs and release pages.
- **Primary dashboard:** Left module rail, top command/status bar, dense main grid, right receipt panel.
- **Grid:** 12 columns desktop, 6 columns tablet, 1 column mobile.
- **Max content width:** 1440px for app dashboard, 860px for docs.
- **Border radius:** sm 3px, md 6px, lg 8px, full 9999px. Never use large bubbly cards.
- **Cards:** Use only for repeated modules and status units. Do not nest cards inside cards.
- **Tables:** Sticky header, tabular numbers, compact rows, visible active row state.

## Components

- **Module tile:** State badge, last action, rollback availability, risk tier, primary command.
- **Receipt panel:** Append-only action log with timestamp, command, result, and rollback link.
- **Traffic class row:** Class name, process count, candidates, current queue, packet hint, confidence.
- **Risk tier badge:** Safe green, optional cyan, experimental amber, banned red.
- **Command buttons:** Text plus icon when command intent matters. Use direct verbs: Inspect, Apply, Remove, Explain.
- **Charts:** Thin strokes, no filled gradient areas. Show queue counters and latency samples with labeled thresholds.

## Motion

- **Approach:** Minimal-functional.
- **Easing:** enter `cubic-bezier(.16,1,.3,1)`, exit `cubic-bezier(.7,0,.84,0)`, move `cubic-bezier(.45,0,.2,1)`.
- **Duration:** micro 70ms, short 160ms, medium 260ms.
- **Rules:** Animate state changes and panel entry only. Do not animate charts for decoration after first render.

## Safe Choices

- **Dense dashboard layout:** Users need to scan counters, modules, and receipts quickly.
- **Monospace data lane:** Ports, PIDs, queue IDs, commands, and timestamps must align.
- **State-driven color:** QoS tools are control surfaces. Color must mean something.

## Risks

- **No soft SaaS polish:** The UI may feel harder than mainstream tools, but it will earn trust with technical users.
- **Compact density:** It sacrifices marketing friendliness for operator speed.
- **Amber experimental lane:** Experimental features are visible instead of hidden. This creates tension, but it stops the project from lying about risk.

## CSS Tokens

```css
:root {
  --font-display: "Archivo", sans-serif;
  --font-body: "Instrument Sans", sans-serif;
  --font-mono: "IBM Plex Mono", "JetBrains Mono", monospace;

  --bg: #f6f8f5;
  --surface: #ffffff;
  --surface-2: #eef2ed;
  --ink: #151a17;
  --muted: #626d66;
  --line: #cbd4cd;
  --primary: #1fa463;
  --secondary: #168aad;
  --warning: #e0a11b;
  --danger: #d84c3f;

  --radius-sm: 3px;
  --radius-md: 6px;
  --radius-lg: 8px;
  --space-1: 4px;
  --space-2: 8px;
  --space-4: 16px;
  --space-6: 24px;
  --space-8: 32px;
}

[data-theme="dark"] {
  --bg: #0f1311;
  --surface: #171d19;
  --surface-2: #202820;
  --ink: #e7eee9;
  --muted: #98a69d;
  --line: #313b34;
}
```

## Decisions Log

| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-06-04 | Initial design system created | Created by design consultation from the winqos-rs README and roadmap. |
| 2026-06-04 | Chose industrial telemetry console | The product must feel measurable, local, reversible, and serious. |
| 2026-06-04 | Rejected SaaS gradients and soft cleaner-app styling | The project should not look like fake one-click PC optimizer software. |
