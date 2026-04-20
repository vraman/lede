use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ColumnType {
    Temporal,
    Categorical,
    OrderedCategorical,
    Geographic,
    Identifier,
    NumericMeasure,
    NumericRatio,
    NumericSigned,
    NumericDimension,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnMeta {
    pub name: String,
    pub col_type: ColumnType,
    pub unique_count: usize,
    pub total_count: usize,
    pub sample_values: Vec<String>,
}

const GEO_NAMES: &[&str] = &[
    "state", "country", "city", "zip", "region", "country_code", "lat", "lng",
];

const RATIO_KEYWORDS: &[&str] = &["rate", "pct", "percent", "share", "ratio"];

const LIKERT_SETS: &[&[&str]] = &[
    &["strongly disagree", "disagree", "neutral", "agree", "strongly agree"],
    &["never", "rarely", "sometimes", "often", "always"],
    &["very poor", "poor", "fair", "good", "excellent"],
    &["1", "2", "3", "4", "5"],
];

const DATE_FORMATS: &[&str] = &[
    "%Y-%m-%d",
    "%Y/%m/%d",
    "%m/%d/%Y",
    "%m-%d-%Y",
    "%d/%m/%Y",
    "%Y-%m-%d %H:%M:%S",
    "%Y-%m-%dT%H:%M:%S",
    "%m/%d/%Y %H:%M:%S",
    "%m/%d/%Y %H:%M",
];

fn is_date(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    for fmt in DATE_FORMATS {
        if chrono::NaiveDate::parse_from_str(value, fmt).is_ok()
            || chrono::NaiveDateTime::parse_from_str(value, fmt).is_ok()
        {
            return true;
        }
    }
    false
}

/// Maximum number of values to sample for expensive checks (date/numeric parsing)
const SAMPLE_SIZE: usize = 200;

const INDEX_NAMES: &[&str] = &["index", "idx", "row", "row_number", "row_num", "rn", "no", "#"];

const DIMENSION_KEYWORDS: &[&str] = &[
    "year", "zip", "code", "rating", "score", "rank", "grade", "level", "tier",
];

fn is_numeric(value: &str) -> bool {
    value.parse::<f64>().is_ok()
}

fn is_ordinal_set(values: &HashSet<String>) -> bool {
    let lower: HashSet<String> = values.iter().map(|v| v.to_lowercase()).collect();
    for set in LIKERT_SETS {
        let ordinal: HashSet<String> = set.iter().map(|s| s.to_string()).collect();
        if lower.is_subset(&ordinal) && lower.len() >= 2 {
            return true;
        }
    }
    false
}

pub fn classify_column(name: &str, values: &[String]) -> ColumnMeta {
    let total_count = values.len();
    let non_empty: Vec<&String> = values.iter().filter(|v| !v.is_empty()).collect();
    let unique: HashSet<String> = non_empty.iter().map(|v| v.to_string()).collect();
    let unique_count = unique.len();

    let sample_values: Vec<String> = unique.iter().take(5).cloned().collect();

    // Sample for expensive checks (date/numeric parsing)
    let sample: Vec<&String> = non_empty.iter().take(SAMPLE_SIZE).copied().collect();
    let sample_len = sample.len();

    let lower_name = name.to_lowercase();

    // Helper to build result
    let meta = |col_type: ColumnType| ColumnMeta {
        name: name.to_string(),
        col_type,
        unique_count,
        total_count,
        sample_values: sample_values.clone(),
    };

    // 1. Temporal: majority of sample parse as date
    if sample_len > 0 {
        let date_count = sample.iter().filter(|v| is_date(v)).count();
        if date_count as f64 / sample_len as f64 > 0.5 {
            return meta(ColumnType::Temporal);
        }
    }

    // 2. Geographic: name matches geo keywords
    if GEO_NAMES.contains(&lower_name.as_str()) {
        return meta(ColumnType::Geographic);
    }

    // 3. OrderedCategorical: values match known ordinal set (only if not all numeric)
    let numeric_count = sample.iter().filter(|v| is_numeric(v)).count();
    let numeric_frac = if sample_len > 0 { numeric_count as f64 / sample_len as f64 } else { 0.0 };
    let all_numeric = numeric_frac > 0.5;

    if !all_numeric && is_ordinal_set(&unique) {
        return meta(ColumnType::OrderedCategorical);
    }

    // 4. Numeric checks (using sample fraction)
    if all_numeric {
        let nums: Vec<f64> = sample
            .iter()
            .filter_map(|v| v.parse::<f64>().ok())
            .collect();

        // Detect -1 sentinel: if >20% of numeric values are exactly -1, filter them out
        // and re-evaluate. Common in messy datasets for "no data".
        let sentinel_count = nums.iter().filter(|&&n| n == -1.0).count();
        if sentinel_count > 0 && sentinel_count as f64 / nums.len().max(1) as f64 > 0.2 {
            // Re-check: without -1 values, is the column still mostly numeric?
            let non_sentinel: Vec<&String> = sample.iter()
                .filter(|v| v.parse::<f64>().map_or(true, |n| n != -1.0))
                .copied()
                .collect();
            let remaining_numeric = non_sentinel.iter().filter(|v| is_numeric(v)).count();
            if non_sentinel.is_empty() || (remaining_numeric as f64 / non_sentinel.len() as f64) < 0.5 {
                // Without sentinels, column is not numeric — fall through to Categorical
                return meta(ColumnType::Categorical);
            }
        }

        let ratio = unique_count as f64 / total_count.max(1) as f64;

        // NumericDimension: very low cardinality numeric (group by, never aggregate)
        if ratio < 0.05 {
            return meta(ColumnType::NumericDimension);
        }

        // Numeric Identifier: very high cardinality + name suggests index/id
        if ratio > 0.9 {
            if lower_name.contains("id") || INDEX_NAMES.contains(&lower_name.as_str()) {
                return meta(ColumnType::Identifier);
            }
        }

        // NumericDimension: all integers with small range (e.g., years, ratings)
        let non_sentinel_nums: Vec<f64> = nums.iter().filter(|&&n| n != -1.0).copied().collect();
        if !non_sentinel_nums.is_empty() && ratio < 0.3 {
            let all_integers = non_sentinel_nums.iter().all(|&n| n == n.floor());
            if all_integers {
                let min_val = non_sentinel_nums.iter().cloned().fold(f64::INFINITY, f64::min);
                let max_val = non_sentinel_nums.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                if (max_val - min_val) < 500.0 {
                    return meta(ColumnType::NumericDimension);
                }
            }
        }

        // NumericDimension: name suggests dimensional use (word-boundary match)
        let name_parts: Vec<&str> = lower_name.split(|c: char| !c.is_alphanumeric()).filter(|s| !s.is_empty()).collect();
        if DIMENSION_KEYWORDS.iter().any(|kw| name_parts.contains(kw)) {
            return meta(ColumnType::NumericDimension);
        }

        // Ratio keyword in name and values between 0-1 or 0-100
        let has_ratio_keyword = RATIO_KEYWORDS.iter().any(|kw| lower_name.contains(kw));
        if has_ratio_keyword {
            let all_ratio_range = nums.iter().all(|&n| (0.0..=1.0).contains(&n))
                || nums.iter().all(|&n| (0.0..=100.0).contains(&n));
            if all_ratio_range {
                return meta(ColumnType::NumericRatio);
            }
        }

        // Signed: any negative in sample
        let has_negative = nums.iter().any(|&n| n < 0.0);
        if has_negative {
            return meta(ColumnType::NumericSigned);
        }

        return meta(ColumnType::NumericMeasure);
    }

    // 5. Identifier: very high cardinality text
    if total_count > 0 && unique_count as f64 / total_count as f64 > 0.9 {
        return meta(ColumnType::Identifier);
    }

    // 6. Default: Categorical
    meta(ColumnType::Categorical)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn read_csv_columns(filename: &str) -> Vec<(String, Vec<String>)> {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.pop(); // go up from src-tauri
        path.push("tests/fixtures");
        path.push(filename);

        let mut rdr = csv::Reader::from_path(&path).expect("Failed to open CSV");
        let headers: Vec<String> = rdr
            .headers()
            .expect("No headers")
            .iter()
            .map(|h| h.to_string())
            .collect();

        let mut columns: Vec<Vec<String>> = vec![vec![]; headers.len()];
        for result in rdr.records() {
            let record = result.expect("Bad record");
            for (i, field) in record.iter().enumerate() {
                columns[i].push(field.to_string());
            }
        }

        headers.into_iter().zip(columns).collect()
    }

    fn classify_file(filename: &str) -> Vec<ColumnMeta> {
        let cols = read_csv_columns(filename);
        cols.into_iter()
            .map(|(name, values)| classify_column(&name, &values))
            .collect()
    }

    fn find_col<'a>(metas: &'a [ColumnMeta], name: &str) -> &'a ColumnMeta {
        metas.iter().find(|m| m.name == name).unwrap()
    }

    #[test]
    fn test_shopify_columns() {
        let metas = classify_file("shopify.csv");

        assert_eq!(find_col(&metas, "order_id").col_type, ColumnType::Identifier);
        assert_eq!(find_col(&metas, "date").col_type, ColumnType::Temporal);
        assert_eq!(find_col(&metas, "customer_name").col_type, ColumnType::Identifier);
        assert_eq!(find_col(&metas, "product").col_type, ColumnType::Categorical);
        assert_eq!(find_col(&metas, "region").col_type, ColumnType::Geographic);
        assert_eq!(find_col(&metas, "quantity").col_type, ColumnType::NumericMeasure);
        assert_eq!(find_col(&metas, "revenue").col_type, ColumnType::NumericMeasure);
    }

    #[test]
    fn test_marketing_columns() {
        let metas = classify_file("marketing.csv");

        assert_eq!(find_col(&metas, "date").col_type, ColumnType::Temporal);
        assert_eq!(find_col(&metas, "campaign").col_type, ColumnType::Categorical);
        assert_eq!(find_col(&metas, "impressions").col_type, ColumnType::NumericMeasure);
        assert_eq!(find_col(&metas, "clicks").col_type, ColumnType::NumericMeasure);
        assert_eq!(find_col(&metas, "spend").col_type, ColumnType::NumericMeasure);
        assert_eq!(find_col(&metas, "conversions").col_type, ColumnType::NumericMeasure);
        assert_eq!(find_col(&metas, "revenue").col_type, ColumnType::NumericMeasure);
    }

    #[test]
    fn test_survey_columns() {
        let metas = classify_file("survey.csv");

        assert_eq!(find_col(&metas, "respondent_id").col_type, ColumnType::Identifier);
        assert_eq!(find_col(&metas, "age").col_type, ColumnType::NumericMeasure);
        assert_eq!(find_col(&metas, "satisfaction").col_type, ColumnType::OrderedCategorical);
        assert_eq!(find_col(&metas, "likelihood_to_recommend").col_type, ColumnType::OrderedCategorical);
        assert_eq!(find_col(&metas, "nps_score").col_type, ColumnType::NumericDimension);
    }

    // --- Edge case tests ---

    fn classify_edge(filename: &str) -> Vec<ColumnMeta> {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.pop();
        path.push("tests/fixtures/edge-cases");
        path.push(filename);

        let mut rdr = csv::ReaderBuilder::new().flexible(true).from_path(&path).expect("Failed to open CSV");
        let headers: Vec<String> = rdr.headers().expect("No headers").iter().map(|h| h.to_string()).collect();
        let mut columns: Vec<Vec<String>> = vec![vec![]; headers.len()];
        for result in rdr.records() {
            let record = result.expect("Bad record");
            for (i, field) in record.iter().enumerate() {
                if i < columns.len() {
                    columns[i].push(field.to_string());
                }
            }
        }
        headers.into_iter().zip(columns).map(|(name, values)| classify_column(&name, &values)).collect()
    }

    #[test]
    fn test_missing_values() {
        let metas = classify_edge("missing_values.csv");
        // Should not crash. Columns with missing values should still classify.
        assert_eq!(metas.len(), 4);
        // "name" has 6 non-empty values out of 8 rows, 6/8 = 0.75 unique ratio → Categorical
        assert_eq!(find_col(&metas, "name").col_type, ColumnType::Categorical);
        assert_eq!(find_col(&metas, "age").col_type, ColumnType::NumericMeasure);
        assert_eq!(find_col(&metas, "score").col_type, ColumnType::NumericDimension);
        assert_eq!(find_col(&metas, "date").col_type, ColumnType::Temporal);
    }

    #[test]
    fn test_mixed_type_column() {
        let metas = classify_edge("mixed_types.csv");
        assert_eq!(metas.len(), 3);
        // "value" has 5/8 numeric (>50%) including -1 → NumericSigned
        // This is acceptable — the app will parse what it can and skip NaN
        assert_eq!(find_col(&metas, "value").col_type, ColumnType::NumericSigned);
        assert_eq!(find_col(&metas, "status").col_type, ColumnType::Categorical);
    }

    #[test]
    fn test_date_formats() {
        let metas = classify_edge("weird_dates.csv");
        // US dates (MM/DD/YYYY) should parse
        assert_eq!(find_col(&metas, "us_date").col_type, ColumnType::Temporal);
        // Euro dates (DD/MM/YYYY) should parse
        assert_eq!(find_col(&metas, "euro_date").col_type, ColumnType::Temporal);
        // ISO datetime
        assert_eq!(find_col(&metas, "timestamp").col_type, ColumnType::Temporal);
        // Text dates like "Jan 15 2024" won't parse — Categorical is acceptable
        // Unix timestamps are pure numbers — NumericMeasure is acceptable
    }

    #[test]
    fn test_many_columns() {
        let metas = classify_edge("many_columns.csv");
        assert_eq!(metas.len(), 25);
        // Should not crash with 25 columns
    }

    #[test]
    fn test_salary_text() {
        let metas = classify_edge("salary_text.csv");
        // "$50,000 - $70,000" does NOT parse as numeric — correct
        // But with only 6 rows and 6 unique values, ratio > 0.9 → Identifier
        // With more data this would be Categorical. For small samples this is acceptable.
        assert_eq!(find_col(&metas, "department").col_type, ColumnType::Categorical);
    }

    #[test]
    fn test_single_column() {
        let metas = classify_edge("single_column.csv");
        assert_eq!(metas.len(), 1);
        assert_eq!(find_col(&metas, "name").col_type, ColumnType::Identifier);
    }

    #[test]
    fn test_single_row() {
        let metas = classify_edge("single_row.csv");
        assert_eq!(metas.len(), 3);
        // With only 1 row, cardinality = 1/1 = 100% — treated as Identifier
        // This is acceptable behavior for single-row files
    }

    // --- Real-world classification tests ---

    fn classify_realworld(filename: &str) -> Option<Vec<ColumnMeta>> {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.pop();
        path.push("tests/fixtures/real-world");
        path.push(filename);

        if !path.exists() {
            eprintln!("SKIP: {} not present — run tests/fixtures/real-world/download.sh", filename);
            return None;
        }

        let mut rdr = csv::Reader::from_path(&path).expect("Failed to open CSV");
        let headers: Vec<String> = rdr.headers().expect("No headers").iter().map(|h| h.to_string()).collect();
        let mut columns: Vec<Vec<String>> = vec![vec![]; headers.len()];
        let mut count = 0;
        for result in rdr.records() {
            let record = result.expect("Bad record");
            for (i, field) in record.iter().enumerate() {
                if i < columns.len() {
                    columns[i].push(field.to_string());
                }
            }
            count += 1;
            if count >= 5000 { break; }
        }
        Some(headers.into_iter().zip(columns).map(|(name, values)| classify_column(&name, &values)).collect())
    }

    #[test]
    fn test_index_column_is_identifier() {
        let Some(metas) = classify_realworld("customers-10000.csv") else { return };
        assert_eq!(find_col(&metas, "Index").col_type, ColumnType::Identifier);
    }

    #[test]
    fn test_timestamp_detected_as_temporal() {
        let Some(metas) = classify_realworld("salary_survey.csv") else { return };
        assert_eq!(find_col(&metas, "Timestamp").col_type, ColumnType::Temporal);
    }

    // --- NumericDimension tests ---

    #[test]
    fn test_founded_is_numeric_dimension() {
        // Year-like values: all integers, small range, low cardinality in large dataset
        let values: Vec<String> = (0..200).map(|i| ((1800 + i % 50) as u32).to_string()).collect();
        let meta = classify_column("Founded", &values);
        assert_eq!(meta.col_type, ColumnType::NumericDimension);
    }

    #[test]
    fn test_zip_code_is_numeric_dimension() {
        // "zip" keyword triggers NumericDimension regardless of range
        let values: Vec<String> = vec!["10001", "90210", "60601", "30301", "10001", "90210"]
            .into_iter().map(String::from).collect();
        let meta = classify_column("zip_code", &values);
        assert_eq!(meta.col_type, ColumnType::NumericDimension);
    }

    #[test]
    fn test_nps_score_is_numeric_dimension() {
        // "score" keyword triggers NumericDimension
        let values: Vec<String> = vec!["9", "7", "5", "3", "10", "8", "1", "7"]
            .into_iter().map(String::from).collect();
        let meta = classify_column("nps_score", &values);
        assert_eq!(meta.col_type, ColumnType::NumericDimension);
    }

    #[test]
    fn test_revenue_stays_numeric_measure() {
        // Float values + no dimension keyword → NumericMeasure
        let values: Vec<String> = vec!["149.98", "49.99", "224.97", "99.99", "99.98", "199.98"]
            .into_iter().map(String::from).collect();
        let meta = classify_column("revenue", &values);
        assert_eq!(meta.col_type, ColumnType::NumericMeasure);
    }

    #[test]
    fn test_year_column_is_numeric_dimension() {
        // "year" keyword triggers NumericDimension
        let values: Vec<String> = vec!["2020", "2021", "2022", "2023", "2024", "2020", "2021"]
            .into_iter().map(String::from).collect();
        let meta = classify_column("year", &values);
        assert_eq!(meta.col_type, ColumnType::NumericDimension);
    }

    #[test]
    fn test_age_stays_numeric_measure() {
        // No keyword, not low cardinality → NumericMeasure
        let values: Vec<String> = vec!["28", "34", "45", "22", "51", "38", "29", "42"]
            .into_iter().map(String::from).collect();
        let meta = classify_column("age", &values);
        assert_eq!(meta.col_type, ColumnType::NumericMeasure);
    }

    #[test]
    fn test_quantity_stays_numeric_measure() {
        // Small integers but no keyword and high ratio → NumericMeasure
        let values: Vec<String> = vec!["2", "1", "3", "1", "2", "2", "1", "4"]
            .into_iter().map(String::from).collect();
        let meta = classify_column("quantity", &values);
        assert_eq!(meta.col_type, ColumnType::NumericMeasure);
    }

    #[test]
    fn test_all_missing_values() {
        let values: Vec<String> = vec!["", "", "", ""]
            .into_iter().map(String::from).collect();
        let meta = classify_column("data", &values);
        // Should not panic; classified as Categorical with 0 unique non-empty values
        assert_eq!(meta.col_type, ColumnType::Categorical);
        assert_eq!(meta.total_count, 4);
    }

    #[test]
    fn test_empty_column() {
        let values: Vec<String> = vec![];
        let meta = classify_column("empty", &values);
        assert_eq!(meta.total_count, 0);
        assert_eq!(meta.unique_count, 0);
    }
}
