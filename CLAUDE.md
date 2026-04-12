# Lede — Project Constitution

## What is Lede

Lede is a persistent local desktop workspace for data curiosity. Users drag in a CSV, get instant visual views, write narrative around them, and return tomorrow to find everything still there.

Tagline: "Don't bury the lede in your data."

## Core Architectural Principle

**Rust owns the data. Frontend owns the pixels.**

- All CSV parsing, column classification, and aggregation happens in Rust
- Frontend (React/TypeScript) only receives pre-computed numbers ready to render
- Never send raw CSV data to the frontend
- Never do data computation in JavaScript

## Tech Stack

- **Tauri** — desktop shell
- **Rust** — all data logic, via the `csv` and `serde` crates
- **React + TypeScript** — UI only
- **DuckDB-WASM** — preloaded on app start, used for view queries
- **Observable Plot** — chart rendering only
- **Tiptap** — prose editor
- All Tauri commands are the API boundary between Rust and frontend

## File Format

Files saved as `.lede` — markdown with YAML frontmatter and fenced `dataview` blocks.

Example:
````
---
title: Q1 Sales Analysis
created: 2025-04-10
sources:
  - sales_2025.csv
---

# Q1 Sales Analysis

Revenue was up this quarter.

```dataview
source: sales_2025.csv
view: bar_chart
metric: revenue
aggregate: sum
group_by: region
sort: desc
limit: 10
suggested_by: rule
last_verified: 2025-04-10
insight: null
```
````

## Column Types (Rust classifier)

- `temporal` — dates, timestamps
- `categorical` — text, or numeric where unique_values/total_rows < 0.05
- `identifier` — high cardinality text, never group by
- `numeric_measure` — numbers to compute against
  - subtype `ratio` — values 0-1 or 0-100 with name containing [rate, pct, percent, share, ratio] → default aggregate: avg
  - subtype `signed` — contains negative values
- `geographic` — categorical where name is in [state, country, city, zip, region, country_code, lat, lng]
- `ordered_categorical` — values match known ordinal sets (Likert, star ratings, etc.)

## View Types

- `bar_chart` — composition (categorical + numeric_measure)
- `line_chart` — trend (temporal + numeric_measure)
- `multi_line_chart` — multiple metrics on same time axis
- `scatter_plot` — relationship (two numeric_measures)
- `histogram` — distribution (single numeric_measure)
- `summary_card` — single value
- `ranked_list` — top N by metric
- `heatmap` — two categoricals + numeric_measure
- `table` — always available, raw data
- `count_bar` — when no numeric columns exist

## Suggestion Algorithm (Rust, max 5 suggestions)

Guards first:
- categorical.unique_count > 50 → no bar_chart
- categorical.unique_count > 20 → ranked_list with limit:10 instead
- temporal.count > 1 → use column named [date, created_at, timestamp], ignore others
- numeric_measures.count > 2 → scatter only highest-variance pair
- columns match [prefix_jan, prefix_feb...] → flag wide format
- ratio column → aggregate: avg not sum

Priority order:
1. Always: table + summary_card
2. Strong: line_chart (temporal + numeric), bar_chart (categorical + numeric)
3. Weaker: count_bar (no numeric), scatter (2+ numeric), histogram (single numeric)
4. Conditional: heatmap (2 categoricals + numeric), ordered_bar (ordinal)

## Tauri Command Boundary

```rust
#[tauri::command]
fn parse_csv(path: String) -> ParsedCSV
// Returns: column metadata, row count, sample rows, suggested views with pre-aggregated data

#[tauri::command]
fn run_view_query(path: String, view_config: ViewConfig) -> ViewResult
// Returns: aggregated data ready for Observable Plot
```

## Performance Requirements

- DuckDB-WASM preloads silently on app open — before any file is dropped
- Something useful renders in under 1 second for files under 50MB
- Render progressively — summary_card first, then charts
- Rust computes all aggregates; frontend receives only the numbers it needs

## Testing Requirements

- Every Rust function has unit tests
- Test against three canonical CSVs in `tests/fixtures/`:
  - `shopify.csv` — date, product, region, revenue, orders
  - `marketing.csv` — date, campaign, impressions, clicks, spend, conversions
  - `survey.csv` — respondent_id, age, satisfaction (Likert), nps_score
- Tests must pass before any feature is considered complete
- Run `cargo test` after every Rust change

## Verification Rules

You cannot reliably verify GUI output. For any step requiring 
visual confirmation, list exactly what the user should check 
and what correct behavior looks like. Never mark a UI step 
complete based on compilation or test results alone.

## UI Decisions

Make all UI decisions independently. Do not ask for approval on visual choices. Pick an aesthetic that is minimal, calm, and focused — this is a thinking tool, not a dashboard. Use muted colors, generous whitespace, a good monospace font for data, a readable serif or sans for prose.

## Git Discipline

- Commit after every passing test suite
- Commit message format: `feat: [what]` / `fix: [what]` / `test: [what]`
- Never commit with failing tests
- Commit at the end of every session with message: "feat: session N complete — [one line summary]"
- Never leave a session with uncommitted work.
