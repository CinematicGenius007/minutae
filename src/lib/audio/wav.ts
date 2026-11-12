const TARGET_SAMPLE_RATE = 16_000

/**
 * Decodes a recorded blob and returns a 16 kHz mono 16-bit WAV — the format
 * whisper.cpp expects, and ~6x smaller than the device-rate stereo capture.
 */
export async function convertBlobToWav(blob: Blob): Promise<Uint8Array> {
	const arrayBuffer = await blob.arrayBuffer()
	const audioContext = new AudioContext()

	try {
		const decoded = await audioContext.decodeAudioData(arrayBuffer.slice(0))
		const frames = Math.max(1, Math.ceil(decoded.duration * TARGET_SAMPLE_RATE))
		const offline = new OfflineAudioContext(1, frames, TARGET_SAMPLE_RATE)
		const source = offline.createBufferSource()
		source.buffer = decoded
		source.connect(offline.destination)
		source.start()
		return encodeAudioBufferAsWav(await offline.startRendering())
	} finally {
		await audioContext.close().catch(() => {})
	}
}

function encodeAudioBufferAsWav(audioBuffer: AudioBuffer): Uint8Array {
	const channelCount = audioBuffer.numberOfChannels
	const sampleRate = audioBuffer.sampleRate
	const frameCount = audioBuffer.length
	const bitsPerSample = 16
	const blockAlign = channelCount * (bitsPerSample / 8)
	const byteRate = sampleRate * blockAlign
	const dataSize = frameCount * blockAlign
	const buffer = new ArrayBuffer(44 + dataSize)
	const view = new DataView(buffer)

	writeAscii(view, 0, "RIFF")
	view.setUint32(4, 36 + dataSize, true)
	writeAscii(view, 8, "WAVE")
	writeAscii(view, 12, "fmt ")
	view.setUint32(16, 16, true)
	view.setUint16(20, 1, true)
	view.setUint16(22, channelCount, true)
	view.setUint32(24, sampleRate, true)
	view.setUint32(28, byteRate, true)
	view.setUint16(32, blockAlign, true)
	view.setUint16(34, bitsPerSample, true)
	writeAscii(view, 36, "data")
	view.setUint32(40, dataSize, true)

	const channels = Array.from({ length: channelCount }, (_, index) =>
		audioBuffer.getChannelData(index),
	)
	let offset = 44

	for (let frame = 0; frame < frameCount; frame++) {
		for (let channel = 0; channel < channelCount; channel++) {
			const sample = Math.max(-1, Math.min(1, channels[channel][frame] ?? 0))
			const pcm = sample < 0 ? sample * 0x8000 : sample * 0x7fff
			view.setInt16(offset, pcm, true)
			offset += 2
		}
	}

	return new Uint8Array(buffer)
}

function writeAscii(view: DataView, offset: number, value: string) {
	for (let index = 0; index < value.length; index++) {
		view.setUint8(offset + index, value.charCodeAt(index))
	}
}
