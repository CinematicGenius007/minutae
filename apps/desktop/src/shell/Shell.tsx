// App shell: sidebar (≥ 900 px) or top bar + bottom tab bar (< 900 px), global shortcuts,
// backend navigation, theme from settings, and the macOS title-bar drag strip.
//
// Screens render inside <main>, which is the scroll container. Screens get `--page-pad`
// (72 / 40 / 20 by width) and `--top-inset` (28 px on macOS narrow layouts) from here.
import { Search } from "lucide-react";
import { type ReactNode, useEffect } from "react";
import { useLocation } from "wouter";
import { cx, MicStatus, NavItem, RecordPill, Sidebar, useTheme, Wordmark } from "@minutae/ui";
import {
  openSearch,
  useEvent,
  useGo,
  useNarrow,
  usePlatform,
  useRecordActions,
  useRecording,
  useSettings,
  useShortcut,
} from "../hooks";
import { api, events, inTauri } from "../lib/api";
import s from "./Shell.module.css";
import { type Place, TabBar } from "./TabBar";

const MAC_TRAFFIC_LIGHTS = 28;

function usePlaces(): Place[] {
  const [loc] = useLocation();
  return [
    { href: "/", label: "Meetings", active: loc === "/" || loc.startsWith("/meetings") || loc === "/live" },
    { href: "/dictate", label: "Dictate", active: loc.startsWith("/dictate") },
    { href: "/library", label: "Library", active: loc.startsWith("/library") },
    { href: "/settings/general", label: "Settings", active: loc.startsWith("/settings") },
  ];
}

// First run: checked once per window load. Onboarding's "finish" returns to "/".
let onboardingChecked = false;
function useFirstRunRedirect(go: (to: string, opts?: { replace?: boolean }) => void) {
  useEffect(() => {
    if (onboardingChecked) return;
    onboardingChecked = true;
    api
      .onboardingDone()
      .then((done) => {
        if (!done) go("/onboarding", { replace: true });
      })
      .catch(() => {});
  }, [go]);
}

const SHORTCUTS: Record<string, string> = { Meetings: "mod+1", Dictate: "mod+2", Library: "mod+3", Settings: "mod+," };

export default function Shell({ children }: { children: ReactNode }) {
  const go = useGo();
  const platform = usePlatform();
  const narrow = useNarrow();
  const places = usePlaces();
  const { state, elapsedMs, live, paused } = useRecording();
  const { start, toggle, busy } = useRecordActions();
  const { settings } = useSettings();
  const { setPreference } = useTheme();
  useFirstRunRedirect(go);

  // Theme follows Settings → General → Appearance (loaded on start, live on change).
  useEffect(() => {
    if (settings) setPreference(settings.appearance);
  }, [settings?.appearance, setPreference, settings]);

  // Tray, menu bar and global shortcuts ask the window to navigate.
  useEvent(events.navigate, ({ route }) => go(route));

  useShortcut(["mod+1", "mod+2", "mod+3", "mod+,"], (_e, combo) => {
    const to = { "mod+1": "/", "mod+2": "/dictate", "mod+3": "/library", "mod+,": "/settings/general" }[combo];
    if (to) go(to);
  });
  useShortcut("mod+k", () => openSearch(go));
  useShortcut("mod+r", () => {
    if (!busy) void toggle();
  });

  const onRecordPill = () => (live ? go("/live") : void start());

  const mic = live
    ? paused
      ? { tone: "off" as const, label: "Paused" }
      : { tone: "onair" as const, label: state?.sourceApp ? `On air · ${state.sourceApp}` : "On air" }
    : settings && !settings.autoDetectMeetings
      ? { tone: "off" as const, label: "Not listening for calls" }
      : { tone: "ok" as const, label: "Listening for calls" };

  const mac = platform === "mac";
  const skip = (
    <a className={s.skip} href="#main" onClick={(e) => {
      e.preventDefault();
      document.getElementById("main")?.focus();
    }}>
      Skip to content
    </a>
  );

  if (narrow) {
    return (
      <div className={cx(s.shell, mac && inTauri && s.mac)}>
        {skip}
        {mac && inTauri ? <div className={s.drag} data-tauri-drag-region /> : null}
        <header className={s.topbar}>
          <Wordmark />
          <button type="button" className={s.iconButton} onClick={() => openSearch(go)} aria-label="Search">
            <Search size={20} strokeWidth={2} aria-hidden />
          </button>
        </header>
        <main id="main" tabIndex={-1} className={s.main}>
          {children}
        </main>
        <TabBar places={places} live={live} onRecord={onRecordPill} />
      </div>
    );
  }

  return (
    <div className={cx(s.shell, mac && inTauri && s.mac)}>
      {skip}
      {mac && inTauri ? <div className={s.drag} data-tauri-drag-region /> : null}
      <Sidebar
        topInset={mac && inTauri ? MAC_TRAFFIC_LIGHTS : undefined}
        footer={
          <>
            <MicStatus tone={mic.tone} label={mic.label} />
            <RecordPill
              fullWidth
              live={live}
              elapsedMs={elapsedMs}
              platform={platform}
              disabled={busy && !live}
              onClick={onRecordPill}
              srHint={live ? "open the live view" : undefined}
            />
          </>
        }
      >
        {places.map((p) => (
          <NavItem key={p.href} href={`#${p.href}`} active={p.active} shortcut={SHORTCUTS[p.label]} platform={platform}>
            {p.label}
          </NavItem>
        ))}
      </Sidebar>
      <main id="main" tabIndex={-1} className={s.main}>
        {children}
      </main>
    </div>
  );
}
