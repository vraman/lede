use crate::classifier::{classify_column, ColumnMeta};
use crate::suggester::{suggest_views, ViewSuggestion};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedCSV {
    pub columns: Vec<ColumnMeta>,
    pub row_count: usize,
    pub sample_rows: Vec<Vec<String>>,
    pub suggestions: Vec<ViewSuggestion>,
}

#[tauri::command]
pub fn parse_csv(path: String) -> Result<ParsedCSV, String> {
    let mut rdr = csv::Reader::from_path(&path).map_err(|e| format!("Failed to open CSV: {}", e))?;

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
        let row: Vec<String> = record.iter().map(|f| f.to_string()).collect();

        if row_count < 10 {
            sample_rows.push(row.clone());
        }

        for (i, field) in row.into_iter().enumerate() {
            if i < num_cols {
                columns_data[i].push(field);
            }
        }
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
}
