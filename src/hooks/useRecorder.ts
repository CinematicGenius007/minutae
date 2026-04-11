import { useCallback, useEffect, useRef, useState } from "react"

export type RecorderState = "idle" | "recording" | "paused" | "stopped"

interface UseRecorderReturn {
	state: RecorderState
	audioLevel: number // 0–1 RMS value
	start: () => Promise<void>
	pause: () => void
	resume: () => void
	stop: () => Promise<Blob>
}

export function useRecorder(): UseRecorderReturn {
	const [state, setState] = useState<RecorderState>("idle")
	const [audioLevel, setAudioLevel] = useState(0)

	const mediaRecorderRef = useRef<MediaRecorder | null>(null)
	const chunksRef = useRef<Blob[]>([])
	const resolveStopRef = useRef<((blob: Blob) => void) | null>(null)
	const analyserRef = useRef<AnalyserNode | null>(null)
	const animFrameRef = useRef<number | null>(null)
	const streamRef = useRef<MediaStream | null>(null)

	// Stable refs so useCallback deps stay empty
	const startLevelMeter = useRef((stream: MediaStream) => {
		const ctx = new AudioContext()
		const source = ctx.createMediaStreamSource(stream)
		const analyser = ctx.createAnalyser()
		analyser.fftSize = 256
		source.connect(analyser)
		analyserRef.current = analyser

		const data = new Uint8Array(analyser.frequencyBinCount)

		function tick() {
			analyser.getByteTimeDomainData(data)
			let sum = 0
			for (const v of data) {
				const normalized = (v - 128) / 128
				sum += normalized * normalized
			}
			setAudioLevel(Math.sqrt(sum / data.length))
			animFrameRef.current = requestAnimationFrame(tick)
		}
		animFrameRef.current = requestAnimationFrame(tick)
	})

	const stopLevelMeter = useRef(() => {
		if (animFrameRef.current) {
			cancelAnimationFrame(animFrameRef.current)
			animFrameRef.current = null
		}
		setAudioLevel(0)
	})

	const start = useCallback(async () => {
		const stream = await navigator.mediaDevices.getUserMedia({ audio: true })
		streamRef.current = stream
		chunksRef.current = []

		const mimeType = MediaRecorder.isTypeSupported("audio/webm;codecs=opus")
			? "audio/webm;codecs=opus"
			: "audio/webm"

		const mr = new MediaRecorder(stream, { mimeType })
		mediaRecorderRef.current = mr

		mr.ondataavailable = (e) => {
			if (e.data.size > 0) chunksRef.current.push(e.data)
		}

		mr.onstop = () => {
			const blob = new Blob(chunksRef.current, { type: mimeType })
			resolveStopRef.current?.(blob)
			resolveStopRef.current = null
			for (const t of stream.getTracks()) t.stop()
			stopLevelMeter.current()
		}

		mr.start(1000) // collect chunks every 1s
		setState("recording")
		startLevelMeter.current(stream)
	}, [])

	const pause = useCallback(() => {
		mediaRecorderRef.current?.pause()
		setState("paused")
	}, [])

	const resume = useCallback(() => {
		mediaRecorderRef.current?.resume()
		setState("recording")
	}, [])

	const stop = useCallback((): Promise<Blob> => {
		return new Promise((resolve) => {
			resolveStopRef.current = resolve
			mediaRecorderRef.current?.stop()
			setState("stopped")
		})
	}, [])

	// Cleanup on unmount
	useEffect(() => {
		return () => {
			stopLevelMeter.current()
			if (streamRef.current) {
				for (const t of streamRef.current.getTracks()) t.stop()
			}
		}
	}, [])

	return { state, audioLevel, start, pause, resume, stop }
}
