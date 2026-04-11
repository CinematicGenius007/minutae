import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import { useEffect } from "react"
import { useRecordingStore } from "@/store/recording"

export function useMicMonitor() {
	const setMicActive = useRecordingStore((s) => s.setMicActive)

	useEffect(() => {
		invoke("start_mic_monitor").catch(console.error)

		const unlistenDetected = listen("mic:activity-detected", () => {
			setMicActive(true)
		})

		const unlistenStopped = listen("mic:activity-stopped", () => {
			setMicActive(false)
		})

		return () => {
			invoke("stop_mic_monitor").catch(console.error)
			unlistenDetected.then((fn) => fn())
			unlistenStopped.then((fn) => fn())
		}
	}, [setMicActive])
}
