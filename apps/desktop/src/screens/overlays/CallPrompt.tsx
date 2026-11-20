// Call-detected prompt window (label "prompt"). Non-activating panel: ⏎ records, esc dismisses.
import { Button, Checkbox, localPhrase, usePlatform, useToast } from "@minutae/ui";
import { useCallback, useEffect, useRef, useState } from "react";
import { api, events } from "../../lib/api";
import { errorText } from "../../hooks";
import s from "./overlays.module.css";
import { useTransparentWindow } from "./useTransparentWindow";

export default function CallPromptWindow() {
  useTransparentWindow();
  const platform = usePlatform();
  const toast = useToast();
  const [app, setApp] = useState<string | null>(null);
  const [visible, setVisible] = useState(true);
  const [always, setAlways] = useState(false);
  const [busy, setBusy] = useState<"record" | "dismiss" | null>(null);
  const busyRef = useRef(busy);
  busyRef.current = busy;

  useEffect(() => {
    const offs: (() => void)[] = [];
    let dead = false;
    const keep = (u: () => void) => (dead ? u() : offs.push(u));
    void events
      .callDetected((p) => {
        setApp(p.app);
        setAlways(false);
        setBusy(null);
        setVisible(true);
      })
      .then(keep);
    void events.callEnded(() => setVisible(false)).then(keep);
    return () => {
      dead = true;
      for (const u of offs) u();
    };
  }, []);

  const record = useCallback(() => {
    if (busyRef.current) return;
    setBusy("record");
    api
      .acceptCallPrompt()
      .then(() => setVisible(false))
      .catch((e) => toast.show({ title: `Couldn't start recording. ${errorText(e)}` }))
      .finally(() => setBusy(null));
  }, [toast]);

  const dismiss = useCallback(() => {
    if (busyRef.current) return;
    setBusy("dismiss");
    api
      .dismissCallPrompt(always)
      .then(() => setVisible(false))
      .catch(() => setVisible(false))
      .finally(() => setBusy(null));
  }, [always]);

  useEffect(() => {
    if (!visible) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        dismiss();
      } else if (e.key === "Enter") {
        // A focused button handles its own Enter.
        const t = e.target as HTMLElement | null;
        if (t?.closest("button")) return;
        e.preventDefault();
        record();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [visible, record, dismiss]);

  if (!visible) return null;
  const name = app ?? "An app";

  return (
    <div className={s.promptStage}>
      <section className={s.card} role="dialog" aria-labelledby="prompt-title" aria-describedby="prompt-body">
        <div className={s.cardHead}>
          <span className={s.liveDot} aria-hidden />
          <h1 id="prompt-title" className={s.cardTitle}>
            {name} is using your mic
          </h1>
          <span className={s.now}>now</span>
        </div>
        <p id="prompt-body" className={s.cardBody}>
          Record this call? Audio stays {localPhrase(platform)}.
        </p>
        <div className={s.cardActions}>
          <Button className={s.grow} onClick={record} loading={busy === "record"} shortcut="enter" platform={platform}>
            Record
          </Button>
          <Button variant="secondary" className={s.grow} onClick={dismiss} disabled={busy === "record"} shortcut="esc" platform={platform}>
            Not now
          </Button>
        </div>
        {app ? (
          <Checkbox
            className={s.check}
            label={<span className={s.checkLabel}>Always record {app} calls</span>}
            checked={always}
            onCheckedChange={(v) => setAlways(v === true)}
          />
        ) : null}
      </section>
    </div>
  );
}
