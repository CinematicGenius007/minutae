// Call-detected prompt window (label "prompt"). A non-activating panel: it never takes keyboard
// focus from the call app, so it has no key bindings or keycap hints; it is used with the pointer.
// If accepting fails the backend posts a notification, so the prompt just hides.
import { Button, Checkbox, localPhrase, usePlatform } from "@minutae/ui";
import { useCallback, useEffect, useRef, useState } from "react";
import { api, events } from "../../lib/api";
import s from "./overlays.module.css";
import { useTransparentWindow } from "./useTransparentWindow";

export default function CallPromptWindow() {
  useTransparentWindow();
  const platform = usePlatform();
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
      .catch(() => {})
      .finally(() => {
        setVisible(false);
        setBusy(null);
      });
  }, []);

  const dismiss = useCallback(() => {
    if (busyRef.current) return;
    setBusy("dismiss");
    api
      .dismissCallPrompt(always)
      .then(() => setVisible(false))
      .catch(() => setVisible(false))
      .finally(() => setBusy(null));
  }, [always]);

  if (!visible) return null;
  const name = app ?? "An app";

  return (
    <div className={s.promptStage} data-screen="prompt" data-ready="true">
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
          <Button className={s.grow} onClick={record} loading={busy === "record"}>
            Record
          </Button>
          <Button variant="secondary" className={s.grow} onClick={dismiss} disabled={busy === "record"}>
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
