import type { ReactNode } from "react";
import { Button, Progress, SettingRow, Spinner, useToast } from "@minutae/ui";
import { errorText, useEvent, useLoad } from "../../../hooks";
import { api, events } from "../../../lib/api";
import type { UpdateStatus } from "../../../lib/types";
import s from "../settings.module.css";

/** Settings → General → App: "Check for updates" (updater state lives in the backend). */
export function UpdateRow() {
  const toast = useToast();
  const [status, reload, setStatus] = useLoad(() => api.updateStatus());
  useEvent(events.updateStatus, (u) => setStatus({ status: "ready", data: u, error: null }));

  const run = (fn: () => Promise<UpdateStatus>) =>
    fn()
      .then((u) => setStatus({ status: "ready", data: u, error: null }))
      .catch((e) => toast.show({ title: errorText(e) }));

  const u = status.data;
  const ready = String(status.status !== "loading");

  if (!u) {
    return (
      <div data-ready={ready}>
        <SettingRow
          title="Updates"
          description={status.status === "error" ? "Couldn't read the update status." : "Loading…"}
          control={
            status.status === "error" ? (
              <Button variant="quiet" className={s.quietLink} onClick={() => void reload()}>
                Try again
              </Button>
            ) : (
              <Spinner />
            )
          }
        />
      </div>
    );
  }

  const check = (label: string) => (
    <Button variant="quiet" className={s.quietLink} onClick={() => void run(api.checkForUpdates)}>
      {label}
    </Button>
  );

  let description: string;
  let control: ReactNode = null;
  switch (u.phase) {
    case "disabled":
      description = `Version ${u.currentVersion}. This build doesn't update itself.`;
      break;
    case "idle":
      description = `Version ${u.currentVersion}.`;
      control = check("Check for updates");
      break;
    case "checking":
      description = `Version ${u.currentVersion}.`;
      control = (
        <span className={s.inlineStatus} role="status">
          <Spinner /> Checking…
        </span>
      );
      break;
    case "upToDate":
      description = `Version ${u.currentVersion} is the latest.`;
      control = check("Check again");
      break;
    case "available":
      description = `Version ${u.version} is available. You have ${u.currentVersion}.`;
      control = (
        <Button variant="secondary" className={`${s.smallBtn} ${s.inkOutline}`} onClick={() => void run(api.installUpdate)}>
          Install and restart
        </Button>
      );
      break;
    case "installing": {
      const pct = Math.round((u.fraction ?? 0) * 100);
      description = `Installing ${u.version}. Minutae restarts when it's done.`;
      control = <Progress value={u.fraction ?? 0} width={96} align="end" caption={`${pct}%`} label="Update download" />;
      break;
    }
    case "error":
      description = u.error ?? "Couldn't check for updates.";
      control = check("Try again");
      break;
  }

  return (
    <div data-ready={ready}>
      <SettingRow title="Updates" description={description} control={control} />
    </div>
  );
}
