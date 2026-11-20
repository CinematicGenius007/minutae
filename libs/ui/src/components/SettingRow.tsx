import type { ReactNode } from "react";
import { cx } from "../lib/cx";
import s from "./List.module.css";

export interface SettingRowProps {
  title: ReactNode;
  description?: ReactNode;
  /** Trailing control (Toggle, Select, Button …). */
  control?: ReactNode;
  /** id of the control, so the title labels it. */
  htmlFor?: string;
  disabled?: boolean;
  className?: string;
}

/** ≥ 56 px settings row. Controls apply instantly. */
export function SettingRow({ title, description, control, htmlFor, disabled, className }: SettingRowProps) {
  const Title = htmlFor ? "label" : "div";
  return (
    <div className={cx(s.setting, disabled && s.settingDisabled, className)}>
      <div className={s.settingText}>
        <Title className={s.settingTitle} htmlFor={htmlFor}>
          {title}
        </Title>
        {description ? <div className={s.settingDescription}>{description}</div> : null}
      </div>
      {control ? <div className={s.settingControl}>{control}</div> : null}
    </div>
  );
}
