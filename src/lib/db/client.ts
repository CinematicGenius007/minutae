import Database from "@tauri-apps/plugin-sql"
import { drizzle } from "drizzle-orm/sqlite-proxy"
import * as schema from "./schema"

let _sqlite: InstanceType<typeof Database> | null = null
let _db: ReturnType<typeof drizzle> | null = null

/** Returns the raw tauri-plugin-sql Database instance (for FTS5 / raw SQL queries). */
export async function getSqlite(): Promise<InstanceType<typeof Database>> {
	if (_sqlite) return _sqlite
	_sqlite = await Database.load("sqlite:minutae.db")
	await _sqlite.execute("PRAGMA foreign_keys = ON")
	return _sqlite
}

/** Returns the Drizzle ORM db instance (for typed queries via the schema). */
export async function getDb() {
	if (_db) return _db

	const sqlite = await getSqlite()

	_db = drizzle(
		async (sql, params, method) => {
			if (method === "run") {
				await sqlite.execute(sql, params as unknown[])
				return { rows: [] }
			}
			const rows = await sqlite.select<Record<string, unknown>[]>(sql, params as unknown[])
			// drizzle sqlite-proxy expects rows as arrays of values
			return { rows: rows.map(Object.values) }
		},
		{ schema, logger: import.meta.env.DEV },
	)

	return _db
}
