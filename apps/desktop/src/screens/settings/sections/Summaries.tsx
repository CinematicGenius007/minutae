import { Button, GroupLabel, Segmented, SettingRow, Spinner, StatusDot, Toggle, cx, usePlatform, useToast } from "@minutae/ui";
import { type KeyboardEvent, useMemo, useState } from "react";
import { api } from "../../../lib/api";
import type { ApiProvider, ProviderStatus, Transport } from "../../../lib/types";
import { tildify } from "../shared/format";
import { errorText, useLoad } from "../../../hooks";
import p from "../shared/page.module.css";
import s from "../settings.module.css";
import { ApiKeyDialog } from "./ApiKeyDialog";
import type { SectionProps } from "./types";

const API_PROVIDERS: ApiProvider[] = ["anthropic", "openai", "gemini", "grok"];

function keychainName(platform: string): string {
  return platform === "mac" ? "macOS Keychain" : platform === "windows" ? "Windows Credential Manager" : "your system keyring";
}

/** Status lane: dot + word, or the one action that fixes it. */
function ProviderStatusLane({ p: pr, onAddKey }: { p: ProviderStatus; onAddKey: () => void }) {
  if (pr.availability === "ready") return <StatusDot tone="ok" label="Ready" />;
  if (pr.transport === "grokCli" || pr.transport === "api") {
    return (
      <Button variant="secondary" className={cx(s.smallBtn, s.inkOutline)} onClick={onAddKey}>
        Add key
      </Button>
    );
  }
  if (pr.availability === "notLoggedIn") return <StatusDot tone="off" label="Not signed in" />;
  if (pr.availability === "notInstalled") return <StatusDot tone="off" label="Not installed" />;
  return <StatusDot tone="off" label="Unavailable" />;
}

export function SummariesSection({ settings, update }: SectionProps) {
  const platform = usePlatform();
  const toast = useToast();
  const [statuses, reloadStatuses] = useLoad(() => api.providerStatuses());
  const [storage] = useLoad(() => api.storageUsage());
  const [dialog, setDialog] = useState<ApiProvider[] | null>(null);
  const [dragging, setDragging] = useState<Transport | null>(null);
  const [over, setOver] = useState<Transport | null>(null);
  const [announce, setAnnounce] = useState("");

  // Rows in transportOrder; providers the order doesn't mention go last.
  const rows = useMemo(() => {
    const list = statuses.data ?? [];
    const rank = (t: Transport) => {
      const i = settings.transportOrder.indexOf(t);
      return i === -1 ? Number.MAX_SAFE_INTEGER : i;
    };
    return [...list].sort((a, b) => rank(a.transport) - rank(b.transport));
  }, [statuses.data, settings.transportOrder]);

  const order = rows.map((r) => r.transport);

  const move = (t: Transport, to: number) => {
    const from = order.indexOf(t);
    if (from === -1 || to < 0 || to >= order.length || from === to) return;
    const next = [...order];
    next.splice(from, 1);
    next.splice(to, 0, t);
    void update({ transportOrder: next });
    const name = rows.find((r) => r.transport === t)?.name ?? t;
    setAnnounce(`${name} moved to position ${to + 1} of ${next.length}.`);
  };

  const onHandleKey = (e: KeyboardEvent<HTMLButtonElement>, t: Transport, i: number) => {
    if (e.key === "ArrowUp") {
      e.preventDefault();
      move(t, i - 1);
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      move(t, i + 1);
    }
  };

  // Keep focus on the moved handle after re-render.
  const focusHandle = (t: Transport) => requestAnimationFrame(() => document.getElementById(`handle-${t}`)?.focus());

  const changeFolder = () => {
    api
      .pickFolder()
      .then((path) => {
        if (path) void update({ summariesPath: path });
      })
      .catch((e) => toast.show({ title: `Couldn't open the folder picker. ${errorText(e)}` }));
  };

  const folder = settings.summariesPath ?? (storage.data ? `${storage.data.dataDir}/summaries` : "");

  return (
    <>
      <section
        className={s.group}
        aria-labelledby="g-providers"
        data-ready={String(statuses.status !== "loading" && storage.status !== "loading")}
      >
        <GroupLabel id="g-providers" count="Tried in order · drag to reorder">
          Summarize with
        </GroupLabel>
        {statuses.status === "loading" ? (
          <div className={p.loading}>
            <Spinner /> Checking providers…
          </div>
        ) : statuses.status === "error" && !statuses.data ? (
          <SettingRow
            title="Couldn't check the providers."
            description={statuses.error}
            control={
              <Button variant="quiet" className={s.quietLink} onClick={() => void reloadStatuses()}>
                Try again
              </Button>
            }
          />
        ) : (
          <ol aria-label="Provider order">
            {rows.map((r, i) => {
              const ready = r.availability === "ready";
              return (
                <li
                  key={r.transport}
                  className={s.provider}
                  data-dragging={dragging === r.transport ? "" : undefined}
                  data-over={over === r.transport && dragging !== r.transport ? "" : undefined}
                  draggable
                  onDragStart={(e) => {
                    setDragging(r.transport);
                    e.dataTransfer.effectAllowed = "move";
                    e.dataTransfer.setData("text/plain", r.transport);
                  }}
                  onDragEnd={() => {
                    setDragging(null);
                    setOver(null);
                  }}
                  onDragOver={(e) => {
                    if (!dragging) return;
                    e.preventDefault();
                    setOver(r.transport);
                  }}
                  onDrop={(e) => {
                    e.preventDefault();
                    if (dragging) move(dragging, i);
                    setDragging(null);
                    setOver(null);
                  }}
                >
                  <button
                    id={`handle-${r.transport}`}
                    type="button"
                    className={s.handle}
                    aria-label={`Reorder ${r.name}, position ${i + 1} of ${rows.length}. Use the up and down arrow keys.`}
                    onKeyDown={(e) => {
                      onHandleKey(e, r.transport, i);
                      focusHandle(r.transport);
                    }}
                  >
                    <svg width="14" height="14" viewBox="0 0 24 24" aria-hidden>
                      {[6, 12, 18].map((y) => (
                        <g key={y}>
                          <circle cx="9" cy={y} r="1.6" fill="currentColor" />
                          <circle cx="15" cy={y} r="1.6" fill="currentColor" />
                        </g>
                      ))}
                    </svg>
                  </button>
                  <span className={s.providerNo} aria-hidden>
                    {String(i + 1).padStart(2, "0")}
                  </span>
                  <span className={s.providerText}>
                    <span className={cx(s.providerName, !ready && s.providerOff)}>{r.name}</span>
                    {r.detail ? <span className={s.providerDetail}>{r.detail}</span> : null}
                  </span>
                  <span className={s.providerRight}>
                    <ProviderStatusLane
                      p={r}
                      onAddKey={() => setDialog(r.transport === "grokCli" ? ["grok"] : API_PROVIDERS)}
                    />
                  </span>
                </li>
              );
            })}
          </ol>
        )}
        <span className="mn-sr-only" aria-live="polite">
          {announce}
        </span>
      </section>

      <section className={s.group} aria-labelledby="g-output">
        <GroupLabel id="g-output">Output</GroupLabel>
        <SettingRow
          title="Length"
          description="Brief fits on one screen. Detailed keeps every topic."
          control={
            <Segmented
              aria-label="Summary length"
              value={settings.summaryLength}
              onValueChange={(v) => update({ summaryLength: v })}
              options={[
                { value: "brief", label: "Brief" },
                { value: "detailed", label: "Detailed" },
              ]}
            />
          }
        />
        <SettingRow
          title="Markdown folder"
          description="One .md file per meeting. Point it at your Obsidian vault if you like."
          control={
            <>
              <span className={s.path} title={folder}>
                {tildify(folder)}
              </span>
              <Button variant="quiet" className={s.quietLink} onClick={changeFolder} aria-label="Change Markdown folder">
                Change
              </Button>
            </>
          }
        />
        <SettingRow
          title="Use API keys as last resort"
          htmlFor="s-apifallback"
          description={`Anthropic, OpenAI or Gemini. Keys stay in ${keychainName(platform)}.`}
          control={
            <>
              {settings.apiFallbackEnabled ? (
                <Button variant="quiet" className={s.quietLink} onClick={() => setDialog(API_PROVIDERS)}>
                  Manage keys
                </Button>
              ) : null}
              <Toggle
                id="s-apifallback"
                checked={settings.apiFallbackEnabled}
                onCheckedChange={(v) => update({ apiFallbackEnabled: v })}
              />
            </>
          }
        />
      </section>

      <ApiKeyDialog
        open={dialog !== null}
        providers={dialog ?? API_PROVIDERS}
        onClose={() => setDialog(null)}
        onSaved={() => void reloadStatuses()}
      />
    </>
  );
}
