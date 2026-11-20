import * as RT from "@radix-ui/react-toast";
import { createContext, type ReactNode, useCallback, useContext, useMemo, useState } from "react";
import { cx } from "../lib/cx";
import s from "./Hud.module.css";

export interface ToastOptions {
  /** One short sentence: "Summary copied". */
  title: string;
  /** Optional single action, e.g. "Undo". */
  actionLabel?: string;
  onAction?: () => void;
  /** ms; default 4000 (§7). */
  duration?: number;
}

interface ToastState extends ToastOptions {
  id: number;
}

interface ToastApi {
  show: (options: ToastOptions) => void;
  dismiss: () => void;
}

const ToastContext = createContext<ToastApi | null>(null);

/**
 * Hosts the one toast (bottom-centre, 4 s). A new toast replaces the current one.
 */
export function ToastProvider({ children }: { children: ReactNode }) {
  const [toast, setToast] = useState<ToastState | null>(null);
  const [open, setOpen] = useState(false);

  const show = useCallback((options: ToastOptions) => {
    setToast({ ...options, id: Date.now() + Math.random() });
    setOpen(true);
  }, []);
  const dismiss = useCallback(() => setOpen(false), []);
  const api = useMemo(() => ({ show, dismiss }), [show, dismiss]);

  return (
    <ToastContext.Provider value={api}>
      <RT.Provider swipeDirection="down" duration={4000}>
        {children}
        {toast ? (
          <RT.Root
            key={toast.id}
            open={open}
            onOpenChange={setOpen}
            duration={toast.duration ?? 4000}
            className={cx(s.toast, s.toastRoot, !toast.actionLabel && s.toastNoAction)}
          >
            <RT.Title>{toast.title}</RT.Title>
            {toast.actionLabel ? (
              <RT.Action altText={toast.actionLabel} asChild>
                <button type="button" className={s.toastAction} onClick={toast.onAction}>
                  {toast.actionLabel}
                </button>
              </RT.Action>
            ) : null}
          </RT.Root>
        ) : null}
        <RT.Viewport className={s.viewport} />
      </RT.Provider>
    </ToastContext.Provider>
  );
}

/** `const toast = useToast(); toast.show({ title: "Summary copied", actionLabel: "Undo", onAction })` */
export function useToast(): ToastApi {
  const ctx = useContext(ToastContext);
  if (!ctx) throw new Error("useToast must be used inside <ToastProvider>");
  return ctx;
}

export interface ToastSurfaceProps {
  title: ReactNode;
  actionLabel?: string;
  onAction?: () => void;
  className?: string;
}

/** Static toast surface (gallery, screenshots). Use useToast() in the app. */
export function ToastSurface({ title, actionLabel, onAction, className }: ToastSurfaceProps) {
  return (
    <div className={cx(s.toast, !actionLabel && s.toastNoAction, className)}>
      <span>{title}</span>
      {actionLabel ? (
        <button type="button" className={s.toastAction} onClick={onAction}>
          {actionLabel}
        </button>
      ) : null}
    </div>
  );
}
