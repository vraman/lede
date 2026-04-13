import * as duckdb from "@duckdb/duckdb-wasm";
import { readTextFile } from "@tauri-apps/plugin-fs";

let db: duckdb.AsyncDuckDB | null = null;
let conn: duckdb.AsyncDuckDBConnection | null = null;
let initPromise: Promise<void> | null = null;
let loadedCsvPath: string | null = null;

/** Initialize DuckDB-WASM (call once, idempotent). */
async function init(): Promise<void> {
  if (db) return;
  if (initPromise) return initPromise;

  initPromise = (async () => {
    const DUCKDB_BUNDLES = await duckdb.selectBundle({
      mvp: {
        mainModule: new URL("@duckdb/duckdb-wasm/dist/duckdb-mvp.wasm", import.meta.url).href,
        mainWorker: new URL("@duckdb/duckdb-wasm/dist/duckdb-browser-mvp.worker.js", import.meta.url).href,
      },
      eh: {
        mainModule: new URL("@duckdb/duckdb-wasm/dist/duckdb-eh.wasm", import.meta.url).href,
        mainWorker: new URL("@duckdb/duckdb-wasm/dist/duckdb-browser-eh.worker.js", import.meta.url).href,
      },
    });

    const logger = new duckdb.ConsoleLogger();
    const worker = new Worker(DUCKDB_BUNDLES.mainWorker!);
    db = new duckdb.AsyncDuckDB(logger, worker);
    await db.instantiate(DUCKDB_BUNDLES.mainModule);
    conn = await db.connect();
  })();

  return initPromise;
}

/** Load a CSV file into DuckDB as the "csv" table. */
export async function loadCSV(path: string): Promise<void> {
  if (loadedCsvPath === path) return; // already loaded

  await init();
  if (!db || !conn) throw new Error("DuckDB not initialized");

  // Read CSV content via Tauri FS
  const csvContent = await readTextFile(path);

  // Drop old table if exists
  await conn.query("DROP TABLE IF EXISTS csv");

  // Register CSV content and create table
  await db.registerFileText("data.csv", csvContent);
  await conn.query("CREATE TABLE csv AS SELECT * FROM read_csv_auto('data.csv')");

  loadedCsvPath = path;
}

/** Run a SQL query and return rows as objects. */
export async function query(sql: string): Promise<Record<string, unknown>[]> {
  await init();
  if (!conn) throw new Error("DuckDB not initialized");

  const result = await conn.query(sql);
  const rows: Record<string, unknown>[] = [];
  const schema = result.schema.fields;

  for (let i = 0; i < result.numRows; i++) {
    const row: Record<string, unknown> = {};
    for (let j = 0; j < schema.length; j++) {
      const col = result.getChildAt(j);
      row[schema[j].name] = col?.get(i);
    }
    rows.push(row);
  }

  return rows;
}

/** Check if DuckDB is ready and CSV is loaded. */
export function isLoaded(): boolean {
  return loadedCsvPath !== null;
}
