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
    BoxPlot,
    OrderedBar,
    MultiLineChart,
    Choropleth,
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
    pub title: String,
    pub metric: Option<String>,
    pub group_by: Option<String>,
    pub aggregate: Aggregate,
    pub sort: Option<String>,
    pub limit: Option<usize>,
    pub x_col: Option<String>,
    pub y_col: Option<String>,
    pub shown_by_default: bool,
    #[serde(default = "default_suggested_by")]
    pub suggested_by: String,
    #[serde(default)]
    pub insight: Option<String>,
    #[serde(default)]
    pub insight_edited: bool,
    #[serde(default)]
    pub denominator: Option<String>,
}

fn default_suggested_by() -> String {
    "rule".to_string()
}

/// Build a human-readable chart title.
fn humanize_title(
    view_type: &ViewType,
    metric: Option<&str>,
    group_by: Option<&str>,
    x_col: Option<&str>,
    y_col: Option<&str>,
    is_temporal_group: bool,
) -> String {
    match view_type {
        ViewType::SummaryCard => {
            let m = metric.unwrap_or("value");
            capitalize(m)
        }
        ViewType::ScatterPlot => {
            let x = capitalize(x_col.unwrap_or("x"));
            let y = capitalize(y_col.unwrap_or("y"));
            format!("{} vs {}", x, y)
        }
        ViewType::CountBar | ViewType::OrderedBar => {
            let g = group_by.unwrap_or("value");
            format!("{} breakdown", capitalize(g))
        }
        ViewType::Histogram => {
            let m = metric.unwrap_or("value");
            format!("{} distribution", capitalize(m))
        }
        ViewType::Choropleth => {
            let m = capitalize(metric.unwrap_or("value"));
            let g = group_by.unwrap_or("region");
            format!("{} by {}", m, prettify(g))
        }
        _ => {
            // LineChart, BarChart, etc.
            let m = capitalize(metric.unwrap_or("value"));
            if let Some(g) = group_by {
                if is_temporal_group {
                    format!("{} over time", m)
                } else {
                    format!("{} by {}", m, prettify(g))
                }
            } else {
                m
            }
        }
    }
}

/// Capitalize first letter, replace underscores with spaces.
fn capitalize(s: &str) -> String {
    let s = prettify(s);
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + &chars.as_str().to_lowercase(),
    }
}

/// Replace underscores with spaces, lowercase.
fn prettify(s: &str) -> String {
    s.replace('_', " ")
}

/// Known US state names and abbreviations for choropleth matching.
const US_STATES: &[&str] = &[
    "alabama", "al", "alaska", "ak", "arizona", "az", "arkansas", "ar",
    "california", "ca", "colorado", "co", "connecticut", "ct", "delaware", "de",
    "florida", "fl", "georgia", "ga", "hawaii", "hi", "idaho", "id",
    "illinois", "il", "indiana", "in", "iowa", "ia", "kansas", "ks",
    "kentucky", "ky", "louisiana", "la", "maine", "me", "maryland", "md",
    "massachusetts", "ma", "michigan", "mi", "minnesota", "mn", "mississippi", "ms",
    "missouri", "mo", "montana", "mt", "nebraska", "ne", "nevada", "nv",
    "new hampshire", "nh", "new jersey", "nj", "new mexico", "nm", "new york", "ny",
    "north carolina", "nc", "north dakota", "nd", "ohio", "oh", "oklahoma", "ok",
    "oregon", "or", "pennsylvania", "pa", "rhode island", "ri", "south carolina", "sc",
    "south dakota", "sd", "tennessee", "tn", "texas", "tx", "utah", "ut",
    "vermont", "vt", "virginia", "va", "washington", "wa", "west virginia", "wv",
    "wisconsin", "wi", "wyoming", "wy", "district of columbia", "dc",
];

/// A small set of common country names for choropleth matching.
const COUNTRIES: &[&str] = &[
    "united states", "usa", "us", "canada", "mexico", "brazil", "argentina",
    "united kingdom", "uk", "france", "germany", "italy", "spain", "portugal",
    "netherlands", "belgium", "sweden", "norway", "denmark", "finland",
    "russia", "china", "japan", "india", "australia", "south korea", "indonesia",
    "turkey", "egypt", "south africa", "nigeria", "kenya",
    "saudi arabia", "israel", "iran", "iraq", "pakistan",
    "thailand", "vietnam", "philippines", "malaysia", "singapore",
    "colombia", "chile", "peru", "venezuela", "cuba",
    "poland", "ukraine", "romania", "czech republic", "hungary", "austria",
    "switzerland", "ireland", "greece", "new zealand",
];

/// Check whether a geographic column's sample values match known geographies
/// (US states or world countries) well enough to render a choropleth.
pub fn matches_known_geography(sample_values: &[String]) -> bool {
    if sample_values.is_empty() {
        return false;
    }
    use std::collections::HashSet;
    let us_set: HashSet<&str> = US_STATES.iter().copied().collect();
    let country_set: HashSet<&str> = COUNTRIES.iter().copied().collect();

    let mut matches = 0;
    for val in sample_values {
        let lower = val.trim().to_lowercase();
        if us_set.contains(lower.as_str()) || country_set.contains(lower.as_str()) {
            matches += 1;
        }
    }
    // Require >50% of sample values to match
    matches * 2 > sample_values.len()
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
            c.col_type == ColumnType::Categorical
                || c.col_type == ColumnType::Geographic
                || c.col_type == ColumnType::NumericDimension
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

    // Helper: check if a column name is temporal
    let is_temporal = |name: &str| -> bool {
        columns.iter().any(|c| c.name == name && c.col_type == ColumnType::Temporal)
    };

    // 1. Summary card for primary metric
    if let Some(num) = primary_metric {
        let agg = default_aggregate(num);
        suggestions.push(make_view(ViewType::SummaryCard, Some(&num.name), None, agg, false));
    }

    // 2. Line chart (temporal + primary metric)
    if let Some(temp) = temporal {
        if let Some(num) = primary_metric {
            let agg = default_aggregate(num);
            suggestions.push(make_view(ViewType::LineChart, Some(&num.name), Some(&temp.name), agg, true));
        }
    }

    // 3. Bar chart for ALL categoricals + primary metric (no break)
    for cat in &categoricals {
        if cat.unique_count == 0 || cat.unique_count > 50 {
            continue;
        }
        if let Some(num) = primary_metric {
            let agg = default_aggregate(num);
            suggestions.push(make_bar_chart(&num.name, &cat.name, agg, false));
        }
    }

    // 4. Ordered bar for ALL ordinal columns (no break)
    for ord in &ordered_cats {
        suggestions.push(make_view(ViewType::OrderedBar, None, Some(&ord.name), Aggregate::Count, false));
    }

    // 5. count_bar when no numeric columns exist
    if numerics.is_empty() {
        for cat in categoricals.iter().chain(ordered_cats.iter()) {
            if cat.unique_count == 0 || cat.unique_count > 50 {
                continue;
            }
            suggestions.push(make_view(ViewType::CountBar, None, Some(&cat.name), Aggregate::Count, false));
        }
    }

    // 6. Scatter plot (first two numerics)
    if numerics.len() >= 2 {
        let title = humanize_title(&ViewType::ScatterPlot, None, None, Some(&numerics[0].name), Some(&numerics[1].name), false);
        suggestions.push(ViewSuggestion {
            view_type: ViewType::ScatterPlot,
            title,
            metric: None,
            group_by: None,
            aggregate: Aggregate::Sum,
            sort: None,
            limit: None,
            x_col: Some(numerics[0].name.clone()),
            y_col: Some(numerics[1].name.clone()),
            shown_by_default: false,
            suggested_by: "rule".to_string(),
            insight: None,
            insight_edited: false,
        denominator: None,
            });
    }

    // --- Phase 2: Alternatives (different view_type for same metric+group_by) ---

    // Bar chart alternative for line chart slots
    if let Some(temp) = temporal {
        if let Some(num) = primary_metric {
            let agg = default_aggregate(num);
            suggestions.push(make_bar_chart(&num.name, &temp.name, agg, is_temporal(&temp.name)));
        }
    }

    // (No alternative for high-cardinality categoricals — BarChart with limit is sufficient)

    // Choropleth alternative for geographic columns whose values match known geographies
    for col in columns.iter().filter(|c| c.col_type == ColumnType::Geographic) {
        if col.unique_count == 0 || col.unique_count > 50 {
            continue;
        }
        if !matches_known_geography(&col.sample_values) {
            continue;
        }
        if let Some(num) = primary_metric {
            let agg = default_aggregate(num);
            suggestions.push(make_view(ViewType::Choropleth, Some(&num.name), Some(&col.name), agg, false));
        }
    }

    // (No CountBar alternative for OrderedBar — they render identically)

    // Histogram for each numeric
    for num in &numerics {
        suggestions.push(make_view(ViewType::Histogram, Some(&num.name), None, Aggregate::Count, false));
    }

    // Summary cards for non-primary numerics
    for num in &numerics {
        if primary_metric.map(|p| p.name != num.name).unwrap_or(true) {
            let agg = default_aggregate(num);
            suggestions.push(make_view(ViewType::SummaryCard, Some(&num.name), None, agg, false));
        }
    }

    // --- Mark shown_by_default: first 3 are true ---
    for (i, s) in suggestions.iter_mut().enumerate() {
        s.shown_by_default = i < 3;
    }

    suggestions
}

fn make_view(view_type: ViewType, metric: Option<&str>, group_by: Option<&str>, aggregate: Aggregate, is_temporal_group: bool) -> ViewSuggestion {
    let title = humanize_title(&view_type, metric, group_by, None, None, is_temporal_group);
    ViewSuggestion {
        view_type,
        title,
        metric: metric.map(|s| s.to_string()),
        group_by: group_by.map(|s| s.to_string()),
        aggregate,
        sort: None,
        limit: None,
        x_col: None,
        y_col: None,
        shown_by_default: false,
        suggested_by: "rule".to_string(),
        insight: None,
        insight_edited: false,
        denominator: None,
    }
}

fn make_bar_chart(metric: &str, group_by: &str, aggregate: Aggregate, is_temporal_group: bool) -> ViewSuggestion {
    let title = humanize_title(&ViewType::BarChart, Some(metric), Some(group_by), None, None, is_temporal_group);
    ViewSuggestion {
        view_type: ViewType::BarChart,
        title,
        metric: Some(metric.to_string()),
        group_by: Some(group_by.to_string()),
        aggregate,
        sort: Some("desc".to_string()),
        limit: None,
        x_col: None,
        y_col: None,
        shown_by_default: false,
        suggested_by: "rule".to_string(),
        insight: None,
        insight_edited: false,
        denominator: None,
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

        // Low-cardinality categorical (product has 3 values) should get BarChart without limit
        let product_bar = suggestions.iter().find(|s| {
            s.view_type == ViewType::BarChart && s.group_by.as_deref() == Some("product")
        });
        assert!(product_bar.is_some(), "Expected BarChart for product");
        assert!(product_bar.unwrap().limit.is_none(), "Low-cardinality BarChart should have no limit");
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

    // --- Title generation tests ---

    #[test]
    fn test_title_revenue_over_time() {
        let title = humanize_title(&ViewType::LineChart, Some("revenue"), Some("date"), None, None, true);
        assert_eq!(title, "Revenue over time");
    }

    #[test]
    fn test_title_revenue_by_product() {
        let title = humanize_title(&ViewType::BarChart, Some("revenue"), Some("product"), None, None, false);
        assert_eq!(title, "Revenue by product");
    }

    #[test]
    fn test_title_revenue_by_region() {
        let title = humanize_title(&ViewType::BarChart, Some("revenue"), Some("region"), None, None, false);
        assert_eq!(title, "Revenue by region");
    }

    #[test]
    fn test_title_summary_card() {
        let title = humanize_title(&ViewType::SummaryCard, Some("revenue"), None, None, None, false);
        assert_eq!(title, "Revenue");
    }

    #[test]
    fn test_title_count_bar_breakdown() {
        let title = humanize_title(&ViewType::CountBar, None, Some("product"), None, None, false);
        assert_eq!(title, "Product breakdown");
    }

    #[test]
    fn test_title_histogram() {
        let title = humanize_title(&ViewType::Histogram, Some("revenue"), None, None, None, false);
        assert_eq!(title, "Revenue distribution");
    }

    #[test]
    fn test_title_scatter_plot() {
        let title = humanize_title(&ViewType::ScatterPlot, None, None, Some("spend"), Some("revenue"), false);
        assert_eq!(title, "Spend vs Revenue");
    }

    #[test]
    fn test_title_underscore_column() {
        let title = humanize_title(&ViewType::BarChart, Some("total_spend"), Some("customer_type"), None, None, false);
        assert_eq!(title, "Total spend by customer type");
    }

    #[test]
    fn test_shopify_no_choropleth() {
        // shopify region values ("West", "Central", "East") don't match known geographies
        let metas = classify_file("shopify.csv");
        let suggestions = suggest_views(&metas);
        let has_choropleth = suggestions.iter().any(|s| s.view_type == ViewType::Choropleth);
        assert!(!has_choropleth, "Choropleth should not appear for non-standard regions");
    }

    #[test]
    fn test_state_sales_has_choropleth() {
        let metas = classify_file("state-sales.csv");
        let suggestions = suggest_views(&metas);
        let has_choropleth = suggestions.iter().any(|s| {
            s.view_type == ViewType::Choropleth && s.group_by.as_deref() == Some("state")
        });
        assert!(has_choropleth, "Expected Choropleth for state+revenue, got: {:?}",
            suggestions.iter().map(|s| format!("{:?}:{}", s.view_type, s.title)).collect::<Vec<_>>());
    }

    #[test]
    fn test_matches_known_geography() {
        // US state names
        assert!(matches_known_geography(&["California", "Texas", "New York"].iter().map(|s| s.to_string()).collect::<Vec<_>>()));
        // US state abbreviations
        assert!(matches_known_geography(&["CA", "TX", "NY"].iter().map(|s| s.to_string()).collect::<Vec<_>>()));
        // Countries
        assert!(matches_known_geography(&["France", "Germany", "Japan"].iter().map(|s| s.to_string()).collect::<Vec<_>>()));
        // Non-geographic values
        assert!(!matches_known_geography(&["West", "Central", "East"].iter().map(|s| s.to_string()).collect::<Vec<_>>()));
        // Mixed — less than half match
        assert!(!matches_known_geography(&["California", "Region A", "Zone B"].iter().map(|s| s.to_string()).collect::<Vec<_>>()));
    }

    #[test]
    fn test_empty_columns_returns_empty() {
        let suggestions = suggest_views(&[]);
        assert!(suggestions.is_empty());
    }

    #[test]
    fn test_choropleth_title() {
        let title = humanize_title(&ViewType::Choropleth, Some("revenue"), Some("region"), None, None, false);
        assert_eq!(title, "Revenue by region");
    }

    #[test]
    fn test_shopify_titles() {
        let metas = classify_file("shopify.csv");
        let suggestions = suggest_views(&metas);

        let line = suggestions.iter().find(|s| s.view_type == ViewType::LineChart).unwrap();
        assert_eq!(line.title, "Revenue over time");

        let bar = suggestions.iter().find(|s| s.view_type == ViewType::BarChart && s.group_by.as_deref() == Some("product")).unwrap();
        assert_eq!(bar.title, "Revenue by product");

        let summary = suggestions.iter().find(|s| s.view_type == ViewType::SummaryCard && s.metric.as_deref() == Some("revenue")).unwrap();
        assert_eq!(summary.title, "Revenue");
    }
}
