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

fn is_date(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    // Try common date formats
    let formats = &[
        "%Y-%m-%d",
        "%Y/%m/%d",
        "%m/%d/%Y",
        "%m-%d-%Y",
        "%d/%m/%Y",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S",
    ];
    for fmt in formats {
        if chrono::NaiveDate::parse_from_str(value, fmt).is_ok()
            || chrono::NaiveDateTime::parse_from_str(value, fmt).is_ok()
        {
            return true;
        }
    }
    false
}

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

    // 1. Temporal: majority parse as date
    if !non_empty.is_empty() {
        let date_count = non_empty.iter().filter(|v| is_date(v)).count();
        if date_count as f64 / non_empty.len() as f64 > 0.5 {
            return ColumnMeta {
                name: name.to_string(),
                col_type: ColumnType::Temporal,
                unique_count,
                total_count,
                sample_values,
            };
        }
    }

    // 2. Geographic: name matches geo keywords
    let lower_name = name.to_lowercase();
    if GEO_NAMES.contains(&lower_name.as_str()) {
        return ColumnMeta {
            name: name.to_string(),
            col_type: ColumnType::Geographic,
            unique_count,
            total_count,
            sample_values,
        };
    }

    // 3. OrderedCategorical: values match known ordinal set (only if not all numeric)
    let all_numeric = !non_empty.is_empty()
        && non_empty.iter().filter(|v| is_numeric(v)).count() as f64 / non_empty.len() as f64
            > 0.5;
    if !all_numeric && is_ordinal_set(&unique) {
        return ColumnMeta {
            name: name.to_string(),
            col_type: ColumnType::OrderedCategorical,
            unique_count,
            total_count,
            sample_values,
        };
    }

    // 4. Numeric checks
    if !non_empty.is_empty() {
        let numeric_count = non_empty.iter().filter(|v| is_numeric(v)).count();
        if numeric_count as f64 / non_empty.len() as f64 > 0.5 {
            let ratio = unique_count as f64 / total_count as f64;

            // Low cardinality numeric → Categorical
            if ratio < 0.05 {
                return ColumnMeta {
                    name: name.to_string(),
                    col_type: ColumnType::Categorical,
                    unique_count,
                    total_count,
                    sample_values,
                };
            }

            // Ratio keyword in name and values between 0-1 or 0-100
            let has_ratio_keyword = RATIO_KEYWORDS.iter().any(|kw| lower_name.contains(kw));
            if has_ratio_keyword {
                let nums: Vec<f64> = non_empty
                    .iter()
                    .filter_map(|v| v.parse::<f64>().ok())
                    .collect();
                let all_ratio_range = nums.iter().all(|&n| (0.0..=1.0).contains(&n))
                    || nums.iter().all(|&n| (0.0..=100.0).contains(&n));
                if all_ratio_range {
                    return ColumnMeta {
                        name: name.to_string(),
                        col_type: ColumnType::NumericRatio,
                        unique_count,
                        total_count,
                        sample_values,
                    };
                }
            }

            // High-cardinality numeric with "id" in name → Identifier
            if unique_count as f64 / total_count as f64 > 0.9
                && lower_name.contains("id")
            {
                return ColumnMeta {
                    name: name.to_string(),
                    col_type: ColumnType::Identifier,
                    unique_count,
                    total_count,
                    sample_values,
                };
            }

            // Signed: any negative
            let has_negative = non_empty
                .iter()
                .filter_map(|v| v.parse::<f64>().ok())
                .any(|n| n < 0.0);
            if has_negative {
                return ColumnMeta {
                    name: name.to_string(),
                    col_type: ColumnType::NumericSigned,
                    unique_count,
                    total_count,
                    sample_values,
                };
            }

            return ColumnMeta {
                name: name.to_string(),
                col_type: ColumnType::NumericMeasure,
                unique_count,
                total_count,
                sample_values,
            };
        }
    }

    // 5. Identifier: very high cardinality text
    if total_count > 0 && unique_count as f64 / total_count as f64 > 0.9 {
        return ColumnMeta {
            name: name.to_string(),
            col_type: ColumnType::Identifier,
            unique_count,
            total_count,
            sample_values,
        };
    }

    // 6. Default: Categorical
    ColumnMeta {
        name: name.to_string(),
        col_type: ColumnType::Categorical,
        unique_count,
        total_count,
        sample_values,
    }
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
        assert_eq!(find_col(&metas, "nps_score").col_type, ColumnType::NumericMeasure);
    }
}
