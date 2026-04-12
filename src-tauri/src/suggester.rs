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
    pub shown_by_default: bool,
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

    // Pick the best numeric column: prefer revenue, sales, amount, spend, etc.
    let primary_metric = {
        let preferred = ["revenue", "sales", "amount", "total", "spend", "price", "income"];
        numerics
            .iter()
            .find(|c| preferred.contains(&c.name.to_lowercase().as_str()))
            .or(numerics.last())
            .copied()
    };

    // --- Phase 1: Primary suggestions (highest priority first) ---

    // 1. Summary card for primary metric
    if let Some(num) = primary_metric {
        let agg = default_aggregate(num);
        suggestions.push(make_view(ViewType::SummaryCard, Some(&num.name), None, agg));
    }

    // 2. Line chart (temporal + primary metric)
    if let Some(temp) = temporal {
        if let Some(num) = primary_metric {
            let agg = default_aggregate(num);
            suggestions.push(make_view(ViewType::LineChart, Some(&num.name), Some(&temp.name), agg));
        }
    }

    // 3. Bar chart for ALL categoricals + primary metric (no break)
    for cat in &categoricals {
        if cat.unique_count == 0 || cat.unique_count > 50 {
            continue;
        }
        if let Some(num) = primary_metric {
            let agg = default_aggregate(num);
            if cat.unique_count > 20 {
                suggestions.push(make_ranked_list(&num.name, &cat.name, agg));
            } else {
                suggestions.push(make_bar_chart(&num.name, &cat.name, agg));
            }
        }
    }

    // 4. Ordered bar for ALL ordinal columns (no break)
    for ord in &ordered_cats {
        suggestions.push(make_view(ViewType::OrderedBar, None, Some(&ord.name), Aggregate::Count));
    }

    // 5. count_bar when no numeric columns exist
    if numerics.is_empty() {
        for cat in categoricals.iter().chain(ordered_cats.iter()) {
            if cat.unique_count == 0 || cat.unique_count > 50 {
                continue;
            }
            suggestions.push(make_view(ViewType::CountBar, None, Some(&cat.name), Aggregate::Count));
        }
    }

    // 6. Scatter plot (first two numerics)
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
            shown_by_default: false,
        });
    }

    // --- Phase 2: Alternatives (different view_type for same metric+group_by) ---

    // Bar chart alternative for line chart slots
    if let Some(temp) = temporal {
        if let Some(num) = primary_metric {
            let agg = default_aggregate(num);
            suggestions.push(make_bar_chart(&num.name, &temp.name, agg));
        }
    }

    // BarChart alternative for high-cardinality categorical slots (where RankedList is primary)
    // Low-cardinality categoricals (<=20) already have BarChart as primary — RankedList
    // would look identical, so no alternative is offered.
    for cat in &categoricals {
        if cat.unique_count > 20 && cat.unique_count <= 50 {
            if let Some(num) = primary_metric {
                let agg = default_aggregate(num);
                suggestions.push(make_bar_chart(&num.name, &cat.name, agg));
            }
        }
    }

    // (No CountBar alternative for OrderedBar — they render identically)

    // Histogram for each numeric
    for num in &numerics {
        suggestions.push(make_view(ViewType::Histogram, Some(&num.name), None, Aggregate::Count));
    }

    // Summary cards for non-primary numerics
    for num in &numerics {
        if primary_metric.map(|p| p.name != num.name).unwrap_or(true) {
            let agg = default_aggregate(num);
            suggestions.push(make_view(ViewType::SummaryCard, Some(&num.name), None, agg));
        }
    }

    // --- Mark shown_by_default: first 3 are true ---
    for (i, s) in suggestions.iter_mut().enumerate() {
        s.shown_by_default = i < 3;
    }

    suggestions
}

fn make_view(view_type: ViewType, metric: Option<&str>, group_by: Option<&str>, aggregate: Aggregate) -> ViewSuggestion {
    ViewSuggestion {
        view_type,
        metric: metric.map(|s| s.to_string()),
        group_by: group_by.map(|s| s.to_string()),
        aggregate,
        sort: None,
        limit: None,
        x_col: None,
        y_col: None,
        shown_by_default: false,
    }
}

fn make_bar_chart(metric: &str, group_by: &str, aggregate: Aggregate) -> ViewSuggestion {
    ViewSuggestion {
        view_type: ViewType::BarChart,
        metric: Some(metric.to_string()),
        group_by: Some(group_by.to_string()),
        aggregate,
        sort: Some("desc".to_string()),
        limit: None,
        x_col: None,
        y_col: None,
        shown_by_default: false,
    }
}

fn make_ranked_list(metric: &str, group_by: &str, aggregate: Aggregate) -> ViewSuggestion {
    ViewSuggestion {
        view_type: ViewType::RankedList,
        metric: Some(metric.to_string()),
        group_by: Some(group_by.to_string()),
        aggregate,
        sort: Some("desc".to_string()),
        limit: Some(10),
        x_col: None,
        y_col: None,
        shown_by_default: false,
    }
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

        // Should return more than 5 (alternatives included)
        assert!(suggestions.len() > 5, "Expected >5 suggestions (with alternatives), got {}", suggestions.len());

        // First 3 should be shown_by_default
        assert!(suggestions[0].shown_by_default);
        assert!(suggestions[1].shown_by_default);
        assert!(suggestions[2].shown_by_default);
        assert!(!suggestions[3].shown_by_default);

        // Low-cardinality categorical (product has 3 values) should get BarChart but NOT
        // a RankedList alternative (they'd look identical)
        let has_product_bar = suggestions.iter().any(|s| {
            s.view_type == ViewType::BarChart && s.group_by.as_deref() == Some("product")
        });
        let has_product_ranked = suggestions.iter().any(|s| {
            s.view_type == ViewType::RankedList && s.group_by.as_deref() == Some("product")
        });
        assert!(has_product_bar, "Expected BarChart for product");
        assert!(!has_product_ranked, "RankedList should not appear for low-cardinality product");
    }

    #[test]
    fn test_marketing_suggestions() {
        let metas = classify_file("marketing.csv");
        let suggestions = suggest_views(&metas);

        let types: Vec<&ViewType> = suggestions.iter().map(|s| &s.view_type).collect();
        assert!(types.contains(&&ViewType::ScatterPlot), "Expected scatter_plot, got {:?}", types);
        assert!(suggestions.len() > 5, "Expected >5 with alternatives, got {}", suggestions.len());

        // First 3 shown_by_default
        let shown_count = suggestions.iter().filter(|s| s.shown_by_default).count();
        assert_eq!(shown_count, 3);
    }

    #[test]
    fn test_survey_suggestions() {
        let metas = classify_file("survey.csv");
        let suggestions = suggest_views(&metas);

        // Should have ordered_bar for satisfaction, but NOT count_bar (they render identically)
        let has_ordered = suggestions.iter().any(|s| {
            s.group_by.as_deref() == Some("satisfaction") && s.view_type == ViewType::OrderedBar
        });
        let has_count = suggestions.iter().any(|s| {
            s.group_by.as_deref() == Some("satisfaction") && s.view_type == ViewType::CountBar
        });
        assert!(has_ordered, "Expected OrderedBar for satisfaction");
        assert!(!has_count, "CountBar should not appear as alternative (renders identically)");

        // respondent_id is an Identifier — should NOT be used as group_by in bar_chart
        let has_respondent_bar = suggestions.iter().any(|s| {
            s.group_by.as_deref() == Some("respondent_id") && s.view_type == ViewType::BarChart
        });
        assert!(!has_respondent_bar, "respondent_id should not get a bar_chart");
    }
}
