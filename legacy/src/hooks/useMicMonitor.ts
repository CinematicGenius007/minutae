import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import { useEffect } from "react"
import { useRecordingStore } from "@/store/recording"

/**
 * Starts the CoreAudio mic monitor (Rust) and keeps the Zustand store in sync.
 *
 * IMPORTANT: We register Tauri event listeners BEFORE calling start_mic_monitor.
 * The monitor emits an immediate event if the mic is already in use when it
 * starts (e.g. Minutae opened mid-meeting), so the listeners must be ready first.
 */
export function useMicMonitor() {
	const setMicActive = useRecordingStore((s) => s.setMicActive)

	useEffect(() => {
		let cancelled = false
		let unlistenDetected: (() => void) | null = null
		let unlistenStopped: (() => void) | null = null

		async function setup() {
			// Register listeners FIRST — the monitor may emit immediately on start
			// if the mic was already in use (e.g. user opened Minutae mid-meeting).
			const [unDet, unStop] = await Promise.all([
				listen("mic:activity-detected", () => setMicActive(true)),
				listen("mic:activity-stopped", () => setMicActive(false)),
			])

			if (cancelled) {
				// Component unmounted before listeners were ready — clean up.
				unDet()
				unStop()
				return
			}

			unlistenDetected = unDet
			unlistenStopped = unStop

			// Only start the monitor once listeners are guaranteed to be registered.
			await invoke("start_mic_monitor").catch(console.error)
		}

		setup()

		return () => {
			cancelled = true
			invoke("stop_mic_monitor").catch(console.error)
			unlistenDetected?.()
			unlistenStopped?.()
		}
	}, [setMicActive])
}
