use crate::classifier::{classify_column, ColumnMeta};
use crate::suggester::{suggest_views, ViewSuggestion};
use duckdb::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedCSV {
    pub columns: Vec<ColumnMeta>,
    pub row_count: usize,
    pub sample_rows: Vec<Vec<String>>,
    pub suggestions: Vec<ViewSuggestion>,
}

/// Maximum rows to read for column classification. Beyond this, we only count rows.
const MAX_CLASSIFY_ROWS: usize = 1000;
const MAX_SAMPLE_ROWS: usize = 100;

#[tauri::command]
pub fn parse_csv(path: String) -> Result<ParsedCSV, String> {
    let mut rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .from_path(&path)
        .map_err(|e| format!("Failed to open CSV: {}", e))?;

    let headers: Vec<String> = rdr
        .headers()
        .map_err(|e| format!("Failed to read headers: {}", e))?
        .iter()
        .map(|h| h.to_string())
        .collect();

    let num_cols = headers.len();
    let mut columns_data: Vec<Vec<String>> = vec![vec![]; num_cols];
    let mut sample_rows: Vec<Vec<String>> = Vec::new();
    let mut row_count = 0;

    for result in rdr.records() {
        let record = result.map_err(|e| format!("Failed to read record: {}", e))?;

        if row_count < MAX_CLASSIFY_ROWS {
            // Parse fields for classification and sampling
            let row: Vec<String> = record.iter().map(|f| f.to_string()).collect();

            if row_count < MAX_SAMPLE_ROWS {
                sample_rows.push(row.clone());
            }

            for (i, field) in row.into_iter().enumerate() {
                if i < num_cols {
                    columns_data[i].push(field);
                }
            }
        }
        // After classification window, just count — don't parse fields
        row_count += 1;
    }

    let columns: Vec<ColumnMeta> = headers
        .into_iter()
        .zip(columns_data)
        .map(|(name, values)| classify_column(&name, &values))
        .collect();

    let suggestions = suggest_views(&columns);

    Ok(ParsedCSV {
        columns,
        row_count,
        sample_rows,
        suggestions,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentFile {
    pub path: String,
    pub title: String,
    pub last_opened: String,
}

fn recent_files_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let data_dir = app.path().app_data_dir().map_err(|e: tauri::Error| e.to_string())?;
    std::fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
    Ok(data_dir.join("recent_files.json"))
}

fn load_recent_files(app: &tauri::AppHandle) -> Vec<RecentFile> {
    let path = match recent_files_path(app) {
        Ok(p) => p,
        Err(_) => return vec![],
    };
    if !path.exists() {
        return vec![];
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    serde_json::from_str(&content).unwrap_or_default()
}

#[tauri::command]
pub fn get_recent_files(app: tauri::AppHandle) -> Result<Vec<RecentFile>, String> {
    Ok(load_recent_files(&app))
}

#[tauri::command]
pub fn add_recent_file(app: tauri::AppHandle, path: String, title: String) -> Result<(), String> {
    let file_path = recent_files_path(&app)?;
    let mut recents = load_recent_files(&app);
    recents.retain(|r| r.path != path);
    recents.insert(
        0,
        RecentFile {
            path,
            title,
            last_opened: chrono::Local::now().to_rfc3339(),
        },
    );
    recents.truncate(10);
    let content = serde_json::to_string_pretty(&recents).map_err(|e| e.to_string())?;
    std::fs::write(&file_path, content).map_err(|e| e.to_string())?;
    Ok(())
}

/// Managed DuckDB connection state.
pub type DuckDbState = Arc<Mutex<Connection>>;

/// Create a new in-memory DuckDB connection.
pub fn create_duckdb() -> Connection {
    Connection::open_in_memory().expect("Failed to create DuckDB connection")
}

#[tauri::command]
pub async fn load_csv_into_duckdb(path: String, app: tauri::AppHandle) -> Result<(), String> {
    let db = app.state::<DuckDbState>();
    let conn = db.lock().await;
    conn.execute("DROP TABLE IF EXISTS csv", [])
        .map_err(|e| format!("Failed to drop table: {}", e))?;
    conn.execute(
        &format!("CREATE TABLE csv AS SELECT * FROM read_csv_auto('{}', ignore_errors=true)", path.replace('\'', "''")),
        [],
    )
    .map_err(|e| format!("Failed to load CSV into DuckDB: {}", e))?;
    Ok(())
}

#[tauri::command]
pub async fn query_duckdb(
    sql: String,
    app: tauri::AppHandle,
) -> Result<Vec<HashMap<String, serde_json::Value>>, String> {
    let db = app.state::<DuckDbState>();
    let conn = db.lock().await;

    // Use Arrow interface to avoid the column_name panic in duckdb-rs
    let mut stmt = conn.prepare(&sql).map_err(|e| format!("SQL error: {}", e))?;
    let arrow_result = stmt.query_arrow([]).map_err(|e| format!("Query failed: {}", e))?;

    let batches: Vec<_> = arrow_result.collect();
    let mut result = Vec::new();

    for batch in &batches {
        let schema = batch.schema();
        let num_rows = batch.num_rows();
        let num_cols = batch.num_columns();

        for row_idx in 0..num_rows {
            let mut map = HashMap::new();
            for col_idx in 0..num_cols {
                let col_name = schema.field(col_idx).name().clone();
                let col = batch.column(col_idx);
                let val = arrow_col_to_json(col, row_idx);
                map.insert(col_name, val);
            }
            result.push(map);
        }
    }
    Ok(result)
}

/// Convert an Arrow array value at a given index to a JSON value.
fn arrow_col_to_json(col: &dyn duckdb::arrow::array::Array, idx: usize) -> serde_json::Value {
    use duckdb::arrow::array::*;
    use duckdb::arrow::datatypes::DataType;

    if col.is_null(idx) {
        return serde_json::Value::Null;
    }

    match col.data_type() {
        DataType::Boolean => {
            let arr = col.as_any().downcast_ref::<BooleanArray>().unwrap();
            serde_json::Value::Bool(arr.value(idx))
        }
        DataType::Int8 => serde_json::json!(col.as_any().downcast_ref::<Int8Array>().unwrap().value(idx)),
        DataType::Int16 => serde_json::json!(col.as_any().downcast_ref::<Int16Array>().unwrap().value(idx)),
        DataType::Int32 => serde_json::json!(col.as_any().downcast_ref::<Int32Array>().unwrap().value(idx)),
        DataType::Int64 => serde_json::json!(col.as_any().downcast_ref::<Int64Array>().unwrap().value(idx)),
        DataType::UInt8 => serde_json::json!(col.as_any().downcast_ref::<UInt8Array>().unwrap().value(idx)),
        DataType::UInt16 => serde_json::json!(col.as_any().downcast_ref::<UInt16Array>().unwrap().value(idx)),
        DataType::UInt32 => serde_json::json!(col.as_any().downcast_ref::<UInt32Array>().unwrap().value(idx)),
        DataType::UInt64 => serde_json::json!(col.as_any().downcast_ref::<UInt64Array>().unwrap().value(idx)),
        DataType::Float32 => serde_json::json!(col.as_any().downcast_ref::<Float32Array>().unwrap().value(idx)),
        DataType::Float64 => serde_json::json!(col.as_any().downcast_ref::<Float64Array>().unwrap().value(idx)),
        DataType::Utf8 => {
            let arr = col.as_any().downcast_ref::<StringArray>().unwrap();
            serde_json::Value::String(arr.value(idx).to_string())
        }
        DataType::LargeUtf8 => {
            let arr = col.as_any().downcast_ref::<LargeStringArray>().unwrap();
            serde_json::Value::String(arr.value(idx).to_string())
        }
        _ => serde_json::Value::String(format!("{:?}", col.data_type())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture_path(name: &str) -> String {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.pop();
        path.push("tests/fixtures");
        path.push(name);
        path.to_string_lossy().to_string()
    }

    #[test]
    fn test_parse_shopify() {
        let result = parse_csv(fixture_path("shopify.csv")).unwrap();
        assert_eq!(result.row_count, 8);
        assert_eq!(result.columns.len(), 7);
        assert_eq!(result.sample_rows.len(), 8);
        assert!(!result.suggestions.is_empty());
    }

    #[test]
    fn test_parse_marketing() {
        let result = parse_csv(fixture_path("marketing.csv")).unwrap();
        assert_eq!(result.row_count, 6);
        assert_eq!(result.columns.len(), 7);
    }

    #[test]
    fn test_parse_survey() {
        let result = parse_csv(fixture_path("survey.csv")).unwrap();
        assert_eq!(result.row_count, 8);
        assert_eq!(result.columns.len(), 5);
    }

    #[test]
    fn test_parse_nonexistent() {
        let result = parse_csv("/nonexistent/file.csv".to_string());
        assert!(result.is_err());
    }

    fn edge_path(name: &str) -> String {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.pop();
        path.push("tests/fixtures/edge-cases");
        path.push(name);
        path.to_string_lossy().to_string()
    }

    #[test]
    fn test_parse_missing_values() {
        let result = parse_csv(edge_path("missing_values.csv")).unwrap();
        assert_eq!(result.columns.len(), 4);
        assert!(result.row_count >= 6); // some rows have missing values
    }

    #[test]
    fn test_parse_single_column() {
        let result = parse_csv(edge_path("single_column.csv")).unwrap();
        assert_eq!(result.columns.len(), 1);
        assert_eq!(result.row_count, 5);
        // Should produce no suggestions (single Identifier column)
    }

    #[test]
    fn test_parse_single_row() {
        let result = parse_csv(edge_path("single_row.csv")).unwrap();
        assert_eq!(result.columns.len(), 3);
        assert_eq!(result.row_count, 1);
    }

    #[test]
    fn test_parse_many_columns() {
        let result = parse_csv(edge_path("many_columns.csv")).unwrap();
        assert_eq!(result.columns.len(), 25);
    }

    fn realworld_path(name: &str) -> String {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.pop();
        path.push("tests/fixtures/real-world");
        path.push(name);
        path.to_string_lossy().to_string()
    }

    #[test]
    fn test_parse_customers_10k() {
        let start = std::time::Instant::now();
        let result = parse_csv(realworld_path("customers-10000.csv")).unwrap();
        let elapsed = start.elapsed();
        eprintln!("=== customers-10000.csv ===");
        eprintln!("  rows: {}, cols: {}, parse time: {:?}", result.row_count, result.columns.len(), elapsed);
        for col in &result.columns {
            eprintln!("  {} → {:?} (unique: {})", col.name, col.col_type, col.unique_count);
        }
        eprintln!("  suggestions: {}", result.suggestions.len());
        for s in &result.suggestions {
            eprintln!("    {:?} metric={:?} group_by={:?} default={}", s.view_type, s.metric, s.group_by, s.shown_by_default);
        }
        assert!(elapsed.as_secs() < 5, "Too slow: {:?}", elapsed);
    }

    #[test]
    fn test_parse_uncleaned_jobs() {
        let start = std::time::Instant::now();
        let result = parse_csv(realworld_path("uncleaned_ds_jobs.csv")).unwrap();
        let elapsed = start.elapsed();
        eprintln!("=== uncleaned_ds_jobs.csv ===");
        eprintln!("  rows: {}, cols: {}, parse time: {:?}", result.row_count, result.columns.len(), elapsed);
        for col in &result.columns {
            eprintln!("  {} → {:?} (unique: {})", col.name, col.col_type, col.unique_count);
        }
        eprintln!("  suggestions: {}", result.suggestions.len());
        for s in &result.suggestions {
            eprintln!("    {:?} metric={:?} group_by={:?} default={}", s.view_type, s.metric, s.group_by, s.shown_by_default);
        }
        assert!(elapsed.as_secs() < 5, "Too slow: {:?}", elapsed);
    }

    #[test]
    fn test_parse_salary_survey() {
        let start = std::time::Instant::now();
        let result = parse_csv(realworld_path("salary_survey.csv")).unwrap();
        let elapsed = start.elapsed();
        eprintln!("=== salary_survey.csv ===");
        eprintln!("  rows: {}, cols: {}, parse time: {:?}", result.row_count, result.columns.len(), elapsed);
        for col in &result.columns {
            eprintln!("  {} → {:?} (unique: {})", col.name, col.col_type, col.unique_count);
        }
        eprintln!("  suggestions: {}", result.suggestions.len());
        for s in &result.suggestions {
            eprintln!("    {:?} metric={:?} group_by={:?} default={}", s.view_type, s.metric, s.group_by, s.shown_by_default);
        }
        assert!(elapsed.as_secs() < 10, "Too slow: {:?}", elapsed);
    }

    #[test]
    fn test_parse_chronic_disease() {
        let start = std::time::Instant::now();
        let result = parse_csv(realworld_path("chronic_disease.csv")).unwrap();
        let elapsed = start.elapsed();
        eprintln!("=== chronic_disease.csv ===");
        eprintln!("  rows: {}, cols: {}, parse time: {:?}", result.row_count, result.columns.len(), elapsed);
        for col in &result.columns {
            eprintln!("  {} → {:?} (unique: {})", col.name, col.col_type, col.unique_count);
        }
        eprintln!("  suggestions: {}", result.suggestions.len());
        for s in &result.suggestions {
            eprintln!("    {:?} metric={:?} group_by={:?} default={}", s.view_type, s.metric, s.group_by, s.shown_by_default);
        }
        assert!(elapsed.as_secs() < 30, "Too slow: {:?}", elapsed);
    }
}
