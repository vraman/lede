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

/// Public response sent to the frontend — insights are plain strings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiInsightsResponse {
    #[serde(default)]
    pub suggested_views: Vec<AiViewSuggestion>,
    #[serde(default)]
    pub insights: Vec<String>,
    #[serde(default)]
    pub suggested_title: Option<String>,
    #[serde(default)]
    pub suggested_summary: Option<AiSummary>,
}

/// AI-suggested summary card override.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiSummary {
    pub title: String,
    pub metric: String,
    pub aggregate: String,
    /// Optional denominator for ratio metrics (e.g., revenue / orders)
    #[serde(default)]
    pub denominator: Option<String>,
}

/// Internal response parsed from the model — insights may be structured.
#[derive(Debug, Clone, Deserialize)]
struct RawAiResponse {
    #[serde(default)]
    suggested_views: Vec<AiViewSuggestion>,
    #[serde(default)]
    insights: Vec<RawInsight>,
    #[serde(default)]
    suggested_title: Option<String>,
    #[serde(default)]
    suggested_summary: Option<AiSummary>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum RawInsight {
    Structured(AiInsight),
    Plain(String),
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
    pub insight: Option<String>,
    #[serde(default)]
    pub insight_kind: Option<String>,
    #[serde(default)]
    pub insight_params: Option<serde_json::Value>,
    #[serde(default)]
    pub x_col: Option<String>,
    #[serde(default)]
    pub y_col: Option<String>,
    #[serde(default)]
    pub sort: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Structured standalone insight from AI (internal parsing only).
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct AiInsight {
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub params: Option<serde_json::Value>,
    #[serde(default)]
    pub text: Option<String>,
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
    pub sample_rows: Vec<Vec<String>>,
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
                    if !values.is_empty() {
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

/// Helper: find a column index by name.
fn col_index(columns: &[ColumnMeta], name: &str) -> Option<usize> {
    columns.iter().position(|c| c.name == name)
}

/// Helper: extract numeric values for a column from sample rows.
fn numeric_values(columns: &[ColumnMeta], sample_rows: &[Vec<String>], col_name: &str) -> Vec<f64> {
    let idx = match col_index(columns, col_name) { Some(i) => i, None => return vec![] };
    sample_rows.iter()
        .filter_map(|row| row.get(idx)?.parse::<f64>().ok())
        .collect()
}

/// Helper: aggregate numeric values by a group column.
fn aggregate_by_group(
    columns: &[ColumnMeta],
    sample_rows: &[Vec<String>],
    metric: &str,
    group_by: &str,
    agg: &str,
) -> Vec<(String, f64)> {
    let metric_idx = match col_index(columns, metric) { Some(i) => i, None => return vec![] };
    let group_idx = match col_index(columns, group_by) { Some(i) => i, None => return vec![] };

    let mut groups: std::collections::HashMap<String, Vec<f64>> = std::collections::HashMap::new();
    for row in sample_rows {
        let key = match row.get(group_idx) { Some(k) if !k.is_empty() => k.clone(), _ => continue };
        let val = match row.get(metric_idx).and_then(|v| v.parse::<f64>().ok()) { Some(v) => v, None => continue };
        groups.entry(key).or_default().push(val);
    }

    let mut result: Vec<(String, f64)> = groups.into_iter().map(|(key, vals)| {
        let value = match agg {
            "Avg" => vals.iter().sum::<f64>() / vals.len() as f64,
            "Count" => vals.len() as f64,
            "Min" => vals.iter().cloned().fold(f64::INFINITY, f64::min),
            "Max" => vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
            _ => vals.iter().sum::<f64>(), // Sum
        };
        (key, value)
    }).collect();
    result.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    result
}

/// Format a number for display (drop decimals if integer-like).
fn fmt_num(v: f64) -> String {
    if v.abs() >= 1000.0 {
        // Add comma separators
        let s = if v == v.round() { format!("{}", v as i64) } else { format!("{:.2}", v) };
        let parts: Vec<&str> = s.splitn(2, '.').collect();
        let int_part = parts[0];
        let negative = int_part.starts_with('-');
        let digits: &str = if negative { &int_part[1..] } else { int_part };
        let mut result = String::new();
        for (i, c) in digits.chars().rev().enumerate() {
            if i > 0 && i % 3 == 0 { result.push(','); }
            result.push(c);
        }
        let formatted: String = result.chars().rev().collect();
        let formatted = if negative { format!("-{}", formatted) } else { formatted };
        if parts.len() > 1 { format!("{}.{}", formatted, parts[1]) } else { formatted }
    } else if v == v.round() {
        format!("{}", v as i64)
    } else {
        format!("{:.2}", v)
    }
}

/// Compute an insight from the model's structured suggestion using actual data.
pub fn realize_insight(
    kind: &str,
    params: &serde_json::Value,
    sample_rows: &[Vec<String>],
    columns: &[ColumnMeta],
) -> Option<String> {
    let get_str = |key: &str| params.get(key).and_then(|v| v.as_str());
    let get_usize = |key: &str| params.get(key).and_then(|v| v.as_u64()).map(|v| v as usize);

    match kind {
        "top_value" => {
            let metric = get_str("metric")?;
            let group_by = get_str("group_by")?;
            let agg = get_str("aggregate").unwrap_or("Sum");
            let groups = aggregate_by_group(columns, sample_rows, metric, group_by, agg);
            let (top_name, top_val) = groups.first()?;
            Some(format!("{} has the highest {} ({}) at {}", top_name, metric, agg.to_lowercase(), fmt_num(*top_val)))
        }
        "compare_two" => {
            let metric = get_str("metric")?;
            let group_by = get_str("group_by")?;
            let agg = get_str("aggregate").unwrap_or("Sum");
            let groups = aggregate_by_group(columns, sample_rows, metric, group_by, agg);
            if groups.len() < 2 { return None; }
            let (a_name, a_val) = &groups[0];
            let (b_name, b_val) = &groups[1];
            if *b_val == 0.0 { return None; }
            let pct = ((a_val - b_val) / b_val * 100.0).round();
            Some(format!("{}'s {} ({}) is {}% higher than {}'s ({})",
                a_name, metric, fmt_num(*a_val), pct, b_name, fmt_num(*b_val)))
        }
        "distribution" => {
            let metric = get_str("metric")?;
            let vals = numeric_values(columns, sample_rows, metric);
            if vals.is_empty() { return None; }
            let min = vals.iter().cloned().fold(f64::INFINITY, f64::min);
            let max = vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let mean = vals.iter().sum::<f64>() / vals.len() as f64;
            Some(format!("{} ranges from {} to {} (mean {})", metric, fmt_num(min), fmt_num(max), fmt_num(mean)))
        }
        "concentration" => {
            let metric = get_str("metric")?;
            let group_by = get_str("group_by")?;
            let agg = get_str("aggregate").unwrap_or("Sum");
            let n = get_usize("n").unwrap_or(3);
            let groups = aggregate_by_group(columns, sample_rows, metric, group_by, agg);
            let total: f64 = groups.iter().map(|(_, v)| v).sum();
            if total == 0.0 { return None; }
            let top_n: f64 = groups.iter().take(n).map(|(_, v)| v).sum();
            let pct = (top_n / total * 100.0).round();
            let top_names: Vec<&str> = groups.iter().take(n).map(|(k, _)| k.as_str()).collect();
            Some(format!("Top {} {} ({}) account for {}% of total {} ({})",
                n, group_by, top_names.join(", "), pct, metric, fmt_num(total)))
        }
        "count" => {
            let column = get_str("column")?;
            let col = columns.iter().find(|c| c.name == column)?;
            Some(format!("There are {} unique {} values", col.unique_count, column))
        }
        _ => None,
    }
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

    /// Run a prompt through the model and return raw text output.
    fn run_prompt(&self, prompt: &str) -> Result<String, String> {
        let model = self.model.as_ref().ok_or("Model not loaded")?;
        let backend = self.backend.as_ref().ok_or("Backend not initialized")?;

        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(CONTEXT_SIZE));

        let mut ctx = model
            .new_context(backend, ctx_params)
            .map_err(|e| format!("Failed to create context: {}", e))?;

        let tokens = model
            .str_to_token(prompt, AddBos::Always)
            .map_err(|e| format!("Tokenization failed: {}", e))?;

        let max_prompt_tokens = CONTEXT_SIZE as usize - 256;
        if tokens.len() > max_prompt_tokens {
            return Err(format!(
                "Prompt too long ({} tokens, max {}).",
                tokens.len(), max_prompt_tokens
            ));
        }

        let mut batch = LlamaBatch::new(CONTEXT_SIZE as usize, 1);
        let last_idx = tokens.len() - 1;
        for (i, &token) in tokens.iter().enumerate() {
            batch
                .add(token, i as i32, &[0], i == last_idx)
                .map_err(|e| format!("Failed to add token to batch: {}", e))?;
        }

        ctx.decode(&mut batch)
            .map_err(|e| format!("Prompt decode failed: {}", e))?;

        let mut sampler = LlamaSampler::chain_simple([
            LlamaSampler::temp(0.1),
            LlamaSampler::greedy(),
        ]);

        let mut output_tokens = Vec::new();
        let mut n_cur = tokens.len();

        for _ in 0..MAX_GENERATE_TOKENS {
            let new_token = sampler.sample(&ctx, batch.n_tokens() - 1);
            if model.is_eog_token(new_token) {
                break;
            }
            output_tokens.push(new_token);
            batch.clear();
            batch
                .add(new_token, n_cur as i32, &[0], true)
                .map_err(|e| format!("Failed to add generated token: {}", e))?;
            n_cur += 1;
            ctx.decode(&mut batch)
                .map_err(|e| format!("Decode failed: {}", e))?;
        }

        let mut output = String::new();
        let mut decoder = encoding_rs::UTF_8.new_decoder();
        for token in &output_tokens {
            let piece = model
                .token_to_piece(*token, &mut decoder, false, None)
                .map_err(|e| format!("Token decode failed: {}", e))?;
            output.push_str(&piece);
        }

        Ok(output)
    }

    pub fn generate(&self, context: &CsvContext) -> Result<AiInsightsResponse, String> {
        let prompt = build_prompt(context);
        let output = self.run_prompt(&prompt)?;
        let mut response = parse_ai_response(&output, &context.columns)?;
        realize_all_insights(&mut response, &context.sample_rows, &context.columns);
        Ok(response)
    }

    pub fn generate_sql(&self, question: &str, columns: &[String], table_name: &str) -> Result<String, String> {
        let mut prompt = String::new();
        prompt.push_str("<start_of_turn>user\n");
        prompt.push_str("Translate this question to a DuckDB SQL query.\n\n");
        prompt.push_str(&format!("Table: {}\nColumns: {}\n\n", table_name, columns.join(", ")));
        prompt.push_str(&format!("Question: {}\n\n", question));
        prompt.push_str("Respond with ONLY the SQL query, no explanation, no markdown.\n");
        prompt.push_str("<end_of_turn>\n<start_of_turn>model\n");

        let output = self.run_prompt(&prompt)?;
        // Clean up: strip markdown fences, whitespace
        let sql = output.trim()
            .trim_start_matches("```sql")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim()
            .to_string();
        Ok(sql)
    }
}

/// Build the prompt for the AI model.
pub fn build_prompt(context: &CsvContext) -> String {
    let mut prompt = String::new();

    prompt.push_str("<start_of_turn>user\n");
    prompt.push_str("You are a data analyst looking for SURPRISING findings — things a user wouldn't notice by glancing at the data.\n\n");

    prompt.push_str(&format!("## Dataset: {} ({} rows)\n\n", context.filename, context.row_count));

    prompt.push_str("## Column Statistics:\n");
    // Intelligent column selection to fit context window:
    // 1. Skip empty columns (0 unique values)
    // 2. Skip ID/code columns (names ending in ID/Id/_id, or abbreviation variants)
    // 3. Cap at 15 columns
    let all_names: std::collections::HashSet<&str> = context.summaries.iter()
        .map(|s| s.name.as_str()).collect();
    // Build a set of (name, unique_count) for dedup detection
    let name_counts: Vec<(&str, usize)> = context.summaries.iter()
        .map(|s| (s.name.as_str(), s.unique_count)).collect();
    let useful_summaries: Vec<&ColumnSummary> = context.summaries.iter()
        .filter(|s| {
            if s.unique_count == 0 { return false; }
            let name = &s.name;
            let lower = name.to_lowercase();
            // Skip columns whose name ends with "ID"/"Id"/"_id"
            if lower.ends_with("_id") || (lower.ends_with("id") && name.len() > 2) {
                let base = name.trim_end_matches("ID").trim_end_matches("Id").trim_end_matches("_id");
                if !base.is_empty() {
                    // Check for any descriptive counterpart: Base, BaseDesc, BaseAbbr
                    let has_counterpart = all_names.contains(base)
                        || all_names.contains(format!("{}Desc", base).as_str())
                        || all_names.contains(format!("{}Abbr", base).as_str());
                    if has_counterpart {
                        return false;
                    }
                }
            }
            // Skip footnote/note/comment columns — metadata noise
            if lower.contains("footnote") || lower.contains("_note") || lower.ends_with("note")
                || lower.contains("comment") || lower.contains("_url") || lower.ends_with("url") {
                return false;
            }
            // Skip abbreviation columns if descriptive version exists
            if lower.ends_with("abbr") || lower.ends_with("abbrev") {
                let base = name.trim_end_matches("Abbr").trim_end_matches("Abbrev").trim_end_matches("abbr");
                let desc_name = format!("{}Desc", base);
                if all_names.contains(desc_name.as_str()) {
                    return false;
                }
            }
            // Skip duplicate-value columns (e.g., DataValueAlt has same unique_count as DataValue)
            if name.contains("Alt") || name.contains("Symbol") {
                let base = name.replace("Alt", "").replace("Symbol", "");
                if name_counts.iter().any(|(n, c)| *n == base && *c == s.unique_count) {
                    return false;
                }
            }
            true
        })
        .take(15)
        .collect();
    for s in &useful_summaries {
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
    prompt.push_str("Suggest 1-3 NEW visualizations NOT already shown.\n");
    prompt.push_str("Focus on NON-OBVIOUS findings. Skip things that are trivially true:\n");
    prompt.push_str("- Do NOT suggest scatter plots between metrics that are mathematically related (e.g. revenue vs orders, price vs total)\n");
    prompt.push_str("- Do NOT state that a larger group has a larger total — that's obvious\n");
    prompt.push_str("- DO look for: outliers, uneven distributions, surprising category leaders, unexpected ratios (e.g. highest revenue per order, not highest total revenue)\n");
    prompt.push_str("- Prefer Avg or per-unit metrics over Sum when comparing groups of different sizes\n\n");
    prompt.push_str("For each view and as standalone insights, use structured insight_kind + insight_params. Do NOT write specific numbers — they will be computed from data.\n\n");
    prompt.push_str("Required fields by view_type:\n");
    prompt.push_str("- BarChart/LineChart/BoxPlot: metric, group_by, aggregate\n");
    prompt.push_str("- ScatterPlot: x_col, y_col (both numeric)\n");
    prompt.push_str("- Histogram: metric\n");
    prompt.push_str("- Heatmap: x_col, y_col (both categorical), metric, aggregate\n");
    prompt.push_str("- CountBar/OrderedBar: group_by\n");
    prompt.push_str("- Choropleth: metric, group_by (geographic column), aggregate\n");
    prompt.push_str("Valid aggregate: Sum, Avg, Count, Min, Max\n\n");
    prompt.push_str("insight_kind options:\n");
    prompt.push_str("- top_value: {metric, group_by, aggregate} → \"X has highest Y\"\n");
    prompt.push_str("- compare_two: {metric, group_by, aggregate} → \"X is N% higher than Y\"\n");
    prompt.push_str("- distribution: {metric} → \"Y ranges from A to B\"\n");
    prompt.push_str("- concentration: {metric, group_by, aggregate, n} → \"Top N account for P%\"\n");
    prompt.push_str("- count: {column} → \"N unique values\"\n\n");
    prompt.push_str("Also include:\n");
    prompt.push_str("- suggested_title: a short, descriptive notebook title (3-6 words)\n");
    prompt.push_str("- suggested_summary: {title, metric, aggregate, denominator?} — the most interesting single number. For simple aggregates: {title:\"Total Revenue\", metric:\"revenue\", aggregate:\"Sum\"}. For ratios: {title:\"Revenue Per Order\", metric:\"revenue\", aggregate:\"Sum\", denominator:\"orders\"} — Rust will compute Sum(metric)/Sum(denominator).\n\n");
    prompt.push_str("Respond with ONLY valid JSON. Example:\n");
    prompt.push_str(r#"{"suggested_title":"Product Revenue Analysis","suggested_summary":{"title":"Revenue Per Order","metric":"revenue","aggregate":"Sum","denominator":"orders"},"suggested_views":[{"view_type":"BoxPlot","title":"Revenue spread by product","metric":"revenue","group_by":"product","aggregate":"Sum","insight_kind":"compare_two","insight_params":{"metric":"revenue","group_by":"product","aggregate":"Sum"}}],"insights":[{"kind":"concentration","params":{"metric":"revenue","group_by":"product","aggregate":"Sum","n":3}}]}"#);
    prompt.push_str("\n\nONLY use column names from the dataset.\n");
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
        &s.metric, &s.group_by, &s.x_col, &s.y_col,
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

    // Try parsing as RawAiResponse first (handles structured insights)
    let raw_response = serde_json::from_str::<RawAiResponse>(&json_str).map_err(|e| {
        format!(
            "Failed to parse AI response as JSON: {}. Raw output: {}",
            e,
            &raw[..raw.len().min(500)]
        )
    })?;

    // Convert RawInsights to plain strings (structured ones get a placeholder that realize_all_insights fills)
    let insights: Vec<String> = raw_response.insights.into_iter().filter_map(|i| {
        match i {
            RawInsight::Plain(s) => Some(s),
            RawInsight::Structured(ai) => ai.text,
        }
    }).collect();

    // Validate suggested_summary references real columns
    let suggested_summary = raw_response.suggested_summary.filter(|s| {
        let metric_exists = columns.iter().any(|c| c.name == s.metric);
        let denom_exists = s.denominator.as_ref()
            .map(|d| columns.iter().any(|c| c.name == *d))
            .unwrap_or(true);
        metric_exists && denom_exists
    });

    let mut response = AiInsightsResponse {
        suggested_views: raw_response.suggested_views,
        insights,
        suggested_title: raw_response.suggested_title,
        suggested_summary,
    };

    response.suggested_views.retain(|s| validate_ai_suggestion(s, columns));

    Ok(response)
}

/// Replace structured insight_kind/insight_params with realized text on all views.
fn realize_all_insights(response: &mut AiInsightsResponse, sample_rows: &[Vec<String>], columns: &[ColumnMeta]) {
    for view in &mut response.suggested_views {
        if let (Some(kind), Some(params)) = (&view.insight_kind, &view.insight_params) {
            if let Some(realized) = realize_insight(kind, params, sample_rows, columns) {
                view.insight = Some(realized);
            }
        }
        // If no insight_kind but insight is present, keep the free text (fallback)
    }
}

/// Extract JSON from model output, stripping markdown fences and prefix text.
/// Uses serde's streaming deserializer to find the first valid JSON object.
fn extract_json(raw: &str) -> String {
    let trimmed = raw.trim();

    // Find the first '{' — skip markdown fences, prefix text, etc.
    let start = match trimmed.find('{') {
        Some(i) => i,
        None => return trimmed.to_string(),
    };

    let json_candidate = &trimmed[start..];

    // Use serde's streaming deserializer to parse exactly one JSON value
    let mut de = serde_json::Deserializer::from_str(json_candidate).into_iter::<serde_json::Value>();
    if let Some(Ok(val)) = de.next() {
        val.to_string()
    } else {
        // Fallback: return from first '{' to end (let caller handle the error)
        json_candidate.to_string()
    }
}

/// Read all rows from a CSV file (up to max_rows) for accurate computation.
fn read_all_rows(path: &str, max_rows: usize) -> Result<Vec<Vec<String>>, String> {
    let mut rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .from_path(path)
        .map_err(|e| format!("Failed to open CSV: {}", e))?;

    let mut rows = Vec::new();
    for result in rdr.records() {
        let record = result.map_err(|e| format!("Failed to read record: {}", e))?;
        rows.push(record.iter().map(|f| f.to_string()).collect());
        if rows.len() >= max_rows {
            break;
        }
    }
    Ok(rows)
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

    // Read all rows for accurate insight computation (up to 50K)
    let all_rows = read_all_rows(&path, 50_000)?;

    let summaries = compute_summaries(&parsed.columns, &all_rows);

    let context = CsvContext {
        filename: path
            .split('/')
            .last()
            .unwrap_or(&path)
            .to_string(),
        row_count: parsed.row_count,
        columns: parsed.columns.clone(),
        summaries,
        sample_rows: all_rows,
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

#[tauri::command]
pub async fn natural_language_to_sql(
    question: String,
    columns: Vec<String>,
    table_name: String,
    app: AppHandle,
) -> Result<String, String> {
    let engine = app.state::<Arc<Mutex<InsightEngine>>>();
    let engine = engine.lock().await;
    if !engine.is_loaded() {
        return Err("AI model not loaded".to_string());
    }
    tokio::task::block_in_place(|| engine.generate_sql(&question, &columns, &table_name))
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
            sample_rows: vec![
                vec!["2024-01-01".into(), "1200.50".into(), "Widget".into()],
                vec!["2024-01-02".into(), "3400.00".into(), "Gadget".into()],
                vec!["2024-01-03".into(), "800.00".into(), "Widget".into()],
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

    fn parse_json(s: &str) -> serde_json::Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn test_extract_json_plain() {
        let input = r#"{"a": 1}"#;
        assert_eq!(parse_json(&extract_json(input)), parse_json(r#"{"a": 1}"#));
    }

    #[test]
    fn test_extract_json_with_prefix() {
        let input = "Here is the JSON:\n{\"a\": 1}";
        assert_eq!(parse_json(&extract_json(input)), parse_json(r#"{"a": 1}"#));
    }

    #[test]
    fn test_extract_json_nested() {
        let input = r#"{"outer": {"inner": 1}}"#;
        assert_eq!(parse_json(&extract_json(input)), parse_json(r#"{"outer": {"inner": 1}}"#));
    }

    #[test]
    fn test_extract_json_with_trailing_text() {
        let input = r#"{"a": 1} some trailing garbage"#;
        assert_eq!(parse_json(&extract_json(input)), parse_json(r#"{"a": 1}"#));
    }

    #[test]
    fn test_extract_json_markdown_fence() {
        let input = "```json\n{\"a\": 1}\n```";
        assert_eq!(parse_json(&extract_json(input)), parse_json(r#"{"a": 1}"#));
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
            sample_rows,
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

    // --- realize_insight tests ---

    fn test_rows() -> Vec<Vec<String>> {
        vec![
            vec!["2024-01-01".into(), "100.00".into(), "Widget".into()],
            vec!["2024-01-02".into(), "200.00".into(), "Widget".into()],
            vec!["2024-01-03".into(), "150.00".into(), "Gadget".into()],
            vec!["2024-01-04".into(), "50.00".into(), "Gadget".into()],
        ]
    }

    fn test_col_metas() -> Vec<ColumnMeta> {
        vec![
            ColumnMeta { name: "date".into(), col_type: ColumnType::Temporal, unique_count: 4, total_count: 4, sample_values: vec![] },
            ColumnMeta { name: "revenue".into(), col_type: ColumnType::NumericMeasure, unique_count: 4, total_count: 4, sample_values: vec![] },
            ColumnMeta { name: "product".into(), col_type: ColumnType::Categorical, unique_count: 2, total_count: 4, sample_values: vec![] },
        ]
    }

    #[test]
    fn test_realize_top_value() {
        let params = serde_json::json!({"metric": "revenue", "group_by": "product", "aggregate": "Sum"});
        let result = realize_insight("top_value", &params, &test_rows(), &test_col_metas());
        assert!(result.is_some());
        let text = result.unwrap();
        assert!(text.contains("Widget"), "Should identify Widget as top: {}", text);
        assert!(text.contains("300"), "Widget sum should be 300: {}", text);
    }

    #[test]
    fn test_realize_compare_two() {
        let params = serde_json::json!({"metric": "revenue", "group_by": "product", "aggregate": "Sum"});
        let result = realize_insight("compare_two", &params, &test_rows(), &test_col_metas());
        assert!(result.is_some());
        let text = result.unwrap();
        assert!(text.contains("Widget"), "{}", text);
        assert!(text.contains("Gadget"), "{}", text);
        assert!(text.contains("50%"), "300 is 50% more than 200: {}", text);
    }

    #[test]
    fn test_realize_distribution() {
        let params = serde_json::json!({"metric": "revenue"});
        let result = realize_insight("distribution", &params, &test_rows(), &test_col_metas());
        assert!(result.is_some());
        let text = result.unwrap();
        assert!(text.contains("50"), "Min should be 50: {}", text);
        assert!(text.contains("200"), "Max should be 200: {}", text);
        assert!(text.contains("125"), "Mean should be 125: {}", text);
    }

    #[test]
    fn test_realize_concentration() {
        let params = serde_json::json!({"metric": "revenue", "group_by": "product", "aggregate": "Sum", "n": 1});
        let result = realize_insight("concentration", &params, &test_rows(), &test_col_metas());
        assert!(result.is_some());
        let text = result.unwrap();
        assert!(text.contains("60%"), "Widget (300/500) = 60%: {}", text);
    }

    #[test]
    fn test_realize_count() {
        let params = serde_json::json!({"column": "product"});
        let result = realize_insight("count", &params, &test_rows(), &test_col_metas());
        assert!(result.is_some());
        let text = result.unwrap();
        assert!(text.contains("2"), "Should have 2 unique products: {}", text);
    }

    #[test]
    fn test_realize_invalid_column() {
        let params = serde_json::json!({"metric": "nonexistent", "group_by": "product", "aggregate": "Sum"});
        let result = realize_insight("top_value", &params, &test_rows(), &test_col_metas());
        assert!(result.is_none());
    }

    #[test]
    fn test_realize_unknown_kind() {
        let params = serde_json::json!({});
        let result = realize_insight("unknown_kind", &params, &test_rows(), &test_col_metas());
        assert!(result.is_none());
    }

    #[test]
    fn test_fmt_num_commas() {
        assert_eq!(fmt_num(245000.0), "245,000");
        assert_eq!(fmt_num(1200.50), "1,200.50");
        assert_eq!(fmt_num(42.0), "42");
        assert_eq!(fmt_num(3.14), "3.14");
    }
}
