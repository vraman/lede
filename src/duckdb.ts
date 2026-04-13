import { invoke } from "@tauri-apps/api/core";

let loaded = false;

/** Load a CSV file into DuckDB (Rust backend). */
export async function loadCSV(path: string): Promise<void> {
  await invoke("load_csv_into_duckdb", { path });
  loaded = true;
}

/** Run a SQL query against the loaded CSV. */
export async function query(sql: string): Promise<Record<string, unknown>[]> {
  return await invoke("query_duckdb", { sql });
}

/** Check if a CSV has been loaded into DuckDB. */
export function isLoaded(): boolean {
  return loaded;
}
