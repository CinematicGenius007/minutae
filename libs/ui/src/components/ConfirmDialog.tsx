import * as RD from "@radix-ui/react-dialog";
import type { ReactNode } from "react";
import { Button } from "./Button";
import s from "./Dialog.module.css";

export interface ConfirmDialogProps {
  /** Opens the dialog (asChild: pass one button). */
  trigger?: ReactNode;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  title: string;
  description?: ReactNode;
  /** A verb: "Delete meeting". */
  confirmLabel: string;
  cancelLabel?: string;
  onConfirm: () => void;
  /** Destructive confirm renders as underlined destructive text, never a red fill. */
  destructive?: boolean;
}

/** Confirmation for destructive actions. esc closes; focus returns to the trigger. */
export function ConfirmDialog({
  trigger,
  open,
  onOpenChange,
  title,
  description,
  confirmLabel,
  cancelLabel = "Cancel",
  onConfirm,
  destructive = true,
}: ConfirmDialogProps) {
  return (
    <RD.Root open={open} onOpenChange={onOpenChange}>
      {trigger ? <RD.Trigger asChild>{trigger}</RD.Trigger> : null}
      <RD.Portal>
        <RD.Overlay className={s.overlay} />
        <RD.Content className={s.content}>
          <RD.Title className={s.title}>{title}</RD.Title>
          {description ? <RD.Description className={s.description}>{description}</RD.Description> : null}
          {!description ? <RD.Description className="mn-sr-only">{title}</RD.Description> : null}
          <div className={s.actions}>
            <RD.Close asChild>
              <Button variant="secondary">{cancelLabel}</Button>
            </RD.Close>
            <RD.Close asChild>
              <Button variant={destructive ? "destructive" : "primary"} onClick={onConfirm}>
                {confirmLabel}
              </Button>
            </RD.Close>
          </div>
        </RD.Content>
      </RD.Portal>
    </RD.Root>
  );
}
