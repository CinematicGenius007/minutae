# Minutae redesign: "On air" — implementation scope

Written 2026-11-20. Status: **design approved by owner (2026-11-20).** Nothing below is built yet.

> **Stack changed (2026-11-20):** the owner chose Tauri 2 + Rust for macOS, Windows and Linux. §2, §3, §6 and §7 below were written for SwiftUI and are superseded by [`CROSS-PLATFORM.md`](./CROSS-PLATFORM.md). §1, §4 and §5 (decisions, screen list, behaviour) still apply. Design rules that hold on any stack now live in [`DESIGN-LANGUAGE.md`](./DESIGN-LANGUAGE.md).

Design source of truth: Paper file "Optimistic nebula"
(https://app.paper.design/file/01M7W5R5J6SBFEN8K99EKT2NX0)

| Page | What is on it |
|---|---|
| Minutae — Redesign | 01 Meetings, 02 Live recording, 03 Meeting detail, 04 Dictate, 05 Library, 06a–f Settings (General, Transcription, Summaries, Dictation, Storage, Usage), 07 Mobile (direction only), 00 Components, 08 How it works |
| Minutae — Dark | Dark versions of 00, 01, 02, 03, 06a |
| Inspirations | Reference boards (not part of the spec) |

If approved, this replaces the "Xenial horizon / Ink & signal" design referenced in `docs/TASKS.md` and the DS-03, DS-04 and DS-05 rows move to **approved**.

---

## 1. Decisions this redesign changes

These are locked in `docs/TASKS.md`. The owner has to confirm each change before any work starts.

| Locked today | Proposed | Why |
|---|---|---|
| Light theme only | Light **and** dark, following the system by default, with an override in Settings → General | The owner asked for dark mode |
| 5 sidebar places (Meetings, Library, Dictate, Usage, Settings) | 4 places: Meetings ⌘1, Dictate ⌘2, Library ⌘3, Settings ⌘, | Usage becomes Settings → Usage. There is less to learn |
| Fraunces display serif, pine/mint/butter/lilac/peach palette | Geist (display and UI), Geist Mono (labels, times), Newsreader (summary reading text). Palette: ink, paper and one red | Simpler. Red only ever means "recording" |
| Hold ⌥Space for dictation | **Unchanged.** The design uses ⌥Space | — |

---

## 2. Tokens (MinutaeDesign `Tokens.swift`)

Semantic tokens, each with a light and a dark value:

| Token | Light | Dark | Use |
|---|---|---|---|
| `bg` | `#FFFFFF` | `#0D0D0C` | Window, sidebar |
| `ink` | `#111111` | `#EDEDE8` | Text, primary fills, rules under group labels |
| `muted` | `#6B6B6B` | `#8E8E88` | Secondary text |
| `surface` | `#F4F4F2` | `#181817` | Fields, selected nav, live transcript pane |
| `raised` | `#FFFFFF` | `#2A2A28` | Menus, segmented thumb, keycaps |
| `line` | `#E6E6E3` | `#262624` | Row hairlines |
| `onAir` | `#FF3B1F` | `#FF4A2E` | Recording state only. Never decorative |
| `destructive` | `#C8321F` | `#F0624D` | Text-only destructive actions and errors |
| `ok` | `#2BB673` | `#2BB673` | 6 px status dots only |
| `onInk` | `#FFFFFF` | `#0D0D0C` | Text and icons on an `ink` fill |

How it works in code:

- `MinutaeColorToken` gets a `dark` hex. Each token resolves through `NSColor(name:dynamicProvider:)`, so SwiftUI follows the appearance with no view code.
- Remove `.preferredColorScheme(.light)` from `AppShellView.swift:60`, `SearchSheet.swift:21`, `DesignGalleryView.swift:31` and `DebugHarness.swift:311`.
- New `AppSettings.appearance` (`system | light | dark`), applied through `NSApp.appearance`. It switches live, with no relaunch.
- Pine, mint, butter, lilac and peach stay as deprecated aliases until the last screen moves over, then they are deleted.
- `TokenTests` and `ContrastTests` check both palettes. Body text must reach WCAG AA (4.5:1) in both modes; `muted` on `surface` is the tightest pair.

Radius: 6 for fields, 10 for menus and panels, capsule for actions.
Control heights: 28, 32, 36, 44 and 52.
Spacing: keep the existing `Spacing` scale.

Type:

| Role | Font | Size and weight |
|---|---|---|
| Display | Geist | 56/700, −3.5% tracking |
| Title | Geist | 24/600 |
| Body | Geist | 15/400 |
| Label | Geist Mono | 11–12, caps, +8% tracking |
| Reading | Newsreader | 17–21 |

- Bundle `Newsreader-Variable.ttf` and its OFL licence.
- Remove Fraunces once nothing references it.
- Keep the Dynamic Type mapping in `Typography.swift`.

---

## 3. Components (MinutaeDesign)

| Paper component | Swift today | Change |
|---|---|---|
| Button: primary, secondary, quiet, destructive, disabled | `PrimaryButton`, `SecondaryButton` | One `MButton(_:role:)`. Hover and pressed states |
| Record button: idle and live | `RecordButton` | Restyle. Live state is an `onAir` fill with a timer. Sidebar footer |
| Input: default, focused, error, with help text | none | **New** `MTextField(label:text:help:error:)`. Focus ring is ink at 6% |
| Search | `SearchField` | Restyle and keep |
| Select: closed and open | none | **New** `MSelect`. A SwiftUI `Menu` or `Picker(.menu)` with a custom label, so the open list is the native NSMenu |
| Tabs (underline) | none | **New** `UnderlineTabs`. Keyboard ⌘1–3 inside the detail view |
| Segmented | `SegmentedPills` | Restyle (surface track, raised thumb) |
| Toggle | `PineToggleStyle` | Becomes `InkToggleStyle` |
| Checkbox, radio | none | **New** toggle styles |
| Keycap | `KeyCap` | Restyle. Shortcut recorder state |
| Status dot | `StatusPill` | Replaced by `StatusDot(tone:label:)` |
| Filter chip, tag chip | `Chip` | Restyle; removable tag variant |
| Progress | `ProgressTrack` | Restyle. Optional mono caption |
| Toast | none | **New**. Bottom-centre, 4 s, optional Undo |
| HUD: listening, processing, inserted, copied | `HUDCapsule` | Restyle. The HUD stays dark in both appearances |
| Setting row | `SettingsRow` | Keep the API, restyle |
| List row with fixed lanes | `ListRow` | **New** `LaneRow`: time 64, flexible body, people 180, duration 72, status 132 |
| Group label with a rule | `SectionLabel` | Mono caps with a 1 px ink rule underneath |
| Page title | `PageTitle` (italic emphasis) | Simplified. Mono overline plus 56 pt title |

The gallery (`DesignGalleryView`) gets a light/dark switch and shows every row of the Paper "Components" sheet.

---

## 4. Screens

| Paper | Swift target | Notes |
|---|---|---|
| 01 Meetings | `MeetingsScreen` | Grouped Today / Earlier this week / older. Inline processing progress. Week stats |
| 02 Live recording | new `LiveRecordingScreen` (replaces the unused `MeetingRecordingPill` path) | Timer at 1 Hz with monospaced digits. Waveform from the level meter. Live transcript pane only if MTG-07 is on |
| 03 Meeting detail | `MeetingDetailScreen` | Summary / Transcript / Ask tabs. Action items rail. Moments. Bottom player |
| 04 Dictate | `DictateScreen` | How-to strip, recent dictations, copy |
| 05 Library | `LibraryScreen` (placeholder today) and `SearchSheet` | One field: typing searches, ⏎ asks. Answer plus sources. Needs LIB-01; LIB-07 for "Ask" |
| 06a–f Settings | `SettingsScreen` split into 6 sections | Usage moves here from `UsageScreen` |
| 08 Call prompt | non-activating `NSPanel` | Needs MTG-04 detection |
| 08 Menu bar | SW-07 status item popover | Red dot while recording |
| 08 First run | onboarding | Uses SW-06 permission service |
| 08 Failed / empty | inside the Meetings list and detail | See §5 |
| 07 Mobile | out of scope | Direction only. The app is macOS-only |

---

## 5. How it behaves

**Meeting status becomes an enum.** Today `Meeting.status` is a free string (`Records.swift:11`). The new enum is:

```swift
enum MeetingStatus: String, Codable { case recording, processing, done, failed }
```

- It keeps the same raw values, so no data migration is needed.
- The UI reads the processing sub-step (transcribing or summarising, with a percentage) from the job row, not from the meeting.

**Failures speak human.** Map each `LLMRouter` failure kind to one sentence and one fix:

| Failure | Copy | Fix button |
|---|---|---|
| `notInstalled` | "Codex isn't installed." | Show how |
| `notLoggedIn` | "Codex isn't signed in." | Sign in to Codex |
| `rateLimited` | "Codex is busy. Trying the next provider." | — |
| `timeout` | "The summary took too long." | Retry |
| all transports failed | "No summary provider is ready. Transcript is saved." | Open Settings → Summaries |

Audio and transcripts are never deleted on failure.

**One thing on screen per state:**

| State | What shows |
|---|---|
| Idle | Sidebar dot |
| Call detected | Prompt panel |
| Recording | Red pill and menu-bar dot |
| Processing | Row progress bar |
| Done | One notification |
| Failed | Row reason |

**No Save buttons.** Every control applies immediately and writes through `SettingsStore`.

**Keyboard:**

| Shortcut | Action |
|---|---|
| ⌘1–3, ⌘, | Places |
| ⌘K | Search |
| ⌘R | Record in the app |
| ⌃⌥R | Record from any app |
| ⌘M | Mark moment |
| ⌥Space (hold) | Dictate |
| ⏎ / esc | Accept / dismiss on the prompt |

---

## 6. Performance

The targets from ARCHITECTURE §10 stay. UI-specific budgets:

| What | Budget | How |
|---|---|---|
| Window open (warm) | < 300 ms | Lazy screen construction (already in `ScreenRegistry`) |
| Meetings list, 1,000 rows | Smooth scroll | `LazyVStack`, GRDB `ValueObservation` scoped to the visible range, no per-row DB reads |
| Recording screen idle CPU | < 3% | Timer `TimelineView(.periodic(by: 1))`; waveform in a `Canvas` at ≤ 30 fps from a ring buffer; live transcript appends, never re-lays the whole list |
| ⌘K keyword results | < 100 ms | FTS5 first, semantic results stream in after |
| Appearance switch | Instant, no relaunch | Dynamic `NSColor` tokens |
| HUD show | ≤ 150 ms | Existing target |

---

## 7. Task breakdown

These follow the `docs/TASKS.md` format. Use one PR per ID.

| ID | Task | Size | Depends on |
|---|---|---|---|
| DS-06 | Owner approves the redesign and §1 decisions; update `TASKS.md` locked list and design table | S | — |
| SW-08 | Tokens v2: dynamic light/dark colours, appearance setting, Newsreader, contrast tests for both modes | M | DS-06 |
| SW-09 | Component kit v2: every row in §3, gallery with a light/dark switch | L | SW-08 |
| SW-10 | Shell v2: 4 routes, sidebar record pill, shortcuts; update `NavigationStoreTests`, `ScreenRegistryTests`, `ShellCommands.swift`, `MinutaeApp.swift` | M | SW-09 |
| MTG-08 | `MeetingStatus` enum and failure-reason mapping | S | — |
| UI-07 | Meetings list and meeting detail | L | SW-09, MTG-08 |
| UI-08 | Live recording screen | M | SW-09, MTG-01 |
| UI-09 | Dictate page and HUD restyle | M | SW-09 |
| UI-10 | Library: search and ask | L | SW-09, LIB-01, LIB-07 |
| UI-11 | Settings ×6 (Usage folded in) | L | SW-09, SW-04 |
| UI-12 | Call prompt, menu bar popover, first run, empty and failed states | M | SW-07, MTG-04, SW-06 |
| QA-03 | Snapshot tests per screen in light and dark | M | UI-07…12 |

Suggested order:

1. DS-06 → SW-08 → SW-09 → SW-10. This is the critical path, about 4–5 days.
2. MTG-08 runs in parallel with step 1.
3. UI-07, UI-09 and UI-11 run in parallel; then UI-08, UI-12 and UI-10 as their back-end tasks land.
4. QA-03 last.

Rough total for one agent: 3–4 weeks of work. With the screens in parallel: about 2 weeks.

---

## 8. Open questions for the owner

1. Approve dark mode, which reopens "light theme only"?
2. Approve 4 places, with Usage moving under Settings?
3. Is Newsreader OK for summary text, or keep everything in Geist?
4. Should the "Ask" tab in meeting detail ship before LIB-07, hidden behind a flag?
5. Mobile: the phone frame is direction only. Confirm no iOS target is planned.
