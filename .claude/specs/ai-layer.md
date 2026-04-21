# AI Layer — Architecture Spec

## Philosophy
AI insights are an enhancement, never a dependency. The app must
work fully without them. If the model is downloading or unavailable,
all rule-based features continue normally with no errors shown.

## Runtime
llama.cpp via the `llama-cpp-2` Rust crate (or current best-maintained
llama-cpp crate on crates.io — check before building).
- No external dependency
- No Ollama required
- No separate process
- Runs inside the Tauri backend
- Model loaded once at startup, reused across inference calls

## Model
**Gemma 3 4B Instruct Q4_K_M**
- Size: ~2.5GB on disk
- RAM: works on 8GB (fits comfortably on M-series Air)
- Speed: fast on Apple Silicon (M1/M2/M3/M4)
- Structured JSON output: better instruction following than 2B
- Source: Hugging Face — bartowski/google_gemma-3-4b-it-GGUF
- Stored at: `{app_data_dir}/lede/models/gemma-3-4b-it-Q4_K_M.gguf`

## Two-Phase Rendering

### Phase 1 (instant, always)
- Rule-based suggestion engine runs in Rust
- Results render immediately on CSV drop
- No AI involvement
- Works with or without model downloaded

### Phase 2 (2-5 seconds later, when model available)
- Frontend calls `generate_ai_insights` after Phase 1 renders
- AI returns better view suggestions + standalone insights
- Frontend updates view titles and adds insight text
- If AI call fails or times out (>10s): Phase 1 unchanged, no error shown

## Model Download

### Trigger
Automatic on first launch. No user action required.

### UX
- Subtle progress banner at bottom of home screen
- "Setting up AI insights... [████████░░] 73% · 400MB remaining"
- Non-blocking — app fully usable during download
- On completion: banner disappears silently
- On error: silent — AI features just don't appear

### Storage
- Downloads to `{app_data_dir}/lede/models/`
- Downloaded once, persists across restarts
- Never re-downloads unless file is missing or corrupted

### Tauri Events
```
model-download-progress  → f32 (0.0 to 1.0)
model-download-complete  → ()
model-download-error     → String
```

## Tauri Commands

```rust
#[tauri::command]
async fn check_model_status(app: AppHandle) -> ModelStatus
// { downloaded: bool, downloading: bool, progress: f32 }

#[tauri::command]
async fn start_model_download(app: AppHandle) -> Result<(), String>

#[tauri::command]
async fn generate_ai_insights(
    path: String,
    app: AppHandle,
) -> Result<AiInsightsResponse, String>
```

## Input to Model (CsvContext)

```rust
pub struct CsvContext {
    pub filename: String,
    pub row_count: usize,
    pub columns: Vec<ColumnMeta>,       // name + type from classifier
    pub data_rows: Vec<Vec<String>>,    // up to MAX_INSIGHT_ROWS (50k), for stats + realization
    pub current_views: Vec<String>,     // titles of Phase 1 views already shown
}
```

## Prompt Template

```
You are a data analyst helping a non-technical user understand their data.

CSV file: {filename} ({row_count} rows)

Columns:
{for each column: "- {name} ({type}): sample values: {first 5 unique values}"}

Sample data (first 10 rows):
{csv_rows_as_csv_text}

Views already shown to the user: {current_view_titles}

Return ONLY valid JSON matching this exact schema, no other text:
{
  "suggested_views": [
    {
      "view": "bar_chart|line_chart|scatter_plot|histogram|summary_card|ranked_list|heatmap|table|count_bar",
      "metric": "column_name_or_null",
      "aggregate": "sum|avg|count|min|max",
      "group_by": "column_name_or_null",
      "time_column": "column_name_or_null",
      "title": "Human readable title, no sum of prefix",
      "insight": "One specific observation about what this view reveals"
    }
  ],
  "insights": [
    "Specific observation with actual values from the data",
    "Specific observation with actual values from the data"
  ]
}

Rules:
- suggested_views: 3-5 views, different from views already shown
- insights: exactly 2-3, each mentioning specific values from the data
- insights must be surprising — not obvious from column names alone
- Never say "this data shows" — state the finding directly
- Only use column names that actually exist in the data above
- Return ONLY the JSON object, no markdown formatting, no explanation
```

## Output Schema

```rust
pub struct AiInsightsResponse {
    pub suggested_views: Vec<ViewSuggestion>, // same struct as rule-based
    pub insights: Vec<String>,
}
```

## Insight Quality Rules

Insights must be:
- **Specific** — reference actual values from the data
- **Surprising** — not obvious from column names alone
- **Actionable** — something the user could investigate or act on

Never generate:
- "This data shows revenue over time" (obvious, vague)
- "The data contains information about products" (useless)

Always generate:
- "Widget Pro generates 40% more revenue per order than Widget Lite"
- "Revenue dropped 67% in the week of Feb 2 — worth investigating"
- "3 customers account for 45% of total revenue"

## Frontend Rendering of AI Content

AI-generated text is visually distinct from user prose:
- Italic style
- Muted color (--color-text-secondary)
- Small "✦ AI" badge
- Fully editable — user can click, modify, or delete
- `insight_edited: true` set in .lede file when user modifies

## Error Handling

If JSON parsing fails: return error, frontend falls back to Phase 1 silently.
If inference times out (>10s): cancel, frontend falls back to Phase 1 silently.
If model not loaded: return error immediately, frontend falls back silently.
Never show an error to the user for AI failures — just omit AI content.

## Testing

Do NOT test actual model inference in unit tests — too slow, requires model file.
Mock the inference call in tests.

Test:
- `model_manager.is_downloaded()` returns false when file absent
- Prompt building includes correct column names and sample rows
- JSON parsing succeeds with valid response
- JSON parsing fails gracefully with malformed response
- Fallback: generate_ai_insights returns error when model not loaded
