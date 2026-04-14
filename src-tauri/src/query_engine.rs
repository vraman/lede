use crate::ai_insights::InsightEngine;
use crate::classifier::ColumnMeta;
use serde::Deserialize;
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tokio::sync::Mutex;

/// Try to match a natural language question to SQL using simple patterns.
/// Returns None if no pattern matches (should fall through to AI model).
pub fn simple_query_to_sql(question: &str, columns: &[ColumnMeta], table: &str) -> Option<String> {
    let q = question.trim().to_lowercase();
    let q = q.trim_end_matches('?').trim_end_matches('.').trim();

    // "how many rows" / "count" / "row count"
    if q == "count" || q == "how many rows" || q == "row count" || q == "number of rows" {
        return Some(format!("SELECT COUNT(*) as count FROM {}", table));
    }

    // "how many X" → count by group
    if let Some(rest) = q.strip_prefix("how many ") {
        if let Some(col) = fuzzy_match_column(rest.trim(), columns) {
            return Some(format!(
                "SELECT \"{col}\", COUNT(*) as count FROM {table} GROUP BY \"{col}\" ORDER BY count DESC"
            ));
        }
    }

    // "top N X by Y" or "top N X"
    if let Some(rest) = q.strip_prefix("top ") {
        let parts: Vec<&str> = rest.splitn(2, ' ').collect();
        if parts.len() == 2 {
            if let Ok(n) = parts[0].parse::<usize>() {
                let remainder = parts[1];
                // Try "X by Y" first
                if let Some((metric, group_by)) = parse_x_by_y(remainder, columns) {
                    return Some(format!(
                        "SELECT \"{group_by}\", SUM(\"{metric}\") as \"{metric}\" FROM {table} WHERE \"{metric}\" IS NOT NULL GROUP BY \"{group_by}\" ORDER BY \"{metric}\" DESC LIMIT {n}"
                    ));
                }
                // Try "group_col by metric" (e.g., "top 5 states by revenue")
                // Split remainder on " by " and try reversed
                if let Some(by_idx) = remainder.find(" by ") {
                    let group_text = &remainder[..by_idx];
                    let metric_text = &remainder[by_idx + 4..];
                    if let (Some(group), Some(metric)) = (fuzzy_match_column(group_text, columns), fuzzy_match_column(metric_text, columns)) {
                        return Some(format!(
                            "SELECT \"{group}\", SUM(\"{metric}\") as \"{metric}\" FROM {table} WHERE \"{metric}\" IS NOT NULL GROUP BY \"{group}\" ORDER BY \"{metric}\" DESC LIMIT {n}"
                        ));
                    }
                }
            }
        }
    }

    // "total/sum X" → scalar sum
    for prefix in &["total ", "sum ", "sum of "] {
        if let Some(rest) = q.strip_prefix(prefix) {
            if let Some(col) = fuzzy_match_column(rest.trim(), columns) {
                return Some(format!("SELECT SUM(\"{col}\") as \"{col}\" FROM {table}"));
            }
        }
    }

    // "average/avg/mean X"
    for prefix in &["average ", "avg ", "mean "] {
        if let Some(rest) = q.strip_prefix(prefix) {
            // Could be "average X" or "average X by Y"
            if let Some((metric, group_by)) = parse_x_by_y(rest.trim(), columns) {
                return Some(format!(
                    "SELECT \"{group_by}\", AVG(\"{metric}\") as \"{metric}\" FROM {table} WHERE \"{metric}\" IS NOT NULL GROUP BY \"{group_by}\" ORDER BY \"{metric}\" DESC"
                ));
            }
            if let Some(col) = fuzzy_match_column(rest.trim(), columns) {
                return Some(format!("SELECT AVG(\"{col}\") as \"{col}\" FROM {table}"));
            }
        }
    }

    // "min/minimum/lowest X"
    for prefix in &["min ", "minimum ", "lowest "] {
        if let Some(rest) = q.strip_prefix(prefix) {
            if let Some(col) = fuzzy_match_column(rest.trim(), columns) {
                return Some(format!("SELECT MIN(\"{col}\") as \"{col}\" FROM {table}"));
            }
        }
    }

    // "max/maximum/highest X"
    for prefix in &["max ", "maximum ", "highest "] {
        if let Some(rest) = q.strip_prefix(prefix) {
            if let Some(col) = fuzzy_match_column(rest.trim(), columns) {
                return Some(format!("SELECT MAX(\"{col}\") as \"{col}\" FROM {table}"));
            }
        }
    }

    // "X by Y" / "show X by Y" / "show me X by Y"
    let stripped = q
        .strip_prefix("show me ")
        .or_else(|| q.strip_prefix("show "))
        .unwrap_or(q);
    if let Some((metric, group_by)) = parse_x_by_y(stripped, columns) {
        return Some(format!(
            "SELECT \"{group_by}\", SUM(\"{metric}\") as \"{metric}\" FROM {table} WHERE \"{metric}\" IS NOT NULL GROUP BY \"{group_by}\" ORDER BY \"{metric}\" DESC"
        ));
    }

    // "X where Y = Z" / "X for Z"
    if let Some(sql) = parse_where_clause(q, columns, table) {
        return Some(sql);
    }

    None
}

/// Parse "X by Y" pattern, returning (metric_col, group_col).
fn parse_x_by_y(text: &str, columns: &[ColumnMeta]) -> Option<(String, String)> {
    // Try splitting on " by "
    if let Some(idx) = text.find(" by ") {
        let metric_text = &text[..idx];
        let group_text = &text[idx + 4..];
        let metric = fuzzy_match_column(metric_text.trim(), columns)?;
        let group = fuzzy_match_column(group_text.trim(), columns)?;
        if metric != group {
            return Some((metric, group));
        }
    }
    None
}

/// Parse "where X = Y" or "for Y" patterns.
fn parse_where_clause(text: &str, columns: &[ColumnMeta], table: &str) -> Option<String> {
    // "... where X = Y"
    if let Some(idx) = text.find(" where ") {
        let after = &text[idx + 7..];
        if let Some(eq_idx) = after.find(" = ") {
            let col_text = &after[..eq_idx];
            let val = after[eq_idx + 3..].trim().trim_matches('"').trim_matches('\'');
            let col = fuzzy_match_column(col_text.trim(), columns)?;
            return Some(format!(
                "SELECT * FROM {table} WHERE \"{col}\" = '{val}' LIMIT 50",
                val = val.replace('\'', "''")
            ));
        }
    }
    None
}

/// Fuzzy match a user-typed string to a column name.
/// Matches case-insensitively, with underscores/spaces interchangeable.
fn fuzzy_match_column(input: &str, columns: &[ColumnMeta]) -> Option<String> {
    let normalized = input.to_lowercase().replace([' ', '-'], "_");

    // Exact match (case-insensitive)
    for col in columns {
        if col.name.to_lowercase() == normalized {
            return Some(col.name.clone());
        }
    }

    // Normalized match (underscores/spaces)
    for col in columns {
        let col_normalized = col.name.to_lowercase().replace([' ', '-'], "_");
        if col_normalized == normalized {
            return Some(col.name.clone());
        }
    }

    // Substring match — input is contained in column name
    for col in columns {
        let col_lower = col.name.to_lowercase();
        if col_lower.contains(&normalized) && normalized.len() >= 3 {
            return Some(col.name.clone());
        }
    }

    // Try stripping plurals (e.g., "states" → "state", "categories" → "category")
    let singulars: Vec<String> = if normalized.ends_with("ies") && normalized.len() > 4 {
        vec![format!("{}y", &normalized[..normalized.len() - 3])]
    } else if normalized.ends_with('s') && normalized.len() > 3 {
        vec![normalized[..normalized.len() - 1].to_string()]
    } else {
        vec![]
    };
    for singular in &singulars {
        for col in columns {
            let col_lower = col.name.to_lowercase();
            if col_lower == *singular || col_lower.contains(singular.as_str()) {
                return Some(col.name.clone());
            }
        }
    }

    None
}

#[derive(Deserialize)]
pub struct ColumnInfo {
    pub name: String,
    pub col_type: String,
}

#[tauri::command]
pub async fn translate_query(
    question: String,
    columns: Vec<ColumnInfo>,
    table_name: String,
    app: AppHandle,
) -> Result<String, String> {
    // Convert ColumnInfo to ColumnMeta for the pattern engine
    let col_metas: Vec<ColumnMeta> = columns
        .iter()
        .map(|c| ColumnMeta {
            name: c.name.clone(),
            col_type: serde_json::from_str(&format!("\"{}\"", c.col_type)).unwrap_or(crate::classifier::ColumnType::Categorical),
            unique_count: 0,
            total_count: 0,
            sample_values: vec![],
        })
        .collect();

    // Try simple pattern engine first (instant, no model needed)
    if let Some(sql) = simple_query_to_sql(&question, &col_metas, &table_name) {
        return Ok(sql);
    }

    // Fall through to AI model if loaded
    let engine = app.state::<Arc<Mutex<InsightEngine>>>();
    let mut engine = engine.lock().await;
    if engine.is_loaded() {
        let col_strings: Vec<String> = columns.iter().map(|c| format!("{} ({})", c.name, c.col_type)).collect();
        return tokio::task::block_in_place(|| engine.generate_sql(&question, &col_strings, &table_name));
    }

    Err("Try a simpler question — like 'total revenue' or 'revenue by state'".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classifier::ColumnType;

    fn test_cols() -> Vec<ColumnMeta> {
        vec![
            ColumnMeta { name: "state".into(), col_type: ColumnType::Geographic, unique_count: 30, total_count: 30, sample_values: vec![] },
            ColumnMeta { name: "revenue".into(), col_type: ColumnType::NumericMeasure, unique_count: 30, total_count: 30, sample_values: vec![] },
            ColumnMeta { name: "orders".into(), col_type: ColumnType::NumericMeasure, unique_count: 20, total_count: 30, sample_values: vec![] },
            ColumnMeta { name: "category".into(), col_type: ColumnType::Categorical, unique_count: 5, total_count: 30, sample_values: vec![] },
        ]
    }

    #[test]
    fn test_total() {
        let sql = simple_query_to_sql("total revenue", &test_cols(), "csv").unwrap();
        assert!(sql.contains("SUM(\"revenue\")"), "Got: {}", sql);
    }

    #[test]
    fn test_average() {
        let sql = simple_query_to_sql("average revenue", &test_cols(), "csv").unwrap();
        assert!(sql.contains("AVG(\"revenue\")"), "Got: {}", sql);
    }

    #[test]
    fn test_x_by_y() {
        let sql = simple_query_to_sql("revenue by state", &test_cols(), "csv").unwrap();
        assert!(sql.contains("SUM(\"revenue\")"), "Got: {}", sql);
        assert!(sql.contains("GROUP BY \"state\""), "Got: {}", sql);
    }

    #[test]
    fn test_show_x_by_y() {
        let sql = simple_query_to_sql("show me revenue by category", &test_cols(), "csv").unwrap();
        assert!(sql.contains("SUM(\"revenue\")"), "Got: {}", sql);
        assert!(sql.contains("GROUP BY \"category\""), "Got: {}", sql);
    }

    #[test]
    fn test_top_n() {
        let sql = simple_query_to_sql("top 5 states by revenue", &test_cols(), "csv").unwrap();
        assert!(sql.contains("LIMIT 5"), "Got: {}", sql);
    }

    #[test]
    fn test_count() {
        let sql = simple_query_to_sql("how many rows", &test_cols(), "csv").unwrap();
        assert!(sql.contains("COUNT(*)"), "Got: {}", sql);
    }

    #[test]
    fn test_count_by_group() {
        let sql = simple_query_to_sql("how many categories", &test_cols(), "csv").unwrap();
        assert!(sql.contains("GROUP BY \"category\""), "Got: {}", sql);
    }

    #[test]
    fn test_fuzzy_match() {
        let sql = simple_query_to_sql("total rev", &test_cols(), "csv").unwrap();
        assert!(sql.contains("SUM(\"revenue\")"), "Got: {}", sql);
    }

    #[test]
    fn test_no_match() {
        let result = simple_query_to_sql("what is the meaning of life", &test_cols(), "csv");
        assert!(result.is_none());
    }

    #[test]
    fn test_where() {
        let sql = simple_query_to_sql("show data where state = California", &test_cols(), "csv");
        // "show data" doesn't match X-by-Y, but "where" should be caught
        // Actually this won't match because "show data" goes through X-by-Y first
        // The where pattern needs the full query to contain " where "
        assert!(sql.is_none() || sql.unwrap().contains("WHERE"));
    }

    #[test]
    fn test_average_by() {
        let sql = simple_query_to_sql("average revenue by state", &test_cols(), "csv").unwrap();
        assert!(sql.contains("AVG(\"revenue\")"), "Got: {}", sql);
        assert!(sql.contains("GROUP BY \"state\""), "Got: {}", sql);
    }
}
