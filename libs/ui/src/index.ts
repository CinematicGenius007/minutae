/**
 * @minutae/ui — the "On air" component kit.
 *
 * App setup:
 *   import "@minutae/ui/styles.css";   // tokens + base
 *   import "@minutae/ui/fonts";        // bundled Geist, Geist Mono, Newsreader
 *   <ThemeProvider><ToastProvider>…</ToastProvider></ThemeProvider>
 */

// theme + platform
export {
  applyThemePreference,
  type ResolvedTheme,
  type ThemeContextValue,
  type ThemePreference,
  ThemeProvider,
  type ThemeProviderProps,
  useOptionalTheme,
  useTheme,
} from "./theme/ThemeProvider";
export { usePlatform } from "./theme/usePlatform";

// pure helpers
export { cx } from "./lib/cx";
export { clamp01, formatBytes, formatClock, formatDuration, formatRelativeDate } from "./lib/format";
export { detectPlatform, formatShortcut, localPhrase, type Platform, shortcutKeys } from "./lib/platform";

// actions
export { Button, type ButtonProps, type ButtonSize, type ButtonVariant } from "./components/Button";
export { RecordPill, type RecordPillProps } from "./components/RecordPill";
export { ConfirmDialog, type ConfirmDialogProps } from "./components/ConfirmDialog";
export { DropdownMenu, type DropdownMenuProps, type MenuAction, type MenuEntry } from "./components/DropdownMenu";

// inputs
export { TextField, type TextFieldProps } from "./components/TextField";
export { SearchField, type SearchFieldProps } from "./components/SearchField";
export { MenuPreview, type MenuPreviewProps, Select, type SelectEntry, type SelectOption, type SelectProps } from "./components/Select";
export { Segmented, type SegmentedOption, type SegmentedProps } from "./components/Segmented";
export { Tabs, TabsContent, TabsList, TabsTrigger, type TabsTriggerProps } from "./components/Tabs";
export { Toggle, type ToggleProps } from "./components/Toggle";
export { Checkbox, type CheckboxProps } from "./components/Checkbox";
export { type RadioOption, RadioGroup, type RadioGroupProps, RadioItem, type RadioItemProps } from "./components/Radio";
export { FilterChip, type FilterChipProps, Pill, Tag, type TagProps } from "./components/Chip";
export { KeyCap, type KeyCapProps, Shortcut, type ShortcutProps } from "./components/KeyCap";

// status + feedback
export { StatusDot, type StatusDotProps, type StatusTone } from "./components/StatusDot";
export { Progress, type ProgressProps } from "./components/Progress";
export { Spinner, type SpinnerProps } from "./components/Spinner";
export { type ToastOptions, ToastProvider, ToastSurface, type ToastSurfaceProps, useToast } from "./components/Toast";
export { type HudState, HudPill, type HudPillProps } from "./components/HudPill";
export { Tooltip, type TooltipProps, TooltipProvider } from "./components/Tooltip";

// structure
export { GroupLabel, type GroupLabelProps } from "./components/GroupLabel";
export { SettingRow, type SettingRowProps } from "./components/SettingRow";
export { type LaneRowProps, LaneRow, type LaneWidths } from "./components/LaneRow";
export { MicStatus, type MicStatusProps, NavItem, type NavItemProps, Sidebar, type SidebarProps, Wordmark } from "./components/Sidebar";
export { PageHeader, type PageHeaderProps } from "./components/PageHeader";
export { Stat, type StatProps } from "./components/Stat";
export { EmptyState, type EmptyStateProps } from "./components/EmptyState";

// media
export { Waveform, type WaveformProps } from "./components/Waveform";
export { PlayerBar, type PlayerBarProps } from "./components/PlayerBar";

// gallery (mount at /gallery)
export { Gallery, type GalleryProps } from "./gallery/Gallery";
