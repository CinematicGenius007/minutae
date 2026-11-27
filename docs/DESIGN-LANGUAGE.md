# Minutae design language ("On air")

Written 2026-11-20. This document holds the rules. The Paper file holds the pictures:
https://app.paper.design/file/01M7W5R5J6SBFEN8K99EKT2NX0 (pages "Minutae — Redesign" and "Minutae — Dark").

Where this document and a Paper artboard disagree, **this document wins**; fix the artboard.

The rules are written for a web-rendered UI (HTML/CSS in a desktop webview), so the same tokens work on macOS, Windows, Linux and a future phone build.

---

## 1. Principles

Every UI decision has to pass all seven.

1. **One thing at a time.** Each screen has one job and at most one primary (ink-filled) button. If two actions compete, one becomes a quiet text link.
2. **Red means live.** `--onair` (and its deeper `--onair-fill` for filled surfaces with text) is used only while audio is being captured: the record pill, the on-air dot, the live waveform tail. Red is never used for decoration, branding or emphasis. Destructive actions use `--destructive` text, never a red fill.
3. **Text before chrome.** Information sits directly on the page, separated by hairlines and space, not boxed in cards. A container gets a background only if it is a control (field, segmented, chip) or floats (menu, toast, prompt).
4. **Quiet until needed.** Secondary information is `--muted`. Hover reveals; it never hides essentials. No badges, counters or dots unless they change what the user does next.
5. **Every state explains itself.** Loading, empty, processing, failed and offline each have designed copy. An error names the cause in plain words and offers one fix.
6. **Keyboard first.** Every action reachable by mouse is reachable by keyboard, and the shortcut is printed next to it in mono.
7. **Local is visible.** When data stays on the device, say so in the UI ("on this Mac", "saved locally"). Trust is a feature.

---

## 2. Colour

### Tokens

| Token | Light | Dark | Role |
|---|---|---|---|
| `--bg` | `#FFFFFF` | `#0D0D0C` | Window and sidebar ground |
| `--ink` | `#111111` | `#EDEDE8` | Primary text, primary fills, group rules, icons |
| `--muted` | `#6B6B6B` | `#9A9A94` | Secondary text, metadata, inactive nav, placeholders, live partial captions |
| `--faint` | `#B5B5B0` | `#4A4A47` | Disabled controls and disabled text (except tagged planned options), purely decorative elements (unplayed waveform bars, off dots) — never readable non-disabled text |
| `--surface` | `#F4F4F2` | `#181817` | Fields, selected nav item, side panes |
| `--raised` | `#FFFFFF` | `#2A2A28` | Menus, segmented thumb, keycaps, toggle knob track contrast |
| `--line` | `#E6E6E3` | `#262624` | Row hairlines, secondary button, chip and player speed borders |
| `--line-strong` | `#D9D9D5` | `#3A3A38` | Keycap borders, scrollbar thumbs, subtle dividers, dark elevation hairlines |
| `--control-edge` | `#8A8A85` | `#6E6E69` | 3:1 boundaries for controls perceived solely by shape (off toggle track, unchecked checkbox/radio outlines, player scrubber track) and the dashed pill border on planned options — never a text colour |
| `--on-ink` | `#FFFFFF` | `#0D0D0C` | Text and icons on an `--ink` fill |
| `--onair` | `#FF3B1F` | `#FF4A2E` | Recording state only |
| `--onair-fill` | `#DB2F17` | `#DB2F17` | Filled recording surfaces that carry text or icons (live record pill, record button). White on it is 4.74:1 |
| `--on-onair` | `#FFFFFF` | `#FFFFFF` | Text on `--onair-fill` |
| `--destructive` | `#C8321F` | `#F0624D` | Destructive text links, field errors |
| `--ok` | `#2BB673` | `#2BB673` | 6 px status dots and check glyphs only |
| `--hud` | `#111111` | `#262624` | Dictation HUD and toasts. They stay dark in both modes |

### Rules

- **The ratio is 90 / 9 / 1.** About 90% `--bg` and `--surface`, 9% `--ink` and `--muted` text, at most 1% `--onair` (and only while recording).
- **No other colours.** No gradients and no tints of the accent. A screen that needs another colour needs a different design.
- **Dark mode is not inverted light mode.** Both palettes are tuned by hand. Always use tokens, never raw hex values in components.
- **Contrast.** Body text must reach WCAG AA (4.5:1) on its background in both modes; controls whose shape is the sole way to perceive them or their state (off toggle track, unchecked checkbox/radio outlines, player scrubber track) and focus indicators must reach 3:1 (`--control-edge` / `--ink`). Controls identified by their text or fill (secondary buttons, chips, segmented tracks, selects, fields, keycaps, player speed button) keep their resting `--line`, `--line-strong`, or borderless `--surface` look and gain clearly visible edges under `prefers-contrast: more` and `forced-colors: active`. Mono labels at 11 px also need 4.5:1, which is why `--muted` is not lighter. Disabled controls and disabled text use `--faint` (never `--control-edge`), with tagged planned options (such as Cloud `PLANNED`) as the one readable exception (`--muted` text with a 1 px dashed `--control-edge` mono caps pill). Under `prefers-contrast: more`, hairlines (`--line`, `--line-strong`), control edges (`--control-edge`) and `--muted` are strengthened via tokens. Under `forced-colors: active` (Windows Contrast Themes), focus rings survive via a 2 px transparent outline and selected/checked/on states use system `Highlight`/`HighlightText`.
- **Status = dot + word.** Never colour alone (●Ready, ●On air, ●Off).

---

## 3. Typography

All fonts are bundled with the app (Geist, Geist Mono, Newsreader; OFL licences). Nothing depends on system fonts, so every OS renders the same.

| Style | Family | Size / line height | Weight | Tracking | Used for |
|---|---|---|---|---|---|
| `display` | Geist | 56 / 56 | 700 | −0.035em | One page title per screen |
| `display-sm` | Geist | 40 / 42 | 700 | −0.035em | Onboarding, mobile titles |
| `numeral` | Geist | 168 / 150 | 300 | −0.05em | Live timer only |
| `stat` | Geist | 28 / 32 | 600 | −0.02em | Header stats, usage figures |
| `title` | Geist | 24 / 30 | 600 | −0.02em | Recording title, section heroes |
| `heading` | Geist | 17 / 22 | 600 | −0.01em | Row titles, card titles |
| `body` | Geist | 15 / 22 | 400 | 0 | Default UI text |
| `body-sm` | Geist | 13–14 / 19–20 | 400 | 0 | Descriptions, help text, metadata |
| `label` | Geist Mono | 12 / 16 | 500 | +0.08em, UPPERCASE | Group labels, overlines |
| `meta` | Geist Mono | 11–13 / 14–18 | 400 | 0 | Times, durations, sizes, shortcuts |
| `reading` | Newsreader | 17 / 26 (lead 21 / 31) | 400 | 0 | Summaries, decisions, AI answers |

Rules:

- **Contrast comes from size and weight, not colour.** Pair a heavy title with light metadata. At most 2 weights in any one region.
- **Mono is for machine facts:** times, durations, sizes, paths, shortcuts and overline labels. Never use it for sentences.
- **Serif is for reading:** anything the AI wrote for you to read. Never use it for UI controls.
- **Numbers that change use tabular figures** (`font-variant-numeric: tabular-nums`): timers, durations, percentages.
- **Measure:** reading text at most 640 px wide (about 70 characters).
- **Minimum size** is 11 px (mono caps only); otherwise 13 px.
- **Sentence case everywhere.** Only `label` is uppercase, through CSS, never typed in caps.

---

## 4. Spacing and layout

**Base unit: 4 px.** Scale: `2 · 4 · 8 · 12 · 16 · 20 · 24 · 28 · 36 · 44 · 56 · 72`.

| Relationship | Space |
|---|---|
| Icon to its label | 8 |
| Inside a control (vertical / horizontal) | 8–10 / 12–16 |
| Title to its description | 2–4 |
| Rows in a list | 0 gap, 16–18 vertical padding, 1 px `--line` between |
| Group label to first row | 10 (then the ink rule) |
| Group to group | 36–44 |
| Page header to content | 40–48 |
| Page padding (desktop) | 72 left and right, 28 top (toolbar) |
| Sidebar padding | 28 top, 20 sides, 24 bottom |

**Tighter groups, generous separations.** If two things belong together, halve the space. If they don't, double it.

### Layout regions (desktop)

- **Sidebar:** 240 px fixed, hairline right border. Wordmark, 4 places, and at the bottom the mic status and the Record pill.
- **Main:** fluid. Content max width 1200 px. Lists span the full width; reading columns are at most 640 px.
- **Right rail** (detail views): 320 px, 64 px gap from the reading column.
- **Bottom bar** (player): 76 px, hairline top border.

### Lanes

Repeated rows use fixed-width columns, so values line up vertically across rows. Only the main text column flexes.

| Lane | Width |
|---|---|
| Time | 64 |
| People | 180 |
| Duration | 72, right-aligned |
| Status | 132, right-aligned |
| Actions | 56 |

### Responsive

| Width | Layout |
|---|---|
| ≥ 1200 | Full layout as drawn |
| 900–1199 | Sidebar 200 px; right rail drops below the reading column; page padding 40 |
| < 900 (narrow or phone) | No sidebar; bottom tab bar with the Record button in the centre; page padding 20; `display` drops to 40 |

Minimum window size: 960 × 640.

---

## 5. Shape, borders, elevation

| Token | Value | Used for |
|---|---|---|
| `--radius-sm` | 6 px | Fields, selects, segmented track, keycaps, nav items |
| `--radius-md` | 10 px | Menus, panels, toasts, how-to strips |
| `--radius-full` | 999 px | Buttons, chips, toggles, record pill, HUD |

- **Borders:** 1 px `--line` for rows and secondary buttons. 1 px `--ink` only under group labels and tab underlines. 1.5 px `--ink` for a focused field.
- **Elevation:** only floating things cast a shadow.
  - `--shadow-1` (menus, segmented thumb): `0 1px 2px rgb(0 0 0 / .08)`
  - `--shadow-2` (popovers, prompt, toast): `0 12px 32px rgb(0 0 0 / .10), 0 2px 6px rgb(0 0 0 / .06)`
  - Dark mode uses the same shadows plus a `0 0 0 0.5px --line-strong` outline.

---

## 6. Icons

- **Library:** Lucide (open source, cross-platform). Stroke 2, round caps and joins.
- **Sizes:** 14 (inline with 13–14 px text), 16 (buttons), 20 (toolbar and tab bar).
- **Colour:** `currentColor`. Icons inherit the text colour; they never carry meaning by colour.
- **Use sparingly.** Use an icon only when the word alone is slower to scan (search, play, pause, back, close). Nav items are text-only on desktop.
- **The record glyph is always a filled circle;** stop is always a rounded square. Never swap them.

---

## 7. Components

**Every interactive component has six states:** default, hover, pressed, focus-visible, disabled, and loading where it applies.

**Focus ring:** `0 0 0 2px --bg, 0 0 0 4px --ink` (offset ring) paired with `outline: 2px solid transparent; outline-offset: 2px` so the ring survives `forced-colors: active`. Never remove an outline without replacing it.

| Component | Height | Spec | Notes |
|---|---|---|---|
| Button / primary | 36 (52 hero) | `--ink` fill, `--on-ink` text 14/500, pill | One per view. Hover lightens 15% |
| Button / secondary | 36 | 1 px `--line` border, `--ink` text | |
| Button / quiet | — | Underlined text | "Change", "Retry", "Show in Finder" |
| Button / destructive | — | Underlined `--destructive` text plus a confirm dialog | Never filled |
| Record pill | 48 | Idle: `--ink` fill with an `--onair` dot. Live: `--onair` fill, stop square, mono timer | Always bottom of the sidebar |
| Text field | 40 | `--surface` fill, radius 6, 15 px. Focus: `--raised` fill, 1.5 px `--ink` border plus a 4 px 6%-ink halo | Label above (13/500), help or error below (13) |
| Search | 36 | `--surface`, leading icon, trailing shortcut in mono | ⌘K / Ctrl+K anywhere |
| Select | 32 | `--surface`, chevron 10 px; the menu is `--raised`, radius 10, `--shadow-2`, rows 32, checkmark lane 14 | Keyboard: ↑↓ ⏎ esc, type-ahead |
| Segmented | 34 | `--surface` track, 3 px padding; `--raised` thumb with `--shadow-1` | 2–4 options. Planned disabled option: `--muted` label + 1 px dashed `--control-edge` mono caps pill |
| Tabs | 32 | Text 15; active is 600 weight with a 2 px `--ink` underline; the row has a 1 px `--line` base | Switch views, never values |
| Toggle | 24 × 40 | On: `--ink` track. Off: `--control-edge` | Applies instantly. No Save buttons |
| Checkbox / radio | 18 | 1.5 px `--control-edge` outline; checked is an `--ink` fill/ring with an `--on-ink` check | |
| Chip / filter | 30 | Selected: `--ink` fill. Unselected: 1 px `--line` | Tags: `--surface` fill with a × (28 × 28 hit target) |
| Keycap | 24–32 | `--raised`, 1 px `--line-strong` plus a 1.5 px bottom shadow, mono | |
| Status dot | 6 | Colour plus word | |
| Progress | 3 | `--line` track, `--ink` fill, mono caption | Never a spinner if progress is known |
| Toast | 44 | `--hud`, white text, optional Undo, bottom-centre, 4 s | One at a time |
| HUD | 40 | `--hud` pill: listening (red dot + bars), processing, inserted (✓ + count), copied | Never takes focus |
| Group label | — | `label` style; 10 px below, 1 px `--ink` rule; optional mono count on the right | |
| Setting row | ≥ 56 | Title 15/500, description 13 `--muted`, control right-aligned | |

---

## 8. Motion

| Token | Duration | Easing | Used for |
|---|---|---|---|
| `--t-fast` | 120 ms | ease-out | Hover, press, toggle |
| `--t-base` | 200 ms | cubic-bezier(.2,.8,.2,1) | Menus, tabs, segmented thumb |
| `--t-slow` | 320 ms | cubic-bezier(.2,.8,.2,1) | Panels, prompt, page transitions |

- Motion explains a change of state. It is never decoration.
- The only looping animation is the on-air dot pulse (1.6 s, opacity 1 → .4) and the live waveform.
- Honour `prefers-reduced-motion`: drop the transforms, keep the opacity fades, and stop the pulse.

---

## 9. Words

- **Plain, short, specific.** "Transcribing on this Mac · 62%", not "Processing your content…".
- **Buttons are verbs:** Record, Copy summary, Add key. Never use "OK", "Submit" or "Click here".
- **Errors are cause plus fix:** "Codex isn't signed in. [Sign in to Codex]". No "Oops", no error codes in the UI (codes go to logs).
- **Empty states teach one thing:** "No meetings yet. Join a call and Minutae will ask, or press ⌘R."
- **Times:**
  - `14:30` today; `Wed` within the week; `12 Nov` otherwise.
  - Durations `32:14`.
  - Sizes `466 MB`.
  - Use 24-hour or 12-hour from the OS locale.
- **Names:** Meetings, Dictate, Library, Settings. Never "Recordings", "Notes" or "Dashboard".

---

## 10. Platforms

One design, small native adjustments:

| | macOS | Windows | Linux |
|---|---|---|---|
| Shortcut glyphs | ⌘ ⌥ ⌃ ⇧ | Ctrl Alt Shift (text) | Ctrl Alt Shift (text) |
| Window chrome | Transparent title bar, traffic lights over the sidebar (28 px top inset) | Custom caption buttons top-right, 32 px drag strip | Native decorations by default |
| Always-there surface | Menu bar item | System tray | Tray (AppIndicator) where available |
| Scrollbars | Overlay | Thin overlay, styled | Thin overlay, styled |
| Copy | "on this Mac" | "on this PC" | "on this computer" |

Shortcut mapping: ⌘ → Ctrl, ⌥ → Alt everywhere except the dictation hold key. The default dictation key is **⌥ Space** on macOS and **Ctrl + Space** (configurable) on Windows and Linux.

---

## 11. Accessibility checklist

- Keyboard reaches every control, in a logical tab order; esc closes the topmost layer.
- Focus is always visible (§7 ring).
- Hit targets are at least 28 × 28 on desktop and 44 × 44 at narrow widths.
- Recording state is announced to screen readers ("Recording started", timer as a live region every 60 s, not every second).
- Contrast meets AA in both modes. Automated check in CI.
- Reduced motion is respected.

---

## 12. Definition of done for any screen

1. Matches its Paper artboard within 2 px in spacing and exact in tokens, in **light and dark**.
2. Every state is designed and built: empty, loading, processing, error, offline.
3. Keyboard path works and shortcuts are shown.
4. A screenshot test per state per theme passes (see the testing plan).
5. No raw hex values, pixel font sizes or ad-hoc spacing in component code. Tokens only.

---

## 13. Canonical tokens (CSS)

```css
:root {
  /* colour, light */
  --bg: #FFFFFF; --ink: #111111; --muted: #6B6B6B; --faint: #B5B5B0;
  --surface: #F4F4F2; --raised: #FFFFFF; --line: #E6E6E3; --line-strong: #D9D9D5;
  --control-edge: #8A8A85;
  --on-ink: #FFFFFF; --onair: #FF3B1F; --onair-fill: #DB2F17; --on-onair: #FFFFFF;
  --destructive: #C8321F; --ok: #2BB673; --hud: #111111;

  /* type */
  --font-sans: "Geist", system-ui, sans-serif;
  --font-mono: "Geist Mono", ui-monospace, monospace;
  --font-serif: "Newsreader", Georgia, serif;

  /* space (4 px base) */
  --s-1: 2px; --s-2: 4px; --s-3: 8px; --s-4: 12px; --s-5: 16px; --s-6: 20px;
  --s-7: 24px; --s-8: 28px; --s-9: 36px; --s-10: 44px; --s-11: 56px; --s-12: 72px;

  /* shape */
  --radius-sm: 6px; --radius-md: 10px; --radius-full: 999px;
  --shadow-1: 0 1px 2px rgb(0 0 0 / .08);
  --shadow-2: 0 12px 32px rgb(0 0 0 / .10), 0 2px 6px rgb(0 0 0 / .06);

  /* motion */
  --t-fast: 120ms ease-out;
  --t-base: 200ms cubic-bezier(.2,.8,.2,1);
  --t-slow: 320ms cubic-bezier(.2,.8,.2,1);

  /* layout */
  --sidebar-w: 240px; --rail-w: 320px; --reading-max: 640px; --content-max: 1200px;
}

:root[data-theme="dark"] {
  --bg: #0D0D0C; --ink: #EDEDE8; --muted: #9A9A94; --faint: #4A4A47;
  --surface: #181817; --raised: #2A2A28; --line: #262624; --line-strong: #3A3A38;
  --control-edge: #6E6E69;
  --on-ink: #0D0D0C; --onair: #FF4A2E; --onair-fill: #DB2F17; --destructive: #F0624D; --hud: #262624;
}

@media (prefers-color-scheme: dark) {
  :root:not([data-theme="light"]) {
    --bg: #0D0D0C; --ink: #EDEDE8; --muted: #9A9A94; --faint: #4A4A47;
    --surface: #181817; --raised: #2A2A28; --line: #262624; --line-strong: #3A3A38;
    --control-edge: #6E6E69;
    --on-ink: #0D0D0C; --onair: #FF4A2E; --onair-fill: #DB2F17; --destructive: #F0624D; --hud: #262624;
  }
}

@media (prefers-contrast: more) {
  :root {
    --muted: #4A4A47; --line: #8A8A85; --line-strong: #6B6B6B;
    --control-edge: #595956; --destructive: #A62212;
  }
  :root[data-theme="dark"] {
    --muted: #BDBDB7; --line: #6E6E69; --line-strong: #8E8E88;
    --control-edge: #9A9A94; --destructive: #FF7A66;
  }
  @media (prefers-color-scheme: dark) {
    :root:not([data-theme="light"]) {
      --muted: #BDBDB7; --line: #6E6E69; --line-strong: #8E8E88;
      --control-edge: #9A9A94; --destructive: #FF7A66;
    }
  }
}
```
