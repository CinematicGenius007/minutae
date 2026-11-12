import { invoke } from "@tauri-apps/api/core"
import type { Meeting } from "@/lib/db/schema"
import type { SummaryResult } from "@/lib/providers/summary"
import { formatDurationLong, slugify } from "@/lib/utils"

/** The readable body (no frontmatter). This is what the Summary tab renders. */
export function buildSummaryBody(result: SummaryResult): string {
	const lines: string[] = ["## Summary", ...result.summary.map((b) => `- ${b}`)]

	if (result.keyDecisions.length > 0) {
		lines.push("", "## Key Decisions", ...result.keyDecisions.map((d) => `- ${d}`))
	}

	if (result.actionItems.length > 0) {
		lines.push("", "## Action Items", ...result.actionItems.map((a) => `- [ ] ${a}`))
	}

	return `${lines.join("\n")}\n`
}

export function buildSummaryMarkdown(
	meeting: Meeting,
	result: SummaryResult,
	recordingFilename: string,
): string {
	const duration = meeting.durationSeconds ? formatDurationLong(meeting.durationSeconds) : "unknown"

	const lines: string[] = [
		"---",
		`id: ${meeting.id}`,
		`title: "${result.title.replace(/"/g, '\\"')}"`,
		`date: ${meeting.startedAt}`,
		`duration: ${duration}`,
		`participants: [${result.participants.join(", ")}]`,
		`tags: []`,
		`recording: ../recordings/${recordingFilename}`,
		"---",
		"",
	]

	return `${lines.join("\n")}\n${buildSummaryBody(result)}`
}

export async function writeSummaryFile(
	meeting: Meeting,
	result: SummaryResult,
	recordingFilename: string,
): Promise<string> {
	const date = meeting.startedAt.slice(0, 10)
	const slug = slugify(result.title || "meeting")
	const filename = `${date}-${slug}.md`
	const content = buildSummaryMarkdown(meeting, result, recordingFilename)

	return invoke<string>("write_summary_file", { filename, content })
}
