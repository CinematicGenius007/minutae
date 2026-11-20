import { Button, EmptyState, Spinner, Wordmark, cx, localPhrase, usePlatform, useToast } from "@minutae/ui";
import { useCallback, useEffect, useRef, useState } from "react";
import { useLocation } from "wouter";
import { api } from "../../lib/api";
import type { PermissionState, Permissions } from "../../lib/types";
import { errorText } from "../../hooks";
import s from "./onboarding.module.css";

type Kind = keyof Permissions;

const WORDS = ["", "One", "Two", "Three"];

function copyFor(kind: Kind, platform: string): { title: string; body: string } {
  switch (kind) {
    case "microphone":
      return { title: "Microphone", body: "To hear you in meetings and dictation." };
    case "systemAudio":
      return {
        title: "System audio",
        body:
          platform === "mac"
            ? "To hear the other people on the call. macOS calls this “Screen & System Audio”."
            : "To hear the other people on the call.",
      };
    case "accessibility":
      return {
        title: "Accessibility",
        body: "Only for dictation, to type text where your cursor is. Skip if you only record meetings.",
      };
  }
}

export default function OnboardingScreen() {
  const platform = usePlatform();
  const toast = useToast();
  const [, navigate] = useLocation();
  const [perms, setPerms] = useState<Permissions | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<Kind | null>(null);
  const [skipped, setSkipped] = useState<Set<Kind>>(new Set());
  // Kinds asked for in this session. Some never report back (macOS keeps "notDetermined"
  // for system audio and accessibility until the app restarts), so asking once settles the row.
  const [requested, setRequested] = useState<Set<Kind>>(new Set());
  const [finishing, setFinishing] = useState(false);

  const refresh = useCallback(() => {
    api
      .permissions()
      .then((p) => {
        setPerms(p);
        setError(null);
      })
      .catch((e) => setError(errorText(e)));
  }, []);

  // Re-check when the user comes back from System Settings.
  useEffect(() => {
    refresh();
    window.addEventListener("focus", refresh);
    return () => window.removeEventListener("focus", refresh);
  }, [refresh]);

  const request = (kind: Kind) => {
    setBusy(kind);
    api
      .requestPermission(kind)
      .then((p) => {
        setPerms(p);
        setRequested((prev) => new Set(prev).add(kind));
      })
      .catch((e) => toast.show({ title: `Couldn't ask for access. ${errorText(e)}` }))
      .finally(() => setBusy(null));
  };

  const openSettings = (kind: Kind) => {
    api.openSystemSettings(kind).catch((e) => toast.show({ title: `Couldn't open settings. ${errorText(e)}` }));
  };

  const finish = useCallback(
    (replace = false) => {
      setFinishing(true);
      api
        .finishOnboarding()
        .then(() => navigate("/", { replace }))
        .catch((e) => {
          toast.show({ title: `Couldn't finish setup. ${errorText(e)}` });
          setFinishing(false);
        });
    },
    [navigate, toast],
  );

  const kinds: Kind[] = (["microphone", "systemAudio", "accessibility"] as Kind[]).filter(
    (k) => perms && perms[k] !== "notRequired",
  );
  const settled = (k: Kind, st: PermissionState) =>
    st === "granted" || st === "unsupported" || skipped.has(k) || (requested.has(k) && st === "notDetermined");

  // Nothing to switch on here (Windows / Linux: every permission is notRequired): skip the screen.
  const nothingToAsk = perms !== null && kinds.length === 0;
  const autoFinished = useRef(false);
  useEffect(() => {
    if (!nothingToAsk || autoFinished.current) return;
    autoFinished.current = true;
    finish(true);
  }, [nothingToAsk, finish]);
  // The next row that needs the user is the only primary button on screen.
  const next = perms ? kinds.find((k) => !settled(k, perms[k])) : undefined;
  const allDone = perms !== null && next === undefined;
  const count = kinds.length;
  const deviceWord = localPhrase(platform);

  return (
    <main className={s.page} aria-labelledby="onboarding-title" data-screen="onboarding" data-ready={String(perms !== null || error !== null)}>
      <div className={s.top}>
        <Wordmark />
      </div>
      <div className={s.layout}>
        <div className={s.intro}>
          <h1 id="onboarding-title" className={s.title}>
            {!perms
              ? "A few switches, then you're done."
              : count === 0
                ? "Nothing to switch on. You're done."
                : `${count === 1 ? "One switch" : `${WORDS[count] ?? count} switches`}, then you're done.`}
          </h1>
          <p className={s.lead}>
            Everything stays {deviceWord}. You can change any of these later in Settings.
          </p>
          <div className={s.continue}>
            <Button variant={allDone ? "primary" : "secondary"} onClick={() => finish()} loading={finishing} disabled={!perms}>
              Continue
            </Button>
            {!allDone && perms ? <span className={s.later}>You can skip for now.</span> : null}
          </div>
        </div>

        <div className={s.rows}>
          {error && !perms ? (
            <EmptyState
              title="Couldn't check permissions."
              description={error}
              action={
                <Button variant="secondary" onClick={refresh}>
                  Try again
                </Button>
              }
            />
          ) : !perms ? (
            <div className={s.loading}>
              <Spinner /> Checking permissions…
            </div>
          ) : (
            <ol>
              {kinds.map((k, i) => {
                const st = perms[k];
                const c = copyFor(k, platform);
                const optional = k === "accessibility";
                const quiet = optional && st !== "granted";
                const asked = requested.has(k) && st === "notDetermined";
                return (
                  <li key={k} className={s.row}>
                    <span className={s.no} aria-hidden>
                      {String(i + 1).padStart(2, "0")}
                    </span>
                    <div className={s.text}>
                      <h2 className={cx(s.rowTitle, quiet && s.rowTitleQuiet)}>{c.title}</h2>
                      <p className={s.rowBody}>
                        {st === "denied"
                          ? `Turned off. Allow Minutae in ${platform === "mac" ? "System Settings" : "Settings"}, then come back.`
                          : st === "unsupported"
                            ? "Not available on this system. Minutae records your mic only."
                            : asked
                              ? `Asked. If it's still off, allow Minutae in ${platform === "mac" ? "System Settings" : "Settings"}.`
                              : c.body}
                      </p>
                    </div>
                    <div className={s.action}>
                      {st === "granted" ? (
                        <span className={s.allowed}>
                          <svg width="12" height="12" viewBox="0 0 24 24" aria-hidden>
                            <path d="M5 12l5 5 9-10" fill="none" stroke="currentColor" strokeWidth="3.2" strokeLinecap="round" strokeLinejoin="round" />
                          </svg>
                          <span>Allowed</span>
                        </span>
                      ) : st === "unsupported" ? (
                        <span className={s.na}>Not available</span>
                      ) : st === "denied" || asked ? (
                        <Button variant="secondary" className={s.btn} onClick={() => openSettings(k)}>
                          Open Settings
                        </Button>
                      ) : skipped.has(k) ? (
                        <Button variant="quiet" className={s.quiet} onClick={() => request(k)}>
                          Allow now
                        </Button>
                      ) : optional && next !== k ? (
                        <Button variant="quiet" className={s.quiet} onClick={() => setSkipped((prev) => new Set(prev).add(k))}>
                          Later
                        </Button>
                      ) : (
                        <span className={s.pair}>
                          {optional ? (
                            <Button
                              variant="quiet"
                              className={s.quiet}
                              onClick={() => setSkipped((prev) => new Set(prev).add(k))}
                            >
                              Later
                            </Button>
                          ) : null}
                          <Button
                            variant={next === k ? "primary" : "secondary"}
                            className={s.btn}
                            onClick={() => request(k)}
                            loading={busy === k}
                          >
                            Allow…
                          </Button>
                        </span>
                      )}
                    </div>
                  </li>
                );
              })}
            </ol>
          )}
        </div>
      </div>
    </main>
  );
}
