import { env, pipeline } from "@xenova/transformers"

// Disable worker proxy — run ONNX inference on the main thread.
// This avoids worker script path issues inside Tauri's WebView.
env.backends.onnx.wasm.proxy = false

// ─── Singleton pipeline ───────────────────────────────────────────────────────

type FeatureExtractionPipeline = Awaited<ReturnType<typeof pipeline<"feature-extraction">>>

let _pipe: FeatureExtractionPipeline | null = null
let _loading = false
const _callbacks: Array<(pipe: FeatureExtractionPipeline) => void> = []

/**
 * Lazily loads the all-MiniLM-L6-v2 model (downloaded from HuggingFace on
 * first use, then cached in the WebView's Cache Storage — persists across
 * app restarts).
 */
export async function getEmbedder(): Promise<FeatureExtractionPipeline> {
	if (_pipe) return _pipe

	// If already loading, queue until ready
	if (_loading) {
		return new Promise((resolve) => _callbacks.push(resolve))
	}

	_loading = true
	const pipe = await pipeline("feature-extraction", "Xenova/all-MiniLM-L6-v2")
	_pipe = pipe as FeatureExtractionPipeline
	_loading = false

	for (const cb of _callbacks.splice(0)) {
		cb(_pipe)
	}

	return _pipe
}

// ─── Core embed function ──────────────────────────────────────────────────────

/**
 * Returns a normalised 384-dim Float32Array for the given text.
 * Throws if the model hasn't been downloaded yet and there is no internet.
 */
export async function embed(text: string): Promise<Float32Array> {
	const pipe = await getEmbedder()
	const output = await pipe(text, { pooling: "mean", normalize: true })
	return output.data as Float32Array
}

// ─── Chunking ─────────────────────────────────────────────────────────────────

/**
 * Splits `text` into overlapping word-based chunks suitable for embedding.
 *
 * @param text        Raw transcript text
 * @param chunkWords  Target words per chunk (default 200)
 * @param overlapWords Words of overlap between chunks (default 20)
 */
export function chunkText(text: string, chunkWords = 200, overlapWords = 20): string[] {
	const words = text.split(/\s+/).filter(Boolean)
	if (words.length === 0) return []

	const chunks: string[] = []
	let i = 0

	while (i < words.length) {
		chunks.push(words.slice(i, i + chunkWords).join(" "))
		if (i + chunkWords >= words.length) break
		i += chunkWords - overlapWords
	}

	return chunks
}

// ─── Similarity ───────────────────────────────────────────────────────────────

/**
 * Cosine similarity between two equal-length numeric arrays.
 * Works on normalised vectors (dot product = cosine similarity) and
 * unnormalised vectors alike.
 */
export function cosineSimilarity(a: number[], b: number[]): number {
	let dot = 0
	let normA = 0
	let normB = 0
	for (let i = 0; i < a.length; i++) {
		dot += a[i] * b[i]
		normA += a[i] * a[i]
		normB += b[i] * b[i]
	}
	const denom = Math.sqrt(normA) * Math.sqrt(normB)
	return denom === 0 ? 0 : dot / denom
}
