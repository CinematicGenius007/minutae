import { useEffect, useState } from "react"
import { getUsageSummary, listApiUsage } from "@/lib/db/api-usage"
import type { ApiUsageLog } from "@/lib/db/schema"
import { formatDateTime } from "@/lib/utils"

interface UsageSummary {
	totalCostUsd: number
	thisMonthCostUsd: number
	totalInputTokens: number
	totalOutputTokens: number
	count: number
}

export function UsagePage() {
	const [summary, setSummary] = useState<UsageSummary | null>(null)
	const [logs, setLogs] = useState<ApiUsageLog[]>([])
	const [loading, setLoading] = useState(true)

	useEffect(() => {
		async function load() {
			const [s, l] = await Promise.all([getUsageSummary(), listApiUsage(100)])
			setSummary(s)
			setLogs(l)
			setLoading(false)
		}
		load()
	}, [])

	if (loading) {
		return (
			<div className="flex h-full items-center justify-center">
				<span className="text-sm text-[var(--color-text-muted)]">Loading…</span>
			</div>
		)
	}

	const totalTokens = (summary?.totalInputTokens ?? 0) + (summary?.totalOutputTokens ?? 0)

	return (
		<div className="flex flex-col h-full">
			{/* Header */}
			<div className="px-6 pt-5 pb-4 border-b border-[var(--color-border)]">
				<h1 className="text-base font-semibold text-[var(--color-text)]">Usage</h1>
				<p className="text-xs text-[var(--color-text-muted)] mt-0.5">
					Estimated costs based on public API pricing.
				</p>
			</div>

			<div className="flex-1 overflow-y-auto px-6 py-5 space-y-6">
				{/* Summary cards */}
				<div className="grid grid-cols-2 gap-3">
					<StatCard
						label="Total spend"
						value={`$${(summary?.totalCostUsd ?? 0).toFixed(4)}`}
						sub="all time"
					/>
					<StatCard
						label="This month"
						value={`$${(summary?.thisMonthCostUsd ?? 0).toFixed(4)}`}
						sub="last 30 days"
					/>
					<StatCard label="API calls" value={String(summary?.count ?? 0)} sub="total requests" />
					<StatCard
						label="Total tokens"
						value={totalTokens > 1000 ? `${(totalTokens / 1000).toFixed(1)}k` : String(totalTokens)}
						sub={`${((summary?.totalInputTokens ?? 0) / 1000).toFixed(1)}k in · ${((summary?.totalOutputTokens ?? 0) / 1000).toFixed(1)}k out`}
					/>
				</div>

				{/* Log table */}
				{logs.length === 0 ? (
					<div className="flex flex-col items-center justify-center py-16 gap-2">
						<p className="text-sm text-[var(--color-text-muted)]">No API usage recorded yet.</p>
						<p className="text-xs text-[var(--color-text-muted)]">
							Summarize a meeting to see costs here.
						</p>
					</div>
				) : (
					<div>
						<h2 className="text-xs font-semibold uppercase tracking-wider text-[var(--color-text-muted)] mb-3">
							Recent calls
						</h2>
						<div className="rounded-[var(--radius-md)] border border-[var(--color-border)] overflow-hidden">
							<table className="w-full text-xs">
								<thead>
									<tr className="border-b border-[var(--color-border)] bg-[var(--color-surface)]">
										<th className="text-left px-3 py-2 text-[var(--color-text-muted)] font-medium">
											Date
										</th>
										<th className="text-left px-3 py-2 text-[var(--color-text-muted)] font-medium">
											Provider
										</th>
										<th className="text-left px-3 py-2 text-[var(--color-text-muted)] font-medium">
											Model
										</th>
										<th className="text-right px-3 py-2 text-[var(--color-text-muted)] font-medium">
											In
										</th>
										<th className="text-right px-3 py-2 text-[var(--color-text-muted)] font-medium">
											Out
										</th>
										<th className="text-right px-3 py-2 text-[var(--color-text-muted)] font-medium">
											Cost
										</th>
									</tr>
								</thead>
								<tbody className="divide-y divide-[var(--color-border)]">
									{logs.map((log) => (
										<tr key={log.id} className="hover:bg-[var(--color-surface)] transition-colors">
											<td className="px-3 py-2 text-[var(--color-text-muted)] whitespace-nowrap">
												{formatDateTime(log.createdAt)}
											</td>
											<td className="px-3 py-2">
												<ProviderBadge provider={log.provider} />
											</td>
											<td className="px-3 py-2 text-[var(--color-text-muted)] font-mono">
												{log.model}
											</td>
											<td className="px-3 py-2 text-right text-[var(--color-text-muted)] tabular-nums">
												{log.inputTokens.toLocaleString()}
											</td>
											<td className="px-3 py-2 text-right text-[var(--color-text-muted)] tabular-nums">
												{log.outputTokens.toLocaleString()}
											</td>
											<td className="px-3 py-2 text-right text-[var(--color-text)] tabular-nums font-medium">
												${log.costUsd.toFixed(5)}
											</td>
										</tr>
									))}
								</tbody>
							</table>
						</div>
					</div>
				)}
			</div>
		</div>
	)
}

function StatCard({ label, value, sub }: { label: string; value: string; sub: string }) {
	return (
		<div className="bg-[var(--color-surface)] rounded-[var(--radius-md)] px-4 py-3 border border-[var(--color-border)]">
			<p className="text-xs text-[var(--color-text-muted)]">{label}</p>
			<p className="text-xl font-semibold text-[var(--color-text)] mt-1 tabular-nums">{value}</p>
			<p className="text-[10px] text-[var(--color-text-muted)] mt-0.5">{sub}</p>
		</div>
	)
}

function ProviderBadge({ provider }: { provider: string }) {
	const colors: Record<string, string> = {
		gemini: "text-blue-400  bg-blue-400/10",
		claude: "text-orange-400 bg-orange-400/10",
		openai: "text-emerald-400 bg-emerald-400/10",
	}
	const cls = colors[provider] ?? "text-[var(--color-text-muted)] bg-[var(--color-surface-raised)]"
	return (
		<span className={`inline-flex px-1.5 py-0.5 rounded text-[10px] font-medium capitalize ${cls}`}>
			{provider}
		</span>
	)
}
