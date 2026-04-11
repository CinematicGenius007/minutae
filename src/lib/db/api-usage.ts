import { desc, gte, sql } from "drizzle-orm"
import { getDb } from "./client"
import { apiUsageLog, type NewApiUsageLog } from "./schema"

export async function logApiUsage(data: NewApiUsageLog) {
	const db = await getDb()
	await db.insert(apiUsageLog).values(data)
}

export async function listApiUsage(limit = 200) {
	const db = await getDb()
	return db.select().from(apiUsageLog).orderBy(desc(apiUsageLog.createdAt)).limit(limit)
}

export async function getUsageSummary() {
	const db = await getDb()
	const thirtyDaysAgo = new Date(Date.now() - 30 * 24 * 60 * 60 * 1000).toISOString().slice(0, 10)

	const [totals] = await db
		.select({
			totalCostUsd: sql<number>`SUM(cost_usd)`,
			totalInputTokens: sql<number>`SUM(input_tokens)`,
			totalOutputTokens: sql<number>`SUM(output_tokens)`,
			count: sql<number>`COUNT(*)`,
		})
		.from(apiUsageLog)

	const [thisMonth] = await db
		.select({ costUsd: sql<number>`SUM(cost_usd)` })
		.from(apiUsageLog)
		.where(gte(apiUsageLog.createdAt, thirtyDaysAgo))

	return {
		totalCostUsd: totals?.totalCostUsd ?? 0,
		totalInputTokens: totals?.totalInputTokens ?? 0,
		totalOutputTokens: totals?.totalOutputTokens ?? 0,
		count: totals?.count ?? 0,
		thisMonthCostUsd: thisMonth?.costUsd ?? 0,
	}
}
