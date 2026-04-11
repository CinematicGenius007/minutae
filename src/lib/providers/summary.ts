import { z } from "zod"
import { logApiUsage } from "@/lib/db/api-usage"
import { SUMMARIZE_SYSTEM, SUMMARIZE_USER } from "@/lib/prompts/summarize"
import type { AppSettings } from "@/lib/settings"
import { nanoid } from "@/lib/utils"

// ─── Shared types ─────────────────────────────────────────────────────────────

export interface SummaryResult {
	title: string
	summary: string[]
	keyDecisions: string[]
	actionItems: string[]
	participants: string[]
}

export interface SummaryProvider {
	name: string
	summarize(transcript: string, meetingId: string): Promise<SummaryResult>
}

// ─── Zod schema for parsing AI response ──────────────────────────────────────

const SummarySchema = z.object({
	title: z.string().max(120),
	summary: z.array(z.string()),
	keyDecisions: z.array(z.string()),
	actionItems: z.array(z.string()),
	participants: z.array(z.string()),
})

function parseResult(raw: string): SummaryResult {
	const cleaned = raw
		.trim()
		.replace(/^```json\s*/i, "")
		.replace(/```$/i, "")
	const parsed = JSON.parse(cleaned)
	return SummarySchema.parse(parsed)
}

// ─── Gemini Flash ─────────────────────────────────────────────────────────────

export class GeminiProvider implements SummaryProvider {
	name = "gemini"
	private model = "gemini-2.0-flash"

	constructor(private apiKey: string) {}

	async summarize(transcript: string, meetingId: string): Promise<SummaryResult> {
		const { GoogleGenAI } = await import("@google/genai")
		const ai = new GoogleGenAI({ apiKey: this.apiKey })

		const result = await ai.models.generateContent({
			model: this.model,
			contents: [
				{ role: "user", parts: [{ text: `${SUMMARIZE_SYSTEM}\n\n${SUMMARIZE_USER(transcript)}` }] },
			],
		})

		const text = result.text ?? ""
		const inputTokens = result.usageMetadata?.promptTokenCount ?? 0
		const outputTokens = result.usageMetadata?.candidatesTokenCount ?? 0

		await logApiUsage({
			id: nanoid(),
			meetingId,
			provider: "gemini",
			model: this.model,
			endpoint: "generateContent",
			inputTokens,
			outputTokens,
			// Gemini Flash pricing: ~$0.075/1M input, $0.30/1M output
			costUsd: (inputTokens * 0.075 + outputTokens * 0.3) / 1_000_000,
			createdAt: new Date().toISOString(),
		})

		return parseResult(text)
	}
}

// ─── Claude ───────────────────────────────────────────────────────────────────

export class ClaudeProvider implements SummaryProvider {
	name = "claude"
	private model = "claude-haiku-4-5-20251001"

	constructor(private apiKey: string) {}

	async summarize(transcript: string, meetingId: string): Promise<SummaryResult> {
		const Anthropic = (await import("@anthropic-ai/sdk")).default
		const client = new Anthropic({ apiKey: this.apiKey, dangerouslyAllowBrowser: true })

		const message = await client.messages.create({
			model: this.model,
			max_tokens: 1024,
			system: SUMMARIZE_SYSTEM,
			messages: [{ role: "user", content: SUMMARIZE_USER(transcript) }],
		})

		const text = message.content[0]?.type === "text" ? message.content[0].text : ""
		const inputTokens = message.usage.input_tokens
		const outputTokens = message.usage.output_tokens

		await logApiUsage({
			id: nanoid(),
			meetingId,
			provider: "claude",
			model: this.model,
			endpoint: "messages",
			inputTokens,
			outputTokens,
			// Claude Haiku pricing: $0.80/1M input, $4.00/1M output
			costUsd: (inputTokens * 0.8 + outputTokens * 4.0) / 1_000_000,
			createdAt: new Date().toISOString(),
		})

		return parseResult(text)
	}
}

// ─── OpenAI ───────────────────────────────────────────────────────────────────

export class OpenAIProvider implements SummaryProvider {
	name = "openai"
	private model = "gpt-4o-mini"

	constructor(private apiKey: string) {}

	async summarize(transcript: string, meetingId: string): Promise<SummaryResult> {
		const OpenAI = (await import("openai")).default
		const client = new OpenAI({ apiKey: this.apiKey, dangerouslyAllowBrowser: true })

		const res = await client.chat.completions.create({
			model: this.model,
			messages: [
				{ role: "system", content: SUMMARIZE_SYSTEM },
				{ role: "user", content: SUMMARIZE_USER(transcript) },
			],
		})

		const text = res.choices[0]?.message?.content ?? ""
		const inputTokens = res.usage?.prompt_tokens ?? 0
		const outputTokens = res.usage?.completion_tokens ?? 0

		await logApiUsage({
			id: nanoid(),
			meetingId,
			provider: "openai",
			model: this.model,
			endpoint: "chat/completions",
			inputTokens,
			outputTokens,
			// GPT-4o mini pricing: $0.15/1M input, $0.60/1M output
			costUsd: (inputTokens * 0.15 + outputTokens * 0.6) / 1_000_000,
			createdAt: new Date().toISOString(),
		})

		return parseResult(text)
	}
}

// ─── Factory ──────────────────────────────────────────────────────────────────

export function getSummaryProvider(
	settings: Pick<
		AppSettings,
		"summaryProvider" | "geminiApiKey" | "anthropicApiKey" | "summaryOpenaiApiKey"
	>,
): SummaryProvider | null {
	switch (settings.summaryProvider) {
		case "gemini":
			return settings.geminiApiKey ? new GeminiProvider(settings.geminiApiKey) : null
		case "claude":
			return settings.anthropicApiKey ? new ClaudeProvider(settings.anthropicApiKey) : null
		case "openai":
			return settings.summaryOpenaiApiKey ? new OpenAIProvider(settings.summaryOpenaiApiKey) : null
		default:
			return null
	}
}
