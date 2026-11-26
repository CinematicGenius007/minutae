import {
  Button,
  ConfirmDialog,
  GroupLabel,
  KeyCap,
  Segmented,
  Select,
  SettingRow,
  Tag,
  TextField,
  Toggle,
  cx,
  shortcutKeys,
  usePlatform,
  useToast,
} from "@minutae/ui";
import { useEffect, useState } from "react";
import { errorText, updateSettings, useShortcutStatus } from "../../../hooks";
import { api } from "../../../lib/api";
import type { AppSettings, ShortcutStatus } from "../../../lib/types";
import { acceleratorFromEvent, shortcutProblem, toKitShortcut } from "../shared/format";
import s from "../settings.module.css";
import type { SectionProps } from "./types";

const TOGGLE_DICTATION = "minutae --toggle-dictation";
const TOGGLE_RECORDING = "minutae --toggle-recording";
const RETENTION_OPTIONS = [
  { value: "forever", label: "Forever" },
  ...[7, 30, 90].map((days) => ({ value: String(days), label: `${days} days` })),
];

function ModeControl({ settings, update, holdSupported }: SectionProps & { holdSupported: boolean | null }) {
  const noHold = holdSupported === false;
  return (
    <Segmented
      aria-label="Shortcut mode"
      value={noHold ? "toggle" : settings.dictationMode}
      onValueChange={(v) => update({ dictationMode: v as AppSettings["dictationMode"] })}
      options={[
        { value: "hold", label: "Hold to talk", disabled: noHold },
        { value: "toggle", label: "Press to toggle" },
      ]}
    />
  );
}

/** Wayland: the desktop's GlobalShortcuts portal owns the keys. */
function PortalShortcutRows({ status, ...props }: SectionProps & { status: ShortcutStatus }) {
  const toast = useToast();
  const [opening, setOpening] = useState(false);
  const configure = () => {
    setOpening(true);
    api
      .configureShortcuts()
      .catch((e) => toast.show({ title: errorText(e) }))
      .finally(() => setOpening(false));
  };
  const where = status.canConfigure ? "" : " Change it in your system’s keyboard settings.";
  return (
    <>
      <SettingRow
        title="Shortcut"
        description={
          <>
            {status.dictateTrigger ? `Set by your desktop.${where}` : `Not set. Your desktop asks for a key when Minutae starts.${where}`}
            {status.detail ? <span className={s.fieldError}>{status.detail}</span> : null}
          </>
        }
        control={
          <>
            {status.dictateTrigger ? <KeyCap size="lg">{status.dictateTrigger}</KeyCap> : null}
            {status.canConfigure ? (
              <Button variant="secondary" className={s.smallBtn} loading={opening} onClick={configure}>
                Change in system settings
              </Button>
            ) : null}
          </>
        }
      />
      <SettingRow
        title="Hold or toggle"
        description={
          status.holdSupported === false
            ? "Your desktop doesn’t report when you let go of the key, so each press starts or stops dictation."
            : "Hold works when your desktop reports key releases. Otherwise Minutae switches to press to toggle."
        }
        control={<ModeControl {...props} holdSupported={status.holdSupported} />}
      />
    </>
  );
}

/** Wayland without the portal: no global shortcuts; bind the CLI flags instead. */
function NoPortalRows() {
  const toast = useToast();
  const copy = (cmd: string) =>
    api
      .copyText(cmd)
      .then(() => toast.show({ title: "Command copied" }))
      .catch((e) => toast.show({ title: errorText(e) }));
  return (
    <>
      <SettingRow
        title="Shortcut"
        description="Your desktop doesn’t let apps set global shortcuts. Add a custom shortcut in your system’s keyboard settings that runs this command. Each press starts or stops dictation."
        control={
          <>
            <code className={s.path}>{TOGGLE_DICTATION}</code>
            <Button variant="secondary" className={s.smallBtn} onClick={() => copy(TOGGLE_DICTATION)}>
              Copy
            </Button>
          </>
        }
      />
      <SettingRow
        title="Record shortcut"
        description="The same works for starting and stopping a recording."
        control={
          <>
            <code className={s.path}>{TOGGLE_RECORDING}</code>
            <Button variant="secondary" className={s.smallBtn} onClick={() => copy(TOGGLE_RECORDING)}>
              Copy
            </Button>
          </>
        }
      />
    </>
  );
}

export function DictationSection({ settings, update }: SectionProps) {
  const platform = usePlatform();
  const shortcuts = useShortcutStatus();
  const toast = useToast();
  const [deleting, setDeleting] = useState(false);
  const deleteAll = () => {
    setDeleting(true);
    api.deleteAllDictations()
      .then(() => toast.show({ title: "Dictations deleted" }))
      .catch((e) => toast.show({ title: errorText(e) }))
      .finally(() => setDeleting(false));
  };
  const [recording, setRecording] = useState(false);
  const [word, setWord] = useState("");
  // Inline error under the shortcut control: local validation or the backend's refusal.
  const [shortcutError, setShortcutError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  // Shortcut recorder: click the keycap, press the new combination; esc cancels.
  useEffect(() => {
    if (!recording) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") {
        setRecording(false);
        return;
      }
      const acc = acceleratorFromEvent(e, platform);
      if (!acc) return;
      setRecording(false);
      const problem = shortcutProblem(acc, platform, [{ accelerator: settings.recordShortcut, name: "Record" }]);
      if (problem) {
        setShortcutError(problem);
        return;
      }
      if (acc === settings.dictationHotkey) return;
      // Written directly (not through the section's toast wrapper) so a refusal shows inline;
      // the settings store restores the previous shortcut on failure.
      setSaving(true);
      updateSettings({ dictationHotkey: acc })
        .then(() => setShortcutError(null))
        .catch((err) => setShortcutError(errorText(err)))
        .finally(() => setSaving(false));
    };
    const stop = () => setRecording(false);
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("blur", stop);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("blur", stop);
    };
  }, [recording, platform, settings.recordShortcut, settings.dictationHotkey]);

  const keys = shortcutKeys(toKitShortcut(settings.dictationHotkey), platform);
  const keyLabel = platform === "mac" ? keys.join(" ") : keys.join(" + ");

  const add = () => {
    const w = word.trim();
    if (!w) return;
    if (!settings.vocabulary.some((v) => v.toLowerCase() === w.toLowerCase())) {
      void update({ vocabulary: [...settings.vocabulary, w] });
    }
    setWord("");
  };

  return (
    <>
      <section className={s.group} aria-labelledby="g-dictation">
        <GroupLabel id="g-dictation">Dictation</GroupLabel>
        {shortcuts?.backend === "portal" ? (
          <PortalShortcutRows status={shortcuts} settings={settings} update={update} />
        ) : shortcuts?.backend === "unavailable" ? (
          <NoPortalRows />
        ) : (
          <SettingRow
            title="Shortcut"
            description={
              <>
                {recording ? "Press the new shortcut. Esc cancels." : "Click the key, then press the one you want."}
                {shortcutError ? (
                  <span id="s-shortcut-error" className={s.fieldError} role="alert">
                    {shortcutError}
                  </span>
                ) : null}
              </>
            }
            control={
              <>
                <button
                  type="button"
                  className={cx(s.capButton, recording && s.capRecording, shortcutError && !recording && s.capInvalid)}
                  onClick={() => {
                    if (!recording) setShortcutError(null);
                    setRecording((r) => !r);
                  }}
                  disabled={saving}
                  aria-label={recording ? "Recording shortcut. Press the new keys." : `Dictation shortcut: ${keys.join(" ")}. Click to change.`}
                  aria-pressed={recording}
                  aria-invalid={shortcutError ? true : undefined}
                  aria-describedby={shortcutError ? "s-shortcut-error" : undefined}
                >
                  {recording ? "Press keys…" : keyLabel}
                </button>
                <ModeControl settings={settings} update={update} holdSupported={true} />
              </>
            }
          />
        )}
        {shortcuts?.cancelDetail && (
          <SettingRow title="Cancel with Esc" description={shortcuts.cancelDetail} />
        )}
        <SettingRow
          title="Tidy up what I say"
          htmlFor="s-tidy"
          description={`Removes ums and false starts, adds punctuation. Runs ${platform === "mac" ? "on this Mac" : platform === "windows" ? "on this PC" : "on this computer"}.`}
          control={<Toggle id="s-tidy" checked={settings.tidyDictation} onCheckedChange={(v) => update({ tidyDictation: v })} />}
        />
        <SettingRow
          title="Keep the audio"
          htmlFor="s-keepaudio"
          description="Off by default. Only the text is saved."
          control={
            <Toggle id="s-keepaudio" checked={settings.keepDictationAudio} onCheckedChange={(v) => update({ keepDictationAudio: v })} />
          }
        />
        <SettingRow
          title="Keep dictations for"
          htmlFor="s-dictation-retention"
          description="Minutae deletes older dictation text and any saved audio."
          control={
            <Select
              id="s-dictation-retention"
              options={RETENTION_OPTIONS}
              value={settings.dictationRetentionDays?.toString() ?? "forever"}
              onValueChange={(value) => update({ dictationRetentionDays: value === "forever" ? null : Number(value) })}
            />
          }
        />
        <SettingRow
          title="Delete all dictations"
          description="Remove all saved dictation text and audio from this device."
          control={
            <ConfirmDialog
              trigger={<Button variant="destructive" loading={deleting}>Delete all dictations…</Button>}
              title="Delete all dictations?"
              description="This permanently removes saved dictation text and audio from Minutae. Anything already typed into other apps stays."
              confirmLabel="Delete all dictations"
              onConfirm={deleteAll}
            />
          }
        />
      </section>

      <section className={cx(s.group, s.groupTight)} aria-labelledby="g-vocab">
        <GroupLabel id="g-vocab" count="Names, products, jargon">
          Words it should know
        </GroupLabel>
        <form
          className={s.vocabForm}
          onSubmit={(e) => {
            e.preventDefault();
            add();
          }}
        >
          <TextField
            className={s.vocabField}
            aria-label="Add a word"
            placeholder="Add a name, product or term"
            value={word}
            onChange={(e) => setWord(e.currentTarget.value)}
            trailing={
              <button type="submit" className={s.addBtn} disabled={!word.trim()}>
                Add
              </button>
            }
          />
        </form>
        {settings.vocabulary.length > 0 ? (
          <ul className={s.tags} aria-label="Words it knows">
            {settings.vocabulary.map((w) => (
              <li key={w}>
                <Tag onRemove={() => update({ vocabulary: settings.vocabulary.filter((v) => v !== w) })}>{w}</Tag>
              </li>
            ))}
          </ul>
        ) : (
          <p className={s.providerDetail}>No words yet. Add names and jargon so dictation spells them right.</p>
        )}
      </section>
    </>
  );
}
