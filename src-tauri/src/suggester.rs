use crate::classifier::{ColumnMeta, ColumnType};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ViewType {
    Table,
    SummaryCard,
    LineChart,
    BarChart,
    CountBar,
    ScatterPlot,
    Histogram,
    Heatmap,
    OrderedBar,
    RankedList,
    MultiLineChart,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Aggregate {
    Sum,
    Avg,
    Count,
    Min,
    Max,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewSuggestion {
    pub view_type: ViewType,
    pub metric: Option<String>,
    pub group_by: Option<String>,
    pub aggregate: Aggregate,
    pub sort: Option<String>,
    pub limit: Option<usize>,
    pub x_col: Option<String>,
    pub y_col: Option<String>,
}

pub fn suggest_views(columns: &[ColumnMeta]) -> Vec<ViewSuggestion> {
    let mut suggestions: Vec<ViewSuggestion> = Vec::new();

    let temporals: Vec<&ColumnMeta> = columns
        .iter()
        .filter(|c| c.col_type == ColumnType::Temporal)
        .collect();
    let categoricals: Vec<&ColumnMeta> = columns
        .iter()
        .filter(|c| {
            c.col_type == ColumnType::Categorical || c.col_type == ColumnType::Geographic
        })
        .collect();
    let ordered_cats: Vec<&ColumnMeta> = columns
        .iter()
        .filter(|c| c.col_type == ColumnType::OrderedCategorical)
        .collect();
    let numerics: Vec<&ColumnMeta> = columns
        .iter()
        .filter(|c| {
            matches!(
                c.col_type,
                ColumnType::NumericMeasure | ColumnType::NumericRatio | ColumnType::NumericSigned
            )
        })
        .collect();

    // Pick the best temporal column: prefer date, created_at, timestamp
    let temporal = if temporals.len() > 1 {
        let preferred = ["date", "created_at", "timestamp"];
        temporals
            .iter()
            .find(|c| preferred.contains(&c.name.to_lowercase().as_str()))
            .or(temporals.first())
            .copied()
    } else {
        temporals.first().copied()
    };

    // 1. Always: summary_card (pick first numeric)
    if let Some(num) = numerics.first() {
        let agg = default_aggregate(num);
        suggestions.push(ViewSuggestion {
            view_type: ViewType::SummaryCard,
            metric: Some(num.name.clone()),
            group_by: None,
            aggregate: agg,
            sort: None,
            limit: None,
            x_col: None,
            y_col: None,
        });
    }

    // 2. Strong: line_chart (temporal + numeric)
    if let Some(temp) = temporal {
        if let Some(num) = numerics.first() {
            let agg = default_aggregate(num);
            suggestions.push(ViewSuggestion {
                view_type: ViewType::LineChart,
                metric: Some(num.name.clone()),
                group_by: Some(temp.name.clone()),
                aggregate: agg,
                sort: None,
                limit: None,
                x_col: None,
                y_col: None,
            });
        }
    }

    // 3. Strong: bar_chart (categorical + numeric) — guard: unique_count > 50 → skip
    for cat in &categoricals {
        if cat.unique_count > 50 {
            continue;
        }
        if let Some(num) = numerics.first() {
            let agg = default_aggregate(num);
            if cat.unique_count > 20 {
                // ranked_list instead
                suggestions.push(ViewSuggestion {
                    view_type: ViewType::RankedList,
                    metric: Some(num.name.clone()),
                    group_by: Some(cat.name.clone()),
                    aggregate: agg,
                    sort: Some("desc".to_string()),
                    limit: Some(10),
                    x_col: None,
                    y_col: None,
                });
            } else {
                suggestions.push(ViewSuggestion {
                    view_type: ViewType::BarChart,
                    metric: Some(num.name.clone()),
                    group_by: Some(cat.name.clone()),
                    aggregate: agg,
                    sort: Some("desc".to_string()),
                    limit: None,
                    x_col: None,
                    y_col: None,
                });
            }
            break; // Only one bar chart
        }
    }

    // 4. Ordered bar for ordinal columns
    for ord in &ordered_cats {
        suggestions.push(ViewSuggestion {
            view_type: ViewType::OrderedBar,
            metric: None,
            group_by: Some(ord.name.clone()),
            aggregate: Aggregate::Count,
            sort: None,
            limit: None,
            x_col: None,
            y_col: None,
        });
        break; // Only one
    }

    // 5. count_bar when no numeric columns exist
    if numerics.is_empty() {
        if let Some(cat) = categoricals.first().or(ordered_cats.first()) {
            suggestions.push(ViewSuggestion {
                view_type: ViewType::CountBar,
                metric: None,
                group_by: Some(cat.name.clone()),
                aggregate: Aggregate::Count,
                sort: Some("desc".to_string()),
                limit: None,
                x_col: None,
                y_col: None,
            });
        }
    }

    // 6. Scatter plot: highest-variance pair (just use first two numerics)
    if numerics.len() >= 2 {
        suggestions.push(ViewSuggestion {
            view_type: ViewType::ScatterPlot,
            metric: None,
            group_by: None,
            aggregate: Aggregate::Sum,
            sort: None,
            limit: None,
            x_col: Some(numerics[0].name.clone()),
            y_col: Some(numerics[1].name.clone()),
        });
    }

    // 7. Histogram for single numeric
    if numerics.len() == 1 {
        suggestions.push(ViewSuggestion {
            view_type: ViewType::Histogram,
            metric: Some(numerics[0].name.clone()),
            group_by: None,
            aggregate: Aggregate::Count,
            sort: None,
            limit: None,
            x_col: None,
            y_col: None,
        });
    }

    // Cap at 5 suggestions
    suggestions.truncate(5);
    suggestions
}

fn default_aggregate(col: &ColumnMeta) -> Aggregate {
    if col.col_type == ColumnType::NumericRatio {
        Aggregate::Avg
    } else {
        Aggregate::Sum
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classifier::classify_column;
    use std::path::PathBuf;

    fn classify_file(filename: &str) -> Vec<ColumnMeta> {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.pop();
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

        headers
            .into_iter()
            .zip(columns)
            .map(|(name, values)| classify_column(&name, &values))
            .collect()
    }

    #[test]
    fn test_shopify_suggestions() {
        let metas = classify_file("shopify.csv");
        let suggestions = suggest_views(&metas);

        let types: Vec<&ViewType> = suggestions.iter().map(|s| &s.view_type).collect();
        assert!(types.contains(&&ViewType::LineChart), "Expected line_chart, got {:?}", types);
        assert!(types.contains(&&ViewType::BarChart), "Expected bar_chart, got {:?}", types);
        assert!(types.contains(&&ViewType::SummaryCard), "Expected summary_card, got {:?}", types);
        assert!(suggestions.len() <= 5, "Max 5 suggestions");
    }

    #[test]
    fn test_marketing_suggestions() {
        let metas = classify_file("marketing.csv");
        let suggestions = suggest_views(&metas);

        let types: Vec<&ViewType> = suggestions.iter().map(|s| &s.view_type).collect();
        assert!(types.contains(&&ViewType::ScatterPlot), "Expected scatter_plot, got {:?}", types);
        assert!(suggestions.len() <= 5);
    }

    #[test]
    fn test_survey_suggestions() {
        let metas = classify_file("survey.csv");
        let suggestions = suggest_views(&metas);

        let types: Vec<&ViewType> = suggestions.iter().map(|s| &s.view_type).collect();

        // Should have ordered_bar or count_bar for satisfaction
        let has_satisfaction_view = suggestions.iter().any(|s| {
            s.group_by.as_deref() == Some("satisfaction")
                && (s.view_type == ViewType::OrderedBar || s.view_type == ViewType::CountBar)
        });
        assert!(has_satisfaction_view, "Expected ordered bar for satisfaction, got {:?}", suggestions);

        // respondent_id is an Identifier — should NOT be used as group_by in bar_chart
        let has_respondent_bar = suggestions.iter().any(|s| {
            s.group_by.as_deref() == Some("respondent_id") && s.view_type == ViewType::BarChart
        });
        assert!(!has_respondent_bar, "respondent_id should not get a bar_chart");
    }
}
