import { GroupLabel, Segmented, SettingRow, Tag, TextField, Toggle, cx, shortcutKeys, usePlatform } from "@minutae/ui";
import { useEffect, useState } from "react";
import { acceleratorFromEvent, toKitShortcut } from "../shared/format";
import s from "../settings.module.css";
import type { SectionProps } from "./types";

export function DictationSection({ settings, update }: SectionProps) {
  const platform = usePlatform();
  const [recording, setRecording] = useState(false);
  const [word, setWord] = useState("");

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
      void update({ dictationHotkey: acc });
    };
    const stop = () => setRecording(false);
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("blur", stop);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("blur", stop);
    };
  }, [recording, platform, update]);

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
        <SettingRow
          title="Shortcut"
          description={recording ? "Press the new shortcut. Esc cancels." : "Click the key, then press the one you want."}
          control={
            <>
              <button
                type="button"
                className={cx(s.capButton, recording && s.capRecording)}
                onClick={() => setRecording((r) => !r)}
                aria-label={recording ? "Recording shortcut. Press the new keys." : `Dictation shortcut: ${keys.join(" ")}. Click to change.`}
                aria-pressed={recording}
              >
                {recording ? "Press keys…" : keyLabel}
              </button>
              <Segmented
                aria-label="Shortcut mode"
                value={settings.dictationMode}
                onValueChange={(v) => update({ dictationMode: v })}
                options={[
                  { value: "hold", label: "Hold to talk" },
                  { value: "toggle", label: "Press to toggle" },
                ]}
              />
            </>
          }
        />
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
