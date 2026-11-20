import {
  GroupLabel,
  Segmented,
  Select,
  type SelectEntry,
  SettingRow,
  Shortcut,
  Toggle,
  type ThemePreference,
  usePlatform,
  useTheme,
} from "@minutae/ui";
import { api } from "../../../lib/api";
import type { Appearance } from "../../../lib/types";
import { toKitShortcut } from "../shared/format";
import { useLoad } from "../../../hooks";
import s from "../settings.module.css";
import type { SectionProps } from "./types";

const DEFAULT_MIC = "__system_default__";

export function GeneralSection({ settings, update }: SectionProps) {
  const platform = usePlatform();
  const theme = useTheme();
  const [devices] = useLoad(() => api.listInputDevices());

  const micOptions: SelectEntry[] = [
    ...(devices.data ?? []).map((d) => ({ value: d.id, label: d.name })),
    ...(devices.data?.length ? (["separator"] as const) : []),
    { value: DEFAULT_MIC, label: "Follow system default", muted: true },
  ];
  // A saved device that is unplugged stays selectable so the value is visible.
  if (settings.microphone && devices.data && !devices.data.some((d) => d.id === settings.microphone)) {
    micOptions.unshift({ value: settings.microphone, label: `${settings.microphone} (not connected)` });
  }

  const setAppearance = (v: Appearance) => {
    theme.setPreference(v as ThemePreference);
    void update({ appearance: v });
  };

  return (
    <>
      <section className={s.group} aria-labelledby="g-recording">
        <GroupLabel id="g-recording">Recording</GroupLabel>
        <SettingRow
          title="Ask when a call starts"
          htmlFor="s-autodetect"
          description="Minutae notices Zoom, Meet or Teams using your mic and offers to record."
          control={
            <Toggle id="s-autodetect" checked={settings.autoDetectMeetings} onCheckedChange={(v) => update({ autoDetectMeetings: v })} />
          }
        />
        <SettingRow
          title="Include system audio"
          htmlFor="s-sysaudio"
          description="Records the other people on the call, not only your voice."
          control={
            <Toggle id="s-sysaudio" checked={settings.includeSystemAudio} onCheckedChange={(v) => update({ includeSystemAudio: v })} />
          }
        />
        <SettingRow
          title="Microphone"
          htmlFor="s-mic"
          description={devices.status === "error" ? "Couldn't list microphones. Minutae follows the system default." : undefined}
          control={
            <Select
              id="s-mic"
              className={s.selectW}
              options={micOptions}
              value={settings.microphone ?? DEFAULT_MIC}
              placeholder={devices.status === "loading" ? "Loading…" : "Choose a microphone"}
              onValueChange={(v) => update({ microphone: v === DEFAULT_MIC ? null : v })}
            />
          }
        />
        <SettingRow
          title="Ask after"
          description="Wait before asking, so quick mic checks don't trigger a prompt."
          control={
            <Segmented
              aria-label="Ask after"
              value={String(settings.promptDelaySeconds)}
              onValueChange={(v) => update({ promptDelaySeconds: Number(v) })}
              options={[
                { value: "0", label: "Instantly" },
                { value: "5", label: "5 sec" },
                { value: "30", label: "30 sec" },
              ]}
            />
          }
        />
        <SettingRow
          title="Stop when the call ends"
          htmlFor="s-stopend"
          description="Stops 10 seconds after the other app releases the mic, then summarizes."
          control={
            <Toggle id="s-stopend" checked={settings.stopWhenCallEnds} onCheckedChange={(v) => update({ stopWhenCallEnds: v })} />
          }
        />
        <SettingRow
          title="Record shortcut"
          description="Starts or stops a recording from any app."
          control={<Shortcut keys={toKitShortcut(settings.recordShortcut)} variant="caps" platform={platform} />}
        />
      </section>

      <section className={s.group} aria-labelledby="g-app">
        <GroupLabel id="g-app">App</GroupLabel>
        <SettingRow
          title="Appearance"
          control={
            <Segmented<Appearance>
              aria-label="Appearance"
              value={settings.appearance}
              onValueChange={setAppearance}
              options={[
                { value: "system", label: "Match system" },
                { value: "light", label: "Light" },
                { value: "dark", label: "Dark" },
              ]}
            />
          }
        />
        <SettingRow
          title="Open at login"
          htmlFor="s-login"
          control={<Toggle id="s-login" checked={settings.launchAtLogin} onCheckedChange={(v) => update({ launchAtLogin: v })} />}
        />
      </section>
    </>
  );
}
