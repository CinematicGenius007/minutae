import { Search } from "lucide-react";
import type { InputHTMLAttributes, Ref } from "react";
import { cx } from "../lib/cx";
import type { Platform } from "../lib/platform";
import s from "./Field.module.css";
import { Shortcut } from "./KeyCap";

export interface SearchFieldProps extends Omit<InputHTMLAttributes<HTMLInputElement>, "size" | "type"> {
  /** Trailing hint; default "mod+k". Pass null to hide. */
  shortcut?: string | null;
  platform?: Platform;
  className?: string;
  forceState?: "hover" | "focus";
  ref?: Ref<HTMLInputElement>;
}

/** 36 px search box with a leading icon and a mono shortcut hint (⌘K / Ctrl+K). */
export function SearchField({
  shortcut = "mod+k",
  platform,
  placeholder = "Search",
  className,
  forceState,
  disabled,
  ref,
  ...rest
}: SearchFieldProps) {
  return (
    <div className={cx(s.box, s.search, disabled && s.disabled, className)} data-force={forceState}>
      <span className={s.adornment} aria-hidden>
        <Search size={14} strokeWidth={2.2} />
      </span>
      <input
        ref={ref}
        type="search"
        className={s.input}
        placeholder={placeholder}
        aria-label={rest["aria-label"] ?? placeholder}
        disabled={disabled}
        {...rest}
      />
      {shortcut ? (
        <span aria-hidden>
          <Shortcut keys={shortcut} platform={platform} />
        </span>
      ) : null}
    </div>
  );
}
