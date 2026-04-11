// ─── Singleton pipeline ───────────────────────────────────────────────────────
//
// @xenova/transformers is imported DYNAMICALLY inside getEmbedder() so the
// ONNX Runtime never runs at module-load time.  Importing it statically at the
// top of the file triggers ort-web initialisation immediately, which crashes
// Vite's dev server (and causes a white screen in the app) before any UI
// has rendered.
//
// The lazy import means ONNX only loads the first time a user actually
// requests an embedding (i.e. triggers semantic search or a meeting finishes).

// biome-ignore lint/suspicious/noExplicitAny: pipeline type is resolved dynamically
type AnyPipeline = any

let _pipe: AnyPipeline | null = null
let _loading = false
const _callbacks: Array<(pipe: AnyPipeline) => void> = []

/**
 * Lazily loads `@xenova/transformers` + the all-MiniLM-L6-v2 model on first
 * call (downloaded from HuggingFace, then cached in Cache Storage permanently).
 *
 * Subsequent calls return the cached singleton immediately.
 * Concurrent callers while loading is in progress are queued and resolved
 * together once the model is ready.
 */
export async function getEmbedder(): Promise<AnyPipeline> {
	if (_pipe) return _pipe

	if (_loading) {
		return new Promise((resolve) => _callbacks.push(resolve))
	}

	_loading = true

	// Dynamic import — deferred until first use, never runs at module load time.
	const { pipeline, env } = await import("@xenova/transformers")

	// Run ONNX on the main thread — avoids worker script path issues in
	// Tauri's WebView (blob: worker URLs are blocked by some CSP setups).
	env.backends.onnx.wasm.proxy = false

	const pipe = await pipeline("feature-extraction", "Xenova/all-MiniLM-L6-v2")
	_pipe = pipe
	_loading = false

	for (const cb of _callbacks.splice(0)) {
		cb(_pipe)
	}

	return _pipe
}

// ─── Core embed function ──────────────────────────────────────────────────────

/**
 * Returns a normalised 384-dim Float32Array for the given text.
 * Throws if the model is not cached and there is no internet connection.
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
 * @param text         Raw transcript text
 * @param chunkWords   Target words per chunk (default 200)
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
 * Works on normalised (dot product = cosine) and unnormalised vectors alike.
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
