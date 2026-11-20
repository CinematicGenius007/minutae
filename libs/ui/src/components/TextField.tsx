import { type InputHTMLAttributes, type ReactNode, type Ref, useId } from "react";
import { cx } from "../lib/cx";
import s from "./Field.module.css";

export interface TextFieldProps extends Omit<InputHTMLAttributes<HTMLInputElement>, "size"> {
  /** Label above the field (13/500). */
  label?: ReactNode;
  /** Help text below (13, muted). Hidden while an error shows. */
  help?: ReactNode;
  /** Error below (13, destructive). Name the cause and one fix. */
  error?: ReactNode;
  /** Mono text for keys, paths, ids. */
  mono?: boolean;
  /** Trailing element inside the box (e.g. a quiet "Change" button). */
  trailing?: ReactNode;
  /** Applied to the outer wrapper. */
  className?: string;
  /** Forces a visual state in the gallery: "hover" | "focus". */
  forceState?: "hover" | "focus";
  ref?: Ref<HTMLInputElement>;
}

export function TextField({
  label,
  help,
  error,
  mono,
  trailing,
  className,
  id,
  disabled,
  forceState,
  ref,
  ...rest
}: TextFieldProps) {
  const auto = useId();
  const inputId = id ?? auto;
  const noteId = `${inputId}-note`;
  const note = error ?? help;
  return (
    <div className={cx(s.field, className)}>
      {label ? (
        <label className={s.label} htmlFor={inputId}>
          {label}
        </label>
      ) : null}
      <div
        className={cx(s.box, error ? s.invalid : undefined, disabled && s.disabled)}
        data-force={forceState}
      >
        <input
          ref={ref}
          id={inputId}
          className={cx(s.input, mono && s.mono)}
          disabled={disabled}
          aria-invalid={error ? true : undefined}
          aria-describedby={note ? noteId : undefined}
          {...rest}
        />
        {trailing ? <span className={s.adornment}>{trailing}</span> : null}
      </div>
      {note ? (
        <p id={noteId} className={error ? s.error : s.help} role={error ? "alert" : undefined}>
          {note}
        </p>
      ) : null}
    </div>
  );
}
