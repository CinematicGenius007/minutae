import type { AppSettings } from "../../../lib/types";

export interface SectionProps {
  settings: AppSettings;
  update: (patch: Partial<AppSettings>) => Promise<void>;
}
