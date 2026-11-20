import { Download } from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";
import { Button } from "../components/Button";
import { Checkbox } from "../components/Checkbox";
import { FilterChip, Pill, Tag } from "../components/Chip";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { DropdownMenu } from "../components/DropdownMenu";
import { EmptyState } from "../components/EmptyState";
import { GroupLabel } from "../components/GroupLabel";
import { HudPill } from "../components/HudPill";
import { KeyCap, Shortcut } from "../components/KeyCap";
import { LaneRow } from "../components/LaneRow";
import { PageHeader } from "../components/PageHeader";
import { PlayerBar } from "../components/PlayerBar";
import { Progress } from "../components/Progress";
import { RadioGroup, RadioItem } from "../components/Radio";
import { RecordPill } from "../components/RecordPill";
import { SearchField } from "../components/SearchField";
import { Segmented } from "../components/Segmented";
import { MenuPreview, Select, type SelectEntry } from "../components/Select";
import { SettingRow } from "../components/SettingRow";
import { MicStatus, NavItem, Sidebar } from "../components/Sidebar";
import { Stat } from "../components/Stat";
import { StatusDot } from "../components/StatusDot";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "../components/Tabs";
import { TextField } from "../components/TextField";
import { ToastProvider, ToastSurface, useToast } from "../components/Toast";
import { Toggle } from "../components/Toggle";
import { Tooltip, TooltipProvider } from "../components/Tooltip";
import { Waveform } from "../components/Waveform";
import { cx } from "../lib/cx";
import { detectPlatform, localPhrase, type Platform } from "../lib/platform";
import { type ThemePreference, ThemeProvider, useOptionalTheme, useTheme } from "../theme/ThemeProvider";
import g from "./Gallery.module.css";

const MICS: SelectEntry[] = [
  { value: "builtin", label: "MacBook Pro Microphone" },
  { value: "airpods", label: "AirPods Pro" },
  { value: "shure", label: "Shure MV7" },
  "separator",
  { value: "system", label: "Follow system default", muted: true },
];

function seeded(n: number, seed = 7): number[] {
  let x = seed;
  return Array.from({ length: n }, (_, i) => {
    x = (x * 9301 + 49297) % 233280;
    const r = x / 233280;
    const envelope = 0.35 + 0.65 * Math.abs(Math.sin(i / 9));
    return Math.max(0.08, Math.min(1, r * envelope + 0.05));
  });
}
const WAVE = seeded(120);

function Section({ title, note, children }: { title: string; note?: string; children: ReactNode }) {
  return (
    <section className={g.section} aria-label={title}>
      <div className={g.label}>
        <h2 className={g.labelTitle}>{title}</h2>
        {note ? <p className={g.labelNote}>{note}</p> : null}
      </div>
      <div className={g.body}>{children}</div>
    </section>
  );
}

function Spec({ caption, children, className }: { caption: string; children: ReactNode; className?: string }) {
  return (
    <div className={cx(g.spec, className)}>
      {children}
      <div className={g.caption}>{caption}</div>
    </div>
  );
}

function useLiveLevels(count: number, active: boolean): number[] {
  const [levels, setLevels] = useState<number[]>(() => seeded(count, 3));
  useEffect(() => {
    if (!active) return;
    const id = window.setInterval(() => {
      setLevels((prev) => [...prev.slice(1), 0.15 + Math.random() * 0.85]);
    }, 120);
    return () => window.clearInterval(id);
  }, [active]);
  return levels;
}

function GalleryBody() {
  const { preference, setPreference } = useTheme();
  const [platform, setPlatform] = useState<Platform>(() => detectPlatform());
  const toast = useToast();

  const [live, setLive] = useState(false);
  const [elapsed, setElapsed] = useState(12 * 60_000 + 48_000);
  const [mic, setMic] = useState("builtin");
  const [where, setWhere] = useState<"local" | "cloud">("local");
  const [filter, setFilter] = useState("all");
  const [tags, setTags] = useState(["design", "q2", "launch"]);
  const [toggles, setToggles] = useState({ a: true, b: false });
  const [checks, setChecks] = useState({ a: false, b: true });
  const [radio, setRadio] = useState("small");
  const [nav, setNav] = useState("meetings");
  const [playing, setPlaying] = useState(false);
  const [pos, setPos] = useState(4 * 60_000 + 12_000);
  const [speed, setSpeed] = useState(1);
  const [loading, setLoading] = useState(false);
  const liveLevels = useLiveLevels(6, true);
  const liveWave = useLiveLevels(64, true);

  useEffect(() => {
    if (!live) return;
    const id = window.setInterval(() => setElapsed((e) => e + 1000), 1000);
    return () => window.clearInterval(id);
  }, [live]);

  useEffect(() => {
    if (!playing) return;
    const id = window.setInterval(() => setPos((p) => Math.min(p + 1000 * speed, 32 * 60_000 + 14_000)), 1000);
    return () => window.clearInterval(id);
  }, [playing, speed]);

  const here = localPhrase(platform);

  return (
    <div className={g.page}>
      <div className={g.inner}>
        <header className={g.head}>
          <div className={g.headText}>
            <div className={g.overline}>Minutae design · v4</div>
            <h1 className={g.title}>Components</h1>
          </div>
          <div className={g.headRight}>
            <div className={g.switches}>
              <Segmented<ThemePreference>
                aria-label="Theme"
                value={preference}
                onValueChange={setPreference}
                options={[
                  { value: "system", label: "System" },
                  { value: "light", label: "Light" },
                  { value: "dark", label: "Dark" },
                ]}
              />
              <Segmented<Platform>
                aria-label="Shortcut platform"
                value={platform}
                onValueChange={setPlatform}
                options={[
                  { value: "mac", label: "macOS" },
                  { value: "windows", label: "Windows" },
                  { value: "linux", label: "Linux" },
                ]}
              />
            </div>
            <p className={g.blurb}>
              Five heights: 28 · 32 · 36 · 44 · 52. One radius family: 6 for fields, pill for actions. Ink fills mean
              "do it"; red means "live".
            </p>
          </div>
        </header>

        <Section title="Buttons" note="One primary per view.">
          <div className={g.row}>
            <Spec caption="Primary">
              <Button onClick={() => toast.show({ title: "Summary copied", actionLabel: "Undo" })}>Copy summary</Button>
            </Spec>
            <Spec caption="Primary · hover">
              <Button data-force="hover">Copy summary</Button>
            </Spec>
            <Spec caption="Primary · pressed">
              <Button data-force="pressed">Copy summary</Button>
            </Spec>
            <Spec caption="Primary · focus">
              <Button data-force="focus">Copy summary</Button>
            </Spec>
            <Spec caption="Loading">
              <Button
                loading={loading}
                onClick={() => {
                  setLoading(true);
                  window.setTimeout(() => setLoading(false), 1600);
                }}
              >
                Summarize
              </Button>
            </Spec>
          </div>
          <div className={g.row}>
            <Spec caption="Secondary">
              <Button variant="secondary">Export…</Button>
            </Spec>
            <Spec caption="Secondary · hover">
              <Button variant="secondary" data-force="hover">
                Export…
              </Button>
            </Spec>
            <Spec caption="With icon">
              <Button variant="secondary" icon={<Download size={16} />}>
                Download model
              </Button>
            </Spec>
            <Spec caption="Quiet">
              <Button variant="quiet">Change</Button>
            </Spec>
            <Spec caption="Quiet · hover">
              <Button variant="quiet" data-force="hover">
                Show in folder
              </Button>
            </Spec>
            <Spec caption="Disabled">
              <Button disabled>Summarize</Button>
            </Spec>
            <Spec caption="Destructive">
              <ConfirmDialog
                trigger={<Button variant="destructive">Delete meeting…</Button>}
                title="Delete “Q2 Design Sync”?"
                description={`The recording, transcript and summary are removed ${here}. This can't be undone.`}
                confirmLabel="Delete meeting"
                onConfirm={() => toast.show({ title: "Meeting deleted" })}
              />
            </Spec>
          </div>
          <div className={g.row}>
            <Spec caption="Hero · 52">
              <Button size="hero" shortcut="mod+enter" platform={platform}>
                Start recording
              </Button>
            </Spec>
            <Spec caption="Hero · secondary">
              <Button size="hero" variant="secondary">
                Not now
              </Button>
            </Spec>
            <Spec caption="Menu">
              <DropdownMenu
                platform={platform}
                trigger={<Button variant="secondary">More…</Button>}
                items={[
                  { label: "Copy summary", shortcut: "mod+shift+c" },
                  { label: "Export as Markdown", shortcut: "mod+e" },
                  { label: "Show in folder" },
                  "separator",
                  { label: "Delete meeting…", destructive: true },
                ]}
              />
            </Spec>
          </div>
        </Section>

        <Section title="Record" note="Always at the bottom of the sidebar. Red only while live.">
          <div className={g.row}>
            <Spec caption="Record · idle">
              <RecordPill platform={platform} onClick={() => setLive(true)} />
            </Spec>
            <Spec caption="Idle · hover">
              <RecordPill platform={platform} data-force="hover" />
            </Spec>
            <Spec caption="Idle · focus">
              <RecordPill platform={platform} data-force="focus" />
            </Spec>
            <Spec caption="Record · live">
              <RecordPill live elapsedMs={elapsed} onClick={() => setLive(false)} />
            </Spec>
            <Spec caption="Live · hover">
              <RecordPill live elapsedMs={elapsed} data-force="hover" />
            </Spec>
            <Spec caption="Disabled">
              <RecordPill platform={platform} disabled />
            </Spec>
          </div>
        </Section>

        <Section title="Input" note="Label above, help below. Errors say what to do.">
          <div className={g.grid4}>
            <Spec caption="Default">
              <TextField label="Meeting title" placeholder="Untitled meeting" />
            </Spec>
            <Spec caption="Focused">
              <TextField label="Meeting title" defaultValue="Q2 Design Sync" forceState="focus" />
            </Spec>
            <Spec caption="Error">
              <TextField
                label="Grok API key"
                mono
                defaultValue="xai-•••••••"
                error="That key was rejected. Check it at console.x.ai."
              />
            </Spec>
            <Spec caption="Search">
              <div className={g.stack}>
                <SearchField platform={platform} />
              </div>
            </Spec>
          </div>
          <div className={g.grid4}>
            <Spec caption="Help">
              <TextField label="Folder" defaultValue="~/Minutae" help={`Recordings stay ${here}.`} />
            </Spec>
            <Spec caption="Hover">
              <TextField label="Meeting title" placeholder="Untitled meeting" forceState="hover" />
            </Spec>
            <Spec caption="Disabled">
              <TextField label="Model" defaultValue="whisper small" disabled />
            </Spec>
            <Spec caption="Search · focused">
              <SearchField platform={platform} placeholder="Search meetings, people, decisions" forceState="focus" />
            </Spec>
          </div>
        </Section>

        <Section title="Select" note="Checkmark marks the choice. ↑↓ ⏎ esc and type-ahead.">
          <div className={g.row}>
            <Spec caption="Closed · interactive">
              <Select aria-label="Microphone" options={MICS} value={mic} onValueChange={setMic} />
            </Spec>
            <Spec caption="Placeholder">
              <Select aria-label="Language" placeholder="Choose a language" options={[{ value: "en", label: "English" }, { value: "hi", label: "Hindi" }]} />
            </Spec>
            <Spec caption="Disabled">
              <Select aria-label="Microphone" options={MICS} value="builtin" disabled />
            </Spec>
            <Spec caption="Open · hover on row 2">
              <MenuPreview options={MICS} value="builtin" highlighted="airpods" />
            </Spec>
          </div>
        </Section>

        <Section title="Tabs & segments" note="Tabs switch views. Segments switch a value.">
          <div className={g.row} style={{ gap: 56 }}>
            <Spec caption="Tabs · underline" className={g.stack}>
              <Tabs defaultValue="summary" style={{ width: 360 }}>
                <TabsList aria-label="Meeting views">
                  <TabsTrigger value="summary">Summary</TabsTrigger>
                  <TabsTrigger value="transcript">Transcript</TabsTrigger>
                  <TabsTrigger value="ask" shortcut="mod+3" platform={platform}>
                    Ask
                  </TabsTrigger>
                </TabsList>
                <TabsContent value="summary" />
                <TabsContent value="transcript" />
                <TabsContent value="ask" />
              </Tabs>
            </Spec>
            <Spec caption="Segmented">
              <Segmented
                aria-label="Where to transcribe"
                value={where}
                onValueChange={setWhere}
                options={[
                  { value: "local", label: `On this ${platform === "mac" ? "Mac" : platform === "windows" ? "PC" : "computer"}` },
                  { value: "cloud", label: "Cloud" },
                ]}
              />
            </Spec>
            <Spec caption="Filter chips">
              <div className={g.inline} style={{ gap: 8 }}>
                {["all", "meetings", "dictations"].map((f) => (
                  <FilterChip key={f} selected={filter === f} onClick={() => setFilter(f)}>
                    {f === "all" ? "All" : f === "meetings" ? "Meetings" : "Dictations"}
                  </FilterChip>
                ))}
              </div>
            </Spec>
          </div>
          <div className={g.row} style={{ gap: 56 }}>
            <Spec caption="Chip · hover · focus · disabled">
              <div className={g.inline} style={{ gap: 8 }}>
                <FilterChip data-force="hover">Hover</FilterChip>
                <FilterChip data-force="focus">Focus</FilterChip>
                <FilterChip disabled>Disabled</FilterChip>
                <FilterChip selected count={12}>
                  With count
                </FilterChip>
              </div>
            </Spec>
            <Spec caption="Tags · removable">
              <div className={g.inline} style={{ gap: 8 }}>
                {tags.map((t) => (
                  <Tag key={t} onRemove={() => setTags((ts) => ts.filter((x) => x !== t))}>
                    {t}
                  </Tag>
                ))}
                <Tag>static</Tag>
              </div>
            </Spec>
            <Spec caption="Status pill">
              <div className={g.inline} style={{ gap: 8 }}>
                <Pill>3 actions</Pill>
                <Pill>Done</Pill>
              </div>
            </Spec>
          </div>
        </Section>

        <Section title="Choice" note="Toggle applies instantly. No Save buttons anywhere.">
          <div className={g.row} style={{ gap: 44 }}>
            <Spec caption="Toggle · on · off · disabled · focus">
              <div className={g.inline}>
                <Toggle aria-label="Example on" checked={toggles.a} onCheckedChange={(a) => setToggles((t) => ({ ...t, a }))} />
                <Toggle aria-label="Example off" checked={toggles.b} onCheckedChange={(b) => setToggles((t) => ({ ...t, b }))} />
                <Toggle aria-label="Disabled" disabled />
                <Toggle aria-label="Disabled on" disabled checked />
                <Toggle aria-label="Focused" checked forceFocus />
              </div>
            </Spec>
            <Spec caption="Checkbox">
              <div className={g.inline}>
                <Checkbox aria-label="Unchecked" checked={checks.a} onCheckedChange={(v) => setChecks((c) => ({ ...c, a: v === true }))} />
                <Checkbox aria-label="Checked" checked={checks.b} onCheckedChange={(v) => setChecks((c) => ({ ...c, b: v === true }))} />
                <Checkbox aria-label="Mixed" checked="indeterminate" />
                <Checkbox aria-label="Disabled" disabled />
                <Checkbox aria-label="Focused" forceFocus />
              </div>
            </Spec>
            <Spec caption="Radio">
              <RadioGroup aria-label="Example radio" value={radio} onValueChange={setRadio} orientation="horizontal" style={{ gap: 12 }}>
                <RadioItem value="tiny" aria-label="Tiny" />
                <RadioItem value="small" aria-label="Small" />
                <RadioItem value="off" aria-label="Disabled" disabled />
              </RadioGroup>
            </Spec>
            <Spec caption="Keycap">
              <div className={g.inline} style={{ gap: 6 }}>
                <Shortcut keys="mod+k" variant="caps" platform={platform} />
                <KeyCap size="lg">esc</KeyCap>
              </div>
            </Spec>
            <Spec caption="Status dot">
              <div className={g.inline} style={{ gap: 16 }}>
                <StatusDot tone="ok" label="Ready" />
                <StatusDot tone="onair" label="On air" />
                <StatusDot tone="off" label="Off" />
              </div>
            </Spec>
          </div>
          <div className={g.row} style={{ gap: 44 }}>
            <Spec caption="With labels">
              <div className={g.stack}>
                <Checkbox label="Write a .md file for each meeting" defaultChecked />
                <Checkbox label="Keep audio after transcription" />
              </div>
            </Spec>
            <Spec caption="Radio group">
              <RadioGroup
                aria-label="Whisper model"
                defaultValue="small"
                options={[
                  { value: "base", label: "Base · 142 MB" },
                  { value: "small", label: "Small · 466 MB" },
                  { value: "medium", label: "Medium · 1.5 GB", disabled: true },
                ]}
              />
            </Spec>
            <Spec caption="Shortcut · text">
              <div className={g.stack}>
                <Shortcut keys="mod+r" platform={platform} />
                <Shortcut keys="alt+space" platform={platform} />
                <Shortcut keys="mod+shift+k" platform={platform} />
              </div>
            </Spec>
            <Spec caption="Tooltip">
              <Tooltip content="Search" shortcut="mod+k" platform={platform}>
                <Button variant="secondary">Hover me</Button>
              </Tooltip>
            </Spec>
          </div>
        </Section>

        <Section title="Feedback" note="Toasts confirm. HUD floats above every app.">
          <div className={g.row} style={{ gap: 40 }}>
            <Spec caption="Toast">
              <ToastSurface title="Summary copied" actionLabel="Undo" />
            </Spec>
            <Spec caption="Toast · no action">
              <ToastSurface title="Saved locally" />
            </Spec>
            <Spec caption="Progress">
              <div style={{ width: 280, paddingTop: 12 }}>
                <Progress value={0.62} caption={`Transcribing · 62% · ~2 min left`} />
              </div>
            </Spec>
            <Spec caption="Show toast">
              <Button variant="secondary" onClick={() => toast.show({ title: "Summary copied", actionLabel: "Undo" })}>
                Copy summary
              </Button>
            </Spec>
          </div>
          <Spec caption="Dictation HUD · listening · listening with timer · processing · inserted · copied · error">
            <div className={g.hudStage}>
              <HudPill state="listening" levels={liveLevels} />
              <HudPill state="listening" levels={liveLevels} elapsedMs={7000} />
              <HudPill state="processing" />
              <HudPill state="inserted" words={23} />
              <HudPill state="copied" />
              <HudPill state="error" message="Microphone is off. Allow it in Settings." />
            </div>
          </Spec>
        </Section>

        <Section title="Lists" note="Fixed lanes so values line up. Only the title column flexes.">
          <div className={g.list}>
            <GroupLabel count={3}>Today</GroupLabel>
            <LaneRow
              time="14:30"
              title="Q2 Design Sync"
              subtitle="Redesign launch moves to May. Alice owns the updated mockups."
              people="Alice, Bob, Carol"
              duration="32:14"
              status={<Pill>3 actions</Pill>}
              onClick={() => undefined}
            />
            <LaneRow
              time="11:00"
              title="Hiring loop — Senior iOS"
              subtitle={`Transcribing ${here}…`}
              people="Priya, Sam"
              duration="48:02"
              status={<Progress value={0.62} caption="62%" width={96} align="end" />}
              onClick={() => undefined}
            />
            <LaneRow
              time="09:15"
              title="Standup"
              subtitle="Recording now"
              people="Team · 6"
              duration="04:12"
              status={<StatusDot tone="onair" label="On air" />}
              selected
            />
          </div>
          <div className={g.list}>
            <GroupLabel count={1}>Earlier this week</GroupLabel>
            <LaneRow
              time="Wed"
              title="Weekly product review"
              subtitle="Search ships behind a flag. Onboarding copy needs one more pass."
              people="Product team · 6"
              duration="54:40"
              status={<Pill>Done</Pill>}
              onClick={() => undefined}
            />
          </div>
        </Section>

        <Section title="Settings" note="Title, one line of why, control on the right.">
          <div className={g.list}>
            <GroupLabel>Recording</GroupLabel>
            <SettingRow
              title="Ask when a call starts"
              description="Minutae notices when another app uses the mic and asks before recording."
              htmlFor="g-ask"
              control={<Toggle id="g-ask" defaultChecked />}
            />
            <SettingRow
              title="Microphone"
              description="Used for your voice. Other people come from system audio."
              control={<Select aria-label="Microphone" options={MICS} value={mic} onValueChange={setMic} />}
            />
            <SettingRow
              title="Transcribe"
              description={`Local keeps audio ${here}.`}
              control={
                <Segmented
                  aria-label="Transcribe"
                  value={where}
                  onValueChange={setWhere}
                  options={[
                    { value: "local", label: "Locally" },
                    { value: "cloud", label: "Cloud" },
                  ]}
                />
              }
            />
            <SettingRow
              title="Grok API key"
              description="Not set."
              control={<Button variant="quiet">Add key</Button>}
            />
            <SettingRow title="Speaker labels" description="Needs the diarization model." disabled control={<Toggle disabled />} />
          </div>
        </Section>

        <Section title="Navigation" note="240 px sidebar. Text-only nav, shortcuts in mono.">
          <div className={g.frame}>
            <Sidebar
              footer={
                <>
                  <MicStatus tone={live ? "onair" : "ok"} label={live ? "On air" : "Listening for calls"} />
                  <RecordPill fullWidth live={live} elapsedMs={elapsed} platform={platform} onClick={() => setLive((l) => !l)} />
                </>
              }
            >
              {(
                [
                  ["meetings", "Meetings", "mod+1"],
                  ["dictate", "Dictate", "mod+2"],
                  ["library", "Library", "mod+3"],
                  ["settings", "Settings", "mod+,"],
                ] as const
              ).map(([id, label, sc]) => (
                <NavItem key={id} active={nav === id} shortcut={sc} platform={platform} onClick={() => setNav(id)}>
                  {label}
                </NavItem>
              ))}
            </Sidebar>
            <div className={g.frameMain}>
              <div style={{ display: "flex", justifyContent: "flex-end" }}>
                <div style={{ width: 320 }}>
                  <SearchField platform={platform} placeholder="Search meetings, people, decisions" />
                </div>
              </div>
              <PageHeader
                overline="Friday, 20 November"
                title="Meetings"
                right={
                  <>
                    <Stat value="6h 12m" caption="Recorded this week" />
                    <Stat value="9" caption="Open actions" />
                  </>
                }
              />
              <div className={g.list}>
                <GroupLabel count={1}>Today</GroupLabel>
                <LaneRow
                  time="14:30"
                  title="Q2 Design Sync"
                  subtitle="Redesign launch moves to May."
                  people="Alice, Bob, Carol"
                  duration="32:14"
                  status={<Pill>3 actions</Pill>}
                  lanes={{ people: false }}
                />
              </div>
            </div>
          </div>
          <div className={g.row}>
            <Spec caption="Nav · hover · focus">
              <div style={{ width: 200 }} className={g.stack}>
                <NavItem shortcut="mod+2" platform={platform} data-force="hover">
                  Dictate
                </NavItem>
                <NavItem shortcut="mod+3" platform={platform} data-force="focus">
                  Library
                </NavItem>
              </div>
            </Spec>
            <Spec caption="Mic status">
              <div className={g.stack}>
                <MicStatus tone="ok" label="Listening for calls" />
                <MicStatus tone="onair" label="On air" />
                <MicStatus tone="off" label="Mic off" />
              </div>
            </Spec>
          </div>
        </Section>

        <Section title="Page" note="One display title per screen. Empty states teach one thing.">
          <div className={g.grid2}>
            <EmptyState
              title="No meetings yet."
              description={
                <>
                  Join a call and Minutae will ask, or press <Shortcut keys="mod+r" platform={platform} />.
                </>
              }
              action={<Button>Record now</Button>}
            />
            <EmptyState
              title="Codex isn't signed in."
              description="Summaries need a model. Sign in once and it stays signed in."
              action={
                <>
                  <Button variant="secondary">Sign in to Codex</Button>
                  <Button variant="quiet">Use another model</Button>
                </>
              }
            />
          </div>
          <Spec caption="Reading · Newsreader 17/26 · lead 21/31">
            <div className={g.stack}>
              <p className={g.lead}>The redesign launch moves to May so the onboarding copy can get one more pass.</p>
              <p className={g.reading}>
                Alice owns the updated mockups and will share them by Friday. Bob reviews the performance budget before the
                next sync. Carol flagged that search should ship behind a flag until the index is rebuilt {here}.
              </p>
            </div>
          </Spec>
        </Section>

        <Section title="Media" note="Waveform bars. Red tail only while recording.">
          <div className={g.row} style={{ gap: 44 }}>
            <Spec caption="Waveform · played">
              <Waveform levels={WAVE} height={40} progress={0.35} />
            </Spec>
            <Spec caption="Waveform · live tail">
              <Waveform levels={liveWave} height={40} liveTail={8} animated />
            </Spec>
          </div>
          <Spec caption="Player bar">
            <div className={g.playerFrame}>
              <PlayerBar
                playing={playing}
                onTogglePlay={() => setPlaying((p) => !p)}
                currentMs={pos}
                durationMs={32 * 60_000 + 14_000}
                onSeek={setPos}
                speed={speed}
                onSpeedChange={setSpeed}
                extra={<Button variant="quiet">Show in folder</Button>}
              />
            </div>
          </Spec>
        </Section>
      </div>
    </div>
  );
}

export interface GalleryProps {
  /** Initial theme when the gallery hosts its own ThemeProvider. */
  defaultTheme?: ThemePreference;
}

/**
 * Every component in every state. Mount at /gallery. Uses the app's
 * ThemeProvider when present, otherwise provides its own.
 */
export function Gallery({ defaultTheme = "system" }: GalleryProps) {
  const existing = useOptionalTheme();
  const body = (
    <TooltipProvider>
      <ToastProvider>
        <GalleryBody />
      </ToastProvider>
    </TooltipProvider>
  );
  if (existing) return body;
  return (
    <ThemeProvider defaultPreference={defaultTheme} storageKey="minutae.gallery.theme">
      {body}
    </ThemeProvider>
  );
}

export default Gallery;
