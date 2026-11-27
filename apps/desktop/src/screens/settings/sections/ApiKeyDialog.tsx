import { Button, Segmented, TextField, usePlatform, useToast } from "@minutae/ui";
import { useEffect, useRef, useState } from "react";
import { api } from "../../../lib/api";
import type { ApiProvider } from "../../../lib/types";
import { errorText } from "../../../hooks";
import d from "./dialog.module.css";

const NAMES: Record<ApiProvider, string> = { anthropic: "Anthropic", openai: "OpenAI", gemini: "Gemini", grok: "Grok" };
const PLACEHOLDER: Record<ApiProvider, string> = { anthropic: "sk-ant-…", openai: "sk-…", gemini: "AIza…", grok: "xai-…" };
const CONSOLE: Record<ApiProvider, string> = {
  anthropic: "console.anthropic.com",
  openai: "platform.openai.com",
  gemini: "aistudio.google.com",
  grok: "console.x.ai",
};

export interface ApiKeyDialogProps {
  open: boolean;
  onClose: () => void;
  /** Providers the user can pick between; the first is preselected. */
  providers: ApiProvider[];
  onSaved?: () => void;
}

function keychainName(platform: string): string {
  return platform === "mac" ? "macOS Keychain" : platform === "windows" ? "Windows Credential Manager" : "your system keyring";
}

/**
 * API key dialog. Native <dialog> (modal): traps focus, esc closes, focus returns to the opener.
 */
export function ApiKeyDialog({ open, onClose, providers, onSaved }: ApiKeyDialogProps) {
  const platform = usePlatform();
  const toast = useToast();
  const ref = useRef<HTMLDialogElement>(null);
  const opener = useRef<HTMLElement | null>(null);
  const [provider, setProvider] = useState<ApiProvider>(providers[0] ?? "gemini");
  const [key, setKey] = useState("");
  const [hint, setHint] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<"save" | "clear" | null>(null);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    if (open && !el.open) {
      opener.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
      setProvider(providers[0] ?? "gemini");
      el.showModal();
    }
    if (!open && el.open) {
      el.close();
      const prev = opener.current;
      opener.current = null;
      if (prev?.isConnected) prev.focus();
    }
  }, [open, providers]);

  useEffect(() => {
    if (!open) return;
    setKey("");
    setError(null);
    setHint(null);
    api
      .apiKeyHint(provider)
      .then(setHint)
      .catch(() => setHint(null));
  }, [open, provider]);

  const save = () => {
    const v = key.trim();
    if (!v) {
      setError(`Paste a ${NAMES[provider]} key first.`);
      return;
    }
    setBusy("save");
    api
      .setApiKey(provider, v)
      .then(() => {
        toast.show({ title: `${NAMES[provider]} key saved` });
        onSaved?.();
        onClose();
      })
      .catch((e) => setError(`That key was rejected. ${errorText(e)} Check it at ${CONSOLE[provider]}.`))
      .finally(() => setBusy(null));
  };

  const clear = () => {
    setBusy("clear");
    api
      .clearApiKey(provider)
      .then(() => {
        setHint(null);
        toast.show({ title: `${NAMES[provider]} key removed` });
        onSaved?.();
      })
      .catch((e) => setError(`Couldn't remove the key. ${errorText(e)}`))
      .finally(() => setBusy(null));
  };

  return (
    <dialog
      ref={ref}
      className={d.dialog}
      aria-labelledby="apikey-title"
      aria-describedby="apikey-desc"
      onClose={onClose}
      onCancel={(e) => {
        e.preventDefault();
        onClose();
      }}
      onClick={(e) => {
        if (e.target === ref.current) onClose();
      }}
    >
      <form
        className={d.body}
        onSubmit={(e) => {
          e.preventDefault();
          save();
        }}
      >
        <h2 id="apikey-title" className={d.title}>
          {providers.length > 1 ? "API keys" : `Add a ${NAMES[provider]} key`}
        </h2>
        <p id="apikey-desc" className={d.description}>Keys stay in {keychainName(platform)} and are only used when nothing on your list is ready.</p>
        {providers.length > 1 ? (
          <Segmented<ApiProvider>
            aria-label="Provider"
            value={provider}
            onValueChange={setProvider}
            fullWidth
            options={providers.map((p) => ({ value: p, label: NAMES[p] }))}
          />
        ) : null}
        <TextField
          label={`${NAMES[provider]} API key`}
          type="password"
          mono
          autoFocus
          autoComplete="off"
          spellCheck={false}
          placeholder={hint ?? PLACEHOLDER[provider]}
          value={key}
          onChange={(e) => {
            setKey(e.currentTarget.value);
            setError(null);
          }}
          error={error ?? undefined}
          help={hint ? `Saved key: ${hint}. Paste a new one to replace it.` : `Get one at ${CONSOLE[provider]}.`}
        />
        <div className={d.actions}>
          {hint ? (
            <Button variant="destructive" className={d.left} onClick={clear} loading={busy === "clear"}>
              Remove key
            </Button>
          ) : null}
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" loading={busy === "save"}>
            Save key
          </Button>
        </div>
      </form>
    </dialog>
  );
}
