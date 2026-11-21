// Shared app hooks. See each file's header for usage.
export { useEvent, type Subscribe } from "./useEvent";
export { type QueryOptions, type QueryResult, toError, useQuery } from "./useQuery";
export { isTypingTarget, matchesShortcut, type ShortcutOptions, useShortcut } from "./useShortcut";
export { osToPlatform, usePlatform, usePlatformInfo } from "./usePlatform";
export { COMPACT_QUERY, NARROW_QUERY, useCompact, useMediaQuery, useNarrow } from "./useMediaQuery";
export { setRecordingState, useCaptions, useLevels, useRecording } from "./useRecording";
export { reloadSettings, updateSettings, useSettings } from "./useSettings";
export { FOCUS_SEARCH_EVENT, openSearch, useGo } from "./useNav";
export { useRecordActions } from "./useRecordActions";
export { errorText, type Load, useLoad } from "./useLoad";
export { useDebounced } from "./useDebounced";
export { useAppearance } from "./useAppearance";
export { useFailureFix } from "./useFailureFix";
export { useShortcutStatus } from "./useShortcutStatus";
