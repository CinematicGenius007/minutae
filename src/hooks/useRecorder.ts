import { useEffect, useState } from "react"

export type RecorderState = "idle" | "recording" | "paused" | "stopped"

interface UseRecorderReturn {
	state: RecorderState
	audioLevel: number
	start: () => Promise<void>
	pause: () => void
	resume: () => void
	stop: () => Promise<Blob>
	debugState: () => { state: RecorderState; mediaRecorderState: string | null; hasStream: boolean }
}

type Snapshot = {
	state: RecorderState
	audioLevel: number
}

let snapshot: Snapshot = {
	state: "idle",
	audioLevel: 0,
}

const listeners = new Set<(next: Snapshot) => void>()

let mediaRecorder: MediaRecorder | null = null
let stream: MediaStream | null = null
let chunks: Blob[] = []
let resolveStop: ((blob: Blob) => void) | null = null
let animFrame: number | null = null
let audioContext: AudioContext | null = null

function logRecorder(event: string, details?: Record<string, unknown>) {
	console.debug("[recorder]", event, {
		state: mediaRecorder?.state ?? null,
		hasRecorder: Boolean(mediaRecorder),
		hasStream: Boolean(stream),
		...details,
	})
}

function emit(next: Partial<Snapshot>) {
	snapshot = { ...snapshot, ...next }
	for (const listener of listeners) listener(snapshot)
}

function cleanupLevelMeter() {
	if (animFrame != null) {
		cancelAnimationFrame(animFrame)
		animFrame = null
	}
	if (audioContext) {
		void audioContext.close().catch(() => {})
		audioContext = null
	}
	emit({ audioLevel: 0 })
}

function startLevelMeter(nextStream: MediaStream) {
	audioContext = new AudioContext()
	const source = audioContext.createMediaStreamSource(nextStream)
	const analyser = audioContext.createAnalyser()
	analyser.fftSize = 256
	source.connect(analyser)

	const data = new Uint8Array(analyser.frequencyBinCount)

	function tick() {
		analyser.getByteTimeDomainData(data)
		let sum = 0
		for (const value of data) {
			const normalized = (value - 128) / 128
			sum += normalized * normalized
		}
		emit({ audioLevel: Math.sqrt(sum / data.length) })
		animFrame = requestAnimationFrame(tick)
	}

	animFrame = requestAnimationFrame(tick)
}

async function startRecording() {
	logRecorder("start:requested")
	if (mediaRecorder && mediaRecorder.state !== "inactive") return

	if (!navigator.mediaDevices?.getUserMedia) {
		logRecorder("start:unsupported-media-devices")
		throw new Error("Microphone capture is unavailable in this app context")
	}

	stream = await navigator.mediaDevices.getUserMedia({ audio: true })
	chunks = []

	const mimeType = MediaRecorder.isTypeSupported("audio/webm;codecs=opus")
		? "audio/webm;codecs=opus"
		: "audio/webm"

	const recorder = new MediaRecorder(stream, { mimeType })
	mediaRecorder = recorder

	recorder.ondataavailable = (event) => {
		if (event.data.size > 0) chunks.push(event.data)
	}

	recorder.onstop = () => {
		logRecorder("stop:event", { chunkCount: chunks.length })
		const blob = new Blob(chunks, { type: mimeType })
		resolveStop?.(blob)
		resolveStop = null

		if (stream) {
			for (const track of stream.getTracks()) track.stop()
		}
		stream = null
		mediaRecorder = null
		cleanupLevelMeter()
		emit({ state: "stopped" })
	}

	recorder.start(1000)
	startLevelMeter(stream)
	emit({ state: "recording" })
	logRecorder("start:completed")
}

function pauseRecording() {
	logRecorder("pause:requested")
	mediaRecorder?.pause()
	emit({ state: "paused" })
	logRecorder("pause:completed")
}

function resumeRecording() {
	logRecorder("resume:requested")
	mediaRecorder?.resume()
	emit({ state: "recording" })
	logRecorder("resume:completed")
}

function stopRecording(): Promise<Blob> {
	logRecorder("stop:requested")
	if (!mediaRecorder || mediaRecorder.state === "inactive") {
		logRecorder("stop:rejected")
		return Promise.reject(new Error("No active recording to stop"))
	}

	return new Promise((resolve) => {
		resolveStop = resolve
		mediaRecorder?.stop()
		logRecorder("stop:signal-sent")
	})
}

function getDebugState() {
	return {
		state: snapshot.state,
		mediaRecorderState: mediaRecorder?.state ?? null,
		hasStream: Boolean(stream),
	}
}

export function useRecorder(): UseRecorderReturn {
	const [localSnapshot, setLocalSnapshot] = useState(snapshot)

	useEffect(() => {
		listeners.add(setLocalSnapshot)
		return () => {
			listeners.delete(setLocalSnapshot)
		}
	}, [])

	return {
		state: localSnapshot.state,
		audioLevel: localSnapshot.audioLevel,
		start: startRecording,
		pause: pauseRecording,
		resume: resumeRecording,
		stop: stopRecording,
		debugState: getDebugState,
	}
}
