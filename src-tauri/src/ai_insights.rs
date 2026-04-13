use crate::classifier::ColumnMeta;
use crate::model_manager::ModelManager;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::num::NonZeroU32;
use std::path::Path;
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tokio::sync::Mutex;

/// Maximum tokens to generate in a response.
const MAX_GENERATE_TOKENS: usize = 1024;
/// Context window size for inference.
const CONTEXT_SIZE: u32 = 4096;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiInsightsResponse {
    #[serde(default)]
    pub suggested_views: Vec<AiViewSuggestion>,
    #[serde(default)]
    pub insights: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiViewSuggestion {
    pub view_type: String,
    pub title: String,
    #[serde(default)]
    pub metric: Option<String>,
    #[serde(default)]
    pub group_by: Option<String>,
    #[serde(default)]
    pub aggregate: Option<String>,
    #[serde(default)]
    pub time_column: Option<String>,
    #[serde(default)]
    pub insight: Option<String>,
    #[serde(default)]
    pub x_col: Option<String>,
    #[serde(default)]
    pub y_col: Option<String>,
    #[serde(default)]
    pub sort: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Pre-computed summary for a single column.
pub struct ColumnSummary {
    pub name: String,
    pub col_type: crate::classifier::ColumnType,
    pub unique_count: usize,
    /// For categoricals/geographic: top values by frequency
    pub top_values: Vec<(String, usize)>,
    /// For numerics: min, max, mean
    pub numeric_stats: Option<(f64, f64, f64)>,
    /// For temporal: earliest and latest
    pub date_range: Option<(String, String)>,
}

/// Input context for prompt building.
pub struct CsvContext {
    pub filename: String,
    pub row_count: usize,
    pub columns: Vec<ColumnMeta>,
    pub summaries: Vec<ColumnSummary>,
    pub current_views: Vec<String>,
}

/// Compute column summaries from sample rows.
pub fn compute_summaries(columns: &[ColumnMeta], sample_rows: &[Vec<String>]) -> Vec<ColumnSummary> {
    use std::collections::HashMap;

    columns.iter().enumerate()
        .filter(|(_, col)| col.col_type != crate::classifier::ColumnType::Identifier)
        .map(|(i, col)| {
            let values: Vec<&str> = sample_rows.iter()
                .filter_map(|row| row.get(i).map(|s| s.as_str()))
                .filter(|s| !s.is_empty())
                .collect();

            let mut top_values = Vec::new();
            let mut numeric_stats = None;
            let mut date_range = None;

            match col.col_type {
                crate::classifier::ColumnType::NumericMeasure
                | crate::classifier::ColumnType::NumericRatio
                | crate::classifier::ColumnType::NumericSigned
                | crate::classifier::ColumnType::NumericDimension => {
                    let nums: Vec<f64> = values.iter()
                        .filter_map(|v| v.parse::<f64>().ok())
                        .collect();
                    if !nums.is_empty() {
                        let min = nums.iter().cloned().fold(f64::INFINITY, f64::min);
                        let max = nums.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                        let mean = nums.iter().sum::<f64>() / nums.len() as f64;
                        numeric_stats = Some((min, max, mean));
                    }
                }
                crate::classifier::ColumnType::Temporal => {
                    if let (Some(first), Some(last)) = (values.first(), values.last()) {
                        let mut sorted: Vec<&str> = values.clone();
                        sorted.sort();
                        date_range = Some((sorted[0].to_string(), sorted[sorted.len() - 1].to_string()));
                    }
                }
                _ => {
                    // Categorical, Geographic, OrderedCategorical
                    let mut counts: HashMap<&str, usize> = HashMap::new();
                    for v in &values {
                        *counts.entry(v).or_insert(0) += 1;
                    }
                    let mut sorted: Vec<_> = counts.into_iter().collect();
                    sorted.sort_by(|a, b| b.1.cmp(&a.1));
                    top_values = sorted.into_iter().take(5)
                        .map(|(k, v)| (k.to_string(), v))
                        .collect();
                }
            }

            ColumnSummary {
                name: col.name.clone(),
                col_type: col.col_type.clone(),
                unique_count: col.unique_count,
                top_values,
                numeric_stats,
                date_range,
            }
        })
        .collect()
}

pub struct InsightEngine {
    backend: Option<LlamaBackend>,
    model: Option<LlamaModel>,
}

// SAFETY: LlamaBackend and LlamaModel are thread-safe for read operations.
// We only mutate during load_model which is behind a Mutex.
unsafe impl Send for InsightEngine {}
unsafe impl Sync for InsightEngine {}

impl InsightEngine {
    pub fn new() -> Self {
        Self {
            backend: None,
            model: None,
        }
    }

    pub fn load_model(&mut self, path: &Path) -> Result<(), String> {
        if self.model.is_some() {
            return Ok(());
        }

        let backend =
            LlamaBackend::init().map_err(|e| format!("Failed to init llama backend: {}", e))?;

        let model_params = LlamaModelParams::default();
        let model_params = model_params.with_n_gpu_layers(0); // CPU only for portability

        let model = LlamaModel::load_from_file(&backend, path, &model_params)
            .map_err(|e| format!("Failed to load model: {}", e))?;

        self.model = Some(model);
        self.backend = Some(backend);
        Ok(())
    }

    pub fn is_loaded(&self) -> bool {
        self.model.is_some()
    }

    pub fn generate(&self, context: &CsvContext) -> Result<AiInsightsResponse, String> {
        let model = self.model.as_ref().ok_or("Model not loaded")?;
        let backend = self.backend.as_ref().ok_or("Backend not initialized")?;

        let prompt = build_prompt(context);

        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(CONTEXT_SIZE));

        let mut ctx = model
            .new_context(backend, ctx_params)
            .map_err(|e| format!("Failed to create context: {}", e))?;

        // Tokenize the prompt
        let tokens = model
            .str_to_token(&prompt, AddBos::Always)
            .map_err(|e| format!("Tokenization failed: {}", e))?;

        // Leave room for generation (at least 256 tokens)
        let max_prompt_tokens = CONTEXT_SIZE as usize - 256;
        if tokens.len() > max_prompt_tokens {
            return Err(format!(
                "Prompt too long ({} tokens, max {}). Dataset has too many columns for AI analysis.",
                tokens.len(), max_prompt_tokens
            ));
        }

        // Create batch and add prompt tokens
        let mut batch = LlamaBatch::new(CONTEXT_SIZE as usize, 1);
        let last_idx = tokens.len() - 1;
        for (i, &token) in tokens.iter().enumerate() {
            batch
                .add(token, i as i32, &[0], i == last_idx)
                .map_err(|e| format!("Failed to add token to batch: {}", e))?;
        }

        // Decode prompt
        ctx.decode(&mut batch)
            .map_err(|e| format!("Prompt decode failed: {}", e))?;

        // Set up sampler — temperature + greedy for structured output
        let mut sampler = LlamaSampler::chain_simple([
            LlamaSampler::temp(0.3),
            LlamaSampler::greedy(),
        ]);

        // Generate tokens
        let mut output_tokens = Vec::new();
        let mut n_cur = tokens.len();

        for _ in 0..MAX_GENERATE_TOKENS {
            let new_token = sampler.sample(&ctx, batch.n_tokens() - 1);

            if model.is_eog_token(new_token) {
                break;
            }

            output_tokens.push(new_token);

            // Prepare next batch
            batch.clear();
            batch
                .add(new_token, n_cur as i32, &[0], true)
                .map_err(|e| format!("Failed to add generated token: {}", e))?;
            n_cur += 1;

            ctx.decode(&mut batch)
                .map_err(|e| format!("Decode failed: {}", e))?;
        }

        // Convert tokens to text
        let mut output = String::new();
        let mut decoder = encoding_rs::UTF_8.new_decoder();
        for token in &output_tokens {
            let piece = model
                .token_to_piece(*token, &mut decoder, false, None)
                .map_err(|e| format!("Token decode failed: {}", e))?;
            output.push_str(&piece);
        }

        // Parse JSON from output
        parse_ai_response(&output, &context.columns)
    }
}

/// Build the prompt for the AI model.
pub fn build_prompt(context: &CsvContext) -> String {
    let mut prompt = String::new();

    prompt.push_str("<start_of_turn>user\n");
    prompt.push_str("You are a data analyst. Study this dataset and compute specific findings.\n\n");

    prompt.push_str(&format!("## Dataset: {} ({} rows)\n\n", context.filename, context.row_count));

    prompt.push_str("## Column Statistics:\n");
    for s in &context.summaries {
        prompt.push_str(&format!("### {} ({:?}, {} unique)\n", s.name, s.col_type, s.unique_count));
        if let Some((min, max, mean)) = s.numeric_stats {
            prompt.push_str(&format!("  min={:.2}, max={:.2}, mean={:.2}\n", min, max, mean));
        }
        if let Some((earliest, latest)) = &s.date_range {
            prompt.push_str(&format!("  range: {} to {}\n", earliest, latest));
        }
        if !s.top_values.is_empty() {
            let top: Vec<String> = s.top_values.iter()
                .map(|(val, count)| format!("{} ({})", val, count))
                .collect();
            prompt.push_str(&format!("  top values: {}\n", top.join(", ")));
        }
    }

    if !context.current_views.is_empty() {
        prompt.push_str("\n## Already shown:\n");
        for v in &context.current_views {
            prompt.push_str(&format!("- {}\n", v));
        }
    }

    prompt.push_str("\n## Task:\n");
    prompt.push_str("Suggest 1-3 NEW visualizations NOT already shown. Write a specific insight for each citing actual values.\n");
    prompt.push_str("Write 1-2 standalone insights with concrete numbers. NO generic statements.\n\n");
    prompt.push_str("Required fields by view_type:\n");
    prompt.push_str("- BarChart/LineChart/BoxPlot: metric, group_by, aggregate\n");
    prompt.push_str("- ScatterPlot: x_col, y_col (both numeric)\n");
    prompt.push_str("- Histogram: metric\n");
    prompt.push_str("- Heatmap: x_col, y_col (both categorical), metric, aggregate\n");
    prompt.push_str("- CountBar/OrderedBar: group_by\n");
    prompt.push_str("- BarChart with limit: metric, group_by, aggregate, sort, limit (for top-N)\n");
    prompt.push_str("- Choropleth: metric, group_by (geographic column like state/country), aggregate\n");
    prompt.push_str("Valid aggregate: Sum, Avg, Count, Min, Max\n\n");
    prompt.push_str("Respond with ONLY valid JSON. Example:\n");
    prompt.push_str(r#"{"suggested_views":[{"view_type":"BoxPlot","title":"Revenue spread by product","metric":"revenue","group_by":"product","aggregate":"Sum","insight":"Widget Pro median $155 vs Gadget Plus $112"},{"view_type":"ScatterPlot","title":"Revenue vs quantity","x_col":"revenue","y_col":"quantity","insight":"Positive correlation r=0.7"}],"insights":["Top 3 products account for 78% of total revenue"]}"#);
    prompt.push_str("\n\nONLY use column names from the dataset. Every insight MUST cite a number.\n");
    prompt.push_str("<end_of_turn>\n<start_of_turn>model\n");

    prompt
}

/// Check that an AI suggestion has the required fields for its view_type
/// and only references columns that exist in the dataset.
fn validate_ai_suggestion(s: &AiViewSuggestion, columns: &[ColumnMeta]) -> bool {
    let valid_columns: HashSet<&str> = columns.iter().map(|c| c.name.as_str()).collect();

    // Check required fields per view type
    let has_required = match s.view_type.as_str() {
        "BarChart" | "LineChart" | "BoxPlot" | "RankedList" => {
            s.metric.is_some() && s.group_by.is_some()
        }
        "Choropleth" => {
            if s.metric.is_none() || s.group_by.is_none() {
                return false;
            }
            // Validate that group_by column's values match known geographies
            if let Some(col) = columns.iter().find(|c| Some(&c.name) == s.group_by.as_ref()) {
                crate::suggester::matches_known_geography(&col.sample_values)
            } else {
                false
            }
        }
        "ScatterPlot" => {
            // Accept x_col/y_col (correct) or metric/group_by (common AI fallback)
            (s.x_col.is_some() && s.y_col.is_some()) || (s.metric.is_some() && s.group_by.is_some())
        }
        "Heatmap" => {
            // Accept x_col/y_col (correct) or metric/group_by (common AI fallback)
            (s.x_col.is_some() && s.y_col.is_some()) || (s.metric.is_some() && s.group_by.is_some())
        }
        "Histogram" | "SummaryCard" => s.metric.is_some(),
        "CountBar" | "OrderedBar" => s.group_by.is_some(),
        _ => true,
    };
    if !has_required {
        return false;
    }

    // Check all referenced columns exist in dataset
    let refs: Vec<&Option<String>> = vec![
        &s.metric, &s.group_by, &s.x_col, &s.y_col, &s.time_column,
    ];
    for col_ref in refs {
        if let Some(name) = col_ref {
            if !valid_columns.contains(name.as_str()) {
                return false;
            }
        }
    }

    true
}

/// Parse the AI model's JSON response, filtering out invalid suggestions.
pub fn parse_ai_response(raw: &str, columns: &[ColumnMeta]) -> Result<AiInsightsResponse, String> {
    let json_str = extract_json(raw);

    let mut response = serde_json::from_str::<AiInsightsResponse>(&json_str).map_err(|e| {
        format!(
            "Failed to parse AI response as JSON: {}. Raw output: {}",
            e,
            &raw[..raw.len().min(500)]
        )
    })?;

    response.suggested_views.retain(|s| validate_ai_suggestion(s, columns));

    Ok(response)
}

/// Extract JSON from model output, stripping markdown fences if present.
fn extract_json(raw: &str) -> String {
    let trimmed = raw.trim();

    // Check for ```json ... ``` wrapping
    if let Some(start) = trimmed.find('{') {
        // Find matching closing brace
        let mut depth = 0;
        let mut end = start;
        for (i, ch) in trimmed[start..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = start + i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        trimmed[start..end].to_string()
    } else {
        trimmed.to_string()
    }
}

#[tauri::command]
pub async fn generate_ai_insights(
    path: String,
    app: AppHandle,
) -> Result<AiInsightsResponse, String> {
    let model_mgr = app.state::<Arc<ModelManager>>();
    let engine = app.state::<Arc<Mutex<InsightEngine>>>();

    // Wait for model download if still in progress (poll every 2s, up to 10 min)
    {
        let max_wait = std::time::Duration::from_secs(600);
        let poll_interval = std::time::Duration::from_secs(2);
        let start = std::time::Instant::now();
        while !model_mgr.is_downloaded() {
            if !model_mgr.status().downloading && !model_mgr.is_downloaded() {
                return Err("Model not downloaded and no download in progress".to_string());
            }
            if start.elapsed() > max_wait {
                return Err("Timed out waiting for model download".to_string());
            }
            tokio::time::sleep(poll_interval).await;
        }
    }

    // Ensure model is loaded
    {
        let mut engine = engine.lock().await;
        if !engine.is_loaded() {
            engine.load_model(&model_mgr.model_path())?;
        }
    }

    // Parse CSV to get context
    let parsed = crate::commands::parse_csv(path.clone())?;

    let summaries = compute_summaries(&parsed.columns, &parsed.sample_rows);

    let context = CsvContext {
        filename: path
            .split('/')
            .last()
            .unwrap_or(&path)
            .to_string(),
        row_count: parsed.row_count,
        columns: parsed.columns.clone(),
        summaries,
        current_views: parsed
            .suggestions
            .iter()
            .filter(|s| s.shown_by_default)
            .map(|s| s.title.clone())
            .collect(),
    };

    // Run inference (may take a few seconds)
    let engine = engine.lock().await;
    let result = tokio::task::block_in_place(|| engine.generate(&context));

    match result {
        Ok(response) => Ok(response),
        Err(e) => Err(format!("AI inference failed: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classifier::ColumnType;

    fn sample_context() -> CsvContext {
        CsvContext {
            filename: "sales.csv".to_string(),
            row_count: 100,
            columns: vec![
                ColumnMeta {
                    name: "date".to_string(),
                    col_type: ColumnType::Temporal,
                    unique_count: 30,
                    total_count: 100,
                    sample_values: vec!["2024-01-01".to_string(), "2024-01-02".to_string()],
                },
                ColumnMeta {
                    name: "revenue".to_string(),
                    col_type: ColumnType::NumericMeasure,
                    unique_count: 95,
                    total_count: 100,
                    sample_values: vec!["1200.50".to_string(), "3400.00".to_string()],
                },
                ColumnMeta {
                    name: "product".to_string(),
                    col_type: ColumnType::Categorical,
                    unique_count: 5,
                    total_count: 100,
                    sample_values: vec!["Widget".to_string(), "Gadget".to_string()],
                },
            ],
            summaries: vec![
                ColumnSummary {
                    name: "date".to_string(),
                    col_type: ColumnType::Temporal,
                    unique_count: 30,
                    top_values: vec![],
                    numeric_stats: None,
                    date_range: Some(("2024-01-01".to_string(), "2024-12-31".to_string())),
                },
                ColumnSummary {
                    name: "revenue".to_string(),
                    col_type: ColumnType::NumericMeasure,
                    unique_count: 95,
                    top_values: vec![],
                    numeric_stats: Some((100.0, 5000.0, 1200.50)),
                    date_range: None,
                },
                ColumnSummary {
                    name: "product".to_string(),
                    col_type: ColumnType::Categorical,
                    unique_count: 5,
                    top_values: vec![("Widget".to_string(), 40), ("Gadget".to_string(), 30)],
                    numeric_stats: None,
                    date_range: None,
                },
            ],
            current_views: vec![
                "Revenue".to_string(),
                "Revenue over time".to_string(),
            ],
        }
    }

    fn test_columns() -> Vec<ColumnMeta> {
        vec![
            ColumnMeta { name: "date".to_string(), col_type: ColumnType::Temporal, unique_count: 30, total_count: 100, sample_values: vec![] },
            ColumnMeta { name: "revenue".to_string(), col_type: ColumnType::NumericMeasure, unique_count: 95, total_count: 100, sample_values: vec![] },
            ColumnMeta { name: "product".to_string(), col_type: ColumnType::Categorical, unique_count: 5, total_count: 100, sample_values: vec![] },
        ]
    }

    #[test]
    fn test_prompt_includes_columns() {
        let ctx = sample_context();
        let prompt = build_prompt(&ctx);
        assert!(prompt.contains("date"));
        assert!(prompt.contains("revenue"));
        assert!(prompt.contains("product"));
        assert!(prompt.contains("sales.csv"));
    }

    #[test]
    fn test_prompt_includes_summaries() {
        let ctx = sample_context();
        let prompt = build_prompt(&ctx);
        // Numeric stats
        assert!(prompt.contains("min=100.00"), "Should include numeric min");
        assert!(prompt.contains("mean=1200.50"), "Should include numeric mean");
        // Categorical top values
        assert!(prompt.contains("Widget (40)"), "Should include top value counts");
        // Date range
        assert!(prompt.contains("2024-01-01"), "Should include date range");
    }

    #[test]
    fn test_prompt_includes_current_views() {
        let ctx = sample_context();
        let prompt = build_prompt(&ctx);
        assert!(prompt.contains("Revenue over time"));
    }

    #[test]
    fn test_parse_valid_json() {
        let json = r#"{"suggested_views":[{"view_type":"BarChart","title":"Revenue by product","metric":"revenue","group_by":"product","aggregate":"Sum","insight":"Products show uneven revenue distribution"}],"insights":["Widget generates 60% of revenue"]}"#;
        let result = parse_ai_response(json, &test_columns());
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert_eq!(resp.suggested_views.len(), 1);
        assert_eq!(resp.insights.len(), 1);
        assert_eq!(resp.suggested_views[0].view_type, "BarChart");
    }

    #[test]
    fn test_parse_json_with_markdown_fence() {
        let raw = "```json\n{\"suggested_views\":[],\"insights\":[\"test\"]}\n```";
        let result = parse_ai_response(raw, &test_columns());
        assert!(result.is_ok());
        assert_eq!(result.unwrap().insights, vec!["test"]);
    }

    #[test]
    fn test_parse_malformed_json_fails() {
        let raw = "not json at all {{{";
        let result = parse_ai_response(raw, &test_columns());
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_empty_response() {
        let json = r#"{"suggested_views":[],"insights":[]}"#;
        let result = parse_ai_response(json, &test_columns());
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert!(resp.suggested_views.is_empty());
        assert!(resp.insights.is_empty());
    }

    #[test]
    fn test_parse_accepts_scatter_with_metric_group_by() {
        // ScatterPlot with metric/group_by (AI fallback) should be accepted
        let json = r#"{"suggested_views":[{"view_type":"ScatterPlot","title":"test","metric":"revenue","group_by":"date"}],"insights":[]}"#;
        let result = parse_ai_response(json, &test_columns()).unwrap();
        assert_eq!(result.suggested_views.len(), 1);
    }

    #[test]
    fn test_parse_filters_scatter_without_any_cols() {
        // ScatterPlot with no columns at all should be filtered out
        let json = r#"{"suggested_views":[{"view_type":"ScatterPlot","title":"test"}],"insights":[]}"#;
        let result = parse_ai_response(json, &test_columns()).unwrap();
        assert!(result.suggested_views.is_empty());
    }

    #[test]
    fn test_parse_keeps_valid_scatter() {
        let json = r#"{"suggested_views":[{"view_type":"ScatterPlot","title":"test","x_col":"revenue","y_col":"date"}],"insights":[]}"#;
        let result = parse_ai_response(json, &test_columns()).unwrap();
        assert_eq!(result.suggested_views.len(), 1);
    }

    #[test]
    fn test_parse_filters_nonexistent_column() {
        let json = r#"{"suggested_views":[{"view_type":"BarChart","title":"test","metric":"sales","group_by":"product","aggregate":"Sum"}],"insights":[]}"#;
        let result = parse_ai_response(json, &test_columns()).unwrap();
        assert!(result.suggested_views.is_empty()); // "sales" not in columns
    }

    #[test]
    fn test_parse_keeps_valid_bar() {
        let json = r#"{"suggested_views":[{"view_type":"BarChart","title":"test","metric":"revenue","group_by":"product","aggregate":"Sum"}],"insights":[]}"#;
        let result = parse_ai_response(json, &test_columns()).unwrap();
        assert_eq!(result.suggested_views.len(), 1);
    }

    #[test]
    fn test_parse_heatmap_with_metric_group_by_accepted() {
        // AI fallback: Heatmap using metric+group_by instead of x_col+y_col
        let json = r#"{"suggested_views":[{"view_type":"Heatmap","title":"test","metric":"revenue","group_by":"product"}],"insights":[]}"#;
        let result = parse_ai_response(json, &test_columns()).unwrap();
        assert_eq!(result.suggested_views.len(), 1);
    }

    #[test]
    fn test_parse_choropleth_non_geographic_rejected() {
        // Choropleth with group_by column whose values don't match known geographies
        let json = r#"{"suggested_views":[{"view_type":"Choropleth","title":"test","metric":"revenue","group_by":"product","aggregate":"Sum"}],"insights":[]}"#;
        let result = parse_ai_response(json, &test_columns()).unwrap();
        assert!(result.suggested_views.is_empty());
    }

    #[test]
    fn test_parse_missing_insights_key() {
        // Model omits "insights" key entirely — should still parse suggested_views
        let json = r#"{"suggested_views":[{"view_type":"BarChart","title":"test","metric":"revenue","group_by":"product","aggregate":"Sum"}]}"#;
        let result = parse_ai_response(json, &test_columns()).unwrap();
        assert_eq!(result.suggested_views.len(), 1);
        assert!(result.insights.is_empty());
    }

    #[test]
    fn test_parse_missing_suggested_views_key() {
        // Model omits "suggested_views" key — insights should still come through
        let json = r#"{"insights":["Revenue is growing 10% month over month"]}"#;
        let result = parse_ai_response(json, &test_columns()).unwrap();
        assert!(result.suggested_views.is_empty());
        assert_eq!(result.insights.len(), 1);
    }

    #[test]
    fn test_engine_not_loaded() {
        let engine = InsightEngine::new();
        assert!(!engine.is_loaded());
    }

    #[test]
    fn test_generate_fails_when_not_loaded() {
        let engine = InsightEngine::new();
        let ctx = sample_context();
        let result = engine.generate(&ctx);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not loaded"));
    }

    #[test]
    fn test_extract_json_plain() {
        let input = r#"{"a": 1}"#;
        assert_eq!(extract_json(input), r#"{"a": 1}"#);
    }

    #[test]
    fn test_extract_json_with_prefix() {
        let input = "Here is the JSON:\n{\"a\": 1}";
        assert_eq!(extract_json(input), r#"{"a": 1}"#);
    }

    #[test]
    fn test_extract_json_nested() {
        let input = r#"{"outer": {"inner": 1}}"#;
        assert_eq!(extract_json(input), r#"{"outer": {"inner": 1}}"#);
    }

    #[test]
    fn test_prompt_size_for_wide_dataset() {
        // Simulate customers-10000.csv: 12 columns, 8 identifiers
        let columns: Vec<ColumnMeta> = vec![
            ColumnMeta { name: "Index".into(), col_type: ColumnType::Identifier, unique_count: 5000, total_count: 10000, sample_values: vec!["1".into(), "2".into(), "3".into()] },
            ColumnMeta { name: "Customer Id".into(), col_type: ColumnType::Identifier, unique_count: 5000, total_count: 10000, sample_values: vec!["EB54EF".into()] },
            ColumnMeta { name: "First Name".into(), col_type: ColumnType::Categorical, unique_count: 690, total_count: 10000, sample_values: vec!["Heather".into(), "Bob".into(), "Alice".into()] },
            ColumnMeta { name: "Last Name".into(), col_type: ColumnType::Categorical, unique_count: 984, total_count: 10000, sample_values: vec!["Smith".into(), "Jones".into()] },
            ColumnMeta { name: "Company".into(), col_type: ColumnType::Identifier, unique_count: 4797, total_count: 10000, sample_values: vec!["Acme".into()] },
            ColumnMeta { name: "City".into(), col_type: ColumnType::Geographic, unique_count: 4715, total_count: 10000, sample_values: vec!["Portland".into(), "Denver".into()] },
            ColumnMeta { name: "Country".into(), col_type: ColumnType::Geographic, unique_count: 243, total_count: 10000, sample_values: vec!["Norway".into(), "France".into(), "Japan".into()] },
            ColumnMeta { name: "Phone 1".into(), col_type: ColumnType::Identifier, unique_count: 5000, total_count: 10000, sample_values: vec!["555-1234".into()] },
            ColumnMeta { name: "Phone 2".into(), col_type: ColumnType::Identifier, unique_count: 5000, total_count: 10000, sample_values: vec!["555-5678".into()] },
            ColumnMeta { name: "Email".into(), col_type: ColumnType::Identifier, unique_count: 5000, total_count: 10000, sample_values: vec!["a@b.com".into()] },
            ColumnMeta { name: "Subscription Date".into(), col_type: ColumnType::Temporal, unique_count: 878, total_count: 10000, sample_values: vec!["2020-08-26".into()] },
            ColumnMeta { name: "Website".into(), col_type: ColumnType::Identifier, unique_count: 4517, total_count: 10000, sample_values: vec!["http://example.com".into()] },
        ];
        let sample_rows: Vec<Vec<String>> = (0..5).map(|i| {
            vec![
                i.to_string(), format!("ID{}", i), "Alice".into(), "Smith".into(),
                "Acme".into(), "Portland".into(), "Norway".into(),
                "555-1234".into(), "555-5678".into(), "a@b.com".into(),
                "2020-01-01".into(), "http://example.com".into(),
            ]
        }).collect();
        let summaries = compute_summaries(&columns, &sample_rows);
        let ctx = CsvContext {
            filename: "customers-10000.csv".into(),
            row_count: 10000,
            columns,
            summaries,
            current_views: vec![],
        };
        let prompt = build_prompt(&ctx);
        // Prompt should be well under 4096 tokens (~4 chars per token as rough estimate)
        // Identifier columns should be filtered out
        assert!(!prompt.contains("Customer Id"), "Identifiers should be filtered from prompt");
        assert!(!prompt.contains("Phone 1"), "Identifiers should be filtered from prompt");
        assert!(prompt.contains("Country"), "Geographic columns should be in prompt");
        assert!(prompt.contains("Subscription Date"), "Temporal columns should be in prompt");
        // Rough size check: prompt should be under 3000 chars (~750 tokens)
        assert!(prompt.len() < 3000, "Prompt too long: {} chars", prompt.len());
    }
}
