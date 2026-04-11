/** Generates a short random ID (URL-safe, 12 chars). */
export function nanoid(size = 12): string {
	const chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789"
	let id = ""
	const bytes = crypto.getRandomValues(new Uint8Array(size))
	for (const byte of bytes) {
		id += chars[byte % chars.length]
	}
	return id
}

/** Format seconds as MM:SS */
export function formatDuration(seconds: number): string {
	const m = Math.floor(seconds / 60)
	const s = seconds % 60
	return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`
}

/** Format seconds as "Xh Ym" or "Ym Zs" */
export function formatDurationLong(seconds: number): string {
	if (seconds < 60) return `${seconds}s`
	const h = Math.floor(seconds / 3600)
	const m = Math.floor((seconds % 3600) / 60)
	const s = seconds % 60
	if (h > 0) return `${h}h ${m}m`
	return `${m}m ${s}s`
}

/** Format an ISO date string as "Apr 11, 2026" */
export function formatDate(iso: string): string {
	return new Date(iso).toLocaleDateString("en-US", {
		month: "short",
		day: "numeric",
		year: "numeric",
	})
}

/** Format an ISO date string as "Apr 11, 2026 at 2:30 PM" */
export function formatDateTime(iso: string): string {
	return new Date(iso).toLocaleString("en-US", {
		month: "short",
		day: "numeric",
		year: "numeric",
		hour: "numeric",
		minute: "2-digit",
	})
}

/** Slugify a string for filenames */
export function slugify(text: string): string {
	return text
		.toLowerCase()
		.replace(/[^a-z0-9]+/g, "-")
		.replace(/^-|-$/g, "")
		.slice(0, 60)
}
