# View Ontology + Suggestion Algorithm

## View Types

### bar_chart — Composition (what is a whole made of?)
```yaml
view: bar_chart
metric: revenue          # NumericMeasure column
aggregate: sum | avg | count | min | max
group_by: region         # Categorical or Geographic column
sort: desc | asc | natural
limit: 10
normalize: false         # true = 100% stacked
```

### line_chart — Trend (how does something change over time?)
```yaml
view: line_chart
metric: revenue
aggregate: sum
time_column: date        # Temporal column
time_grain: day | week | month | quarter | year
compare: previous_period # optional second line
```

### multi_line_chart — Multiple metrics on same time axis
```yaml
view: multi_line_chart
metrics: [revenue, orders]
aggregate: sum
time_column: date
time_grain: month
```

### scatter_plot — Relationship (how do two things correlate?)
```yaml
view: scatter_plot
x_metric: ad_spend
y_metric: revenue
color_by: region         # optional
size_by: orders          # optional
```

### histogram — Distribution (what is the shape of variation?)
```yaml
view: histogram
metric: order_value
bucket_count: 20         # auto if omitted
```

### summary_card — Single value
```yaml
view: summary_card
metric: revenue
aggregate: sum
compare: previous_period # optional delta
target_value: 1000000    # optional
```

### ranked_list — Top N by metric
```yaml
view: ranked_list
metric: revenue
aggregate: sum
group_by: product
limit: 5
show_delta: true
```

### heatmap — Two categoricals + one metric
```yaml
view: heatmap
x_axis: month
y_axis: region
metric: revenue
aggregate: sum
```

### table — Raw data (always available)
```yaml
view: table
columns: [date, region, revenue, orders]
filter: "region = 'West'"
sort_by: revenue
sort: desc
limit: 50
```

### count_bar — When no numeric columns exist
```yaml
view: count_bar
group_by: category
sort: desc
limit: 10
```

### ordered_bar — Ordinal categories (Likert etc.)
```yaml
view: ordered_bar
group_by: satisfaction   # OrderedCategorical column
metric: null             # count of rows per value
sort: natural            # preserve ordinal order
```

## Universal Optional Fields
```yaml
filter: "region = 'West' AND date > '2025-01-01'"
title: "Custom title"    # overrides auto-generated
color_scheme: default | diverging | categorical
```

## View Metadata (on every view)
```yaml
suggested_by: rule | ai | user
last_verified: 2025-04-10
insight: null            # AI writes here in Phase 2
insight_edited: false    # true if user modified AI text
```

## Chart Title Rules
Generated titles must be human-readable:
- Drop aggregate prefix: "sum of revenue" → "Revenue"
- Capitalize first word only
- Temporal: append "over time" not "by date/month/year"
- Count (no metric): "Category breakdown" not "count of category"
- Examples:
  - sum of revenue by region → "Revenue by region"
  - sum of revenue by date → "Revenue over time"
  - count by satisfaction → "Satisfaction breakdown"

## Suggestion Algorithm

### Step 1: Guards (eliminate before suggesting)
```
categorical.unique_count > 50     → no bar_chart (use ranked_list instead)
categorical.unique_count > 20     → ranked_list with limit: 10
temporal.count > 1                → prefer column named [date, created_at, timestamp]
numeric_measures.count > 2        → scatter only highest-variance pair
columns match [prefix_jan, feb..] → flag wide format, don't suggest charts
ratio column                      → aggregate: avg not sum
empty column (unique_count == 0)  → skip entirely
identifier column                 → never group by, never use as metric
```

### Step 2: Priority Rules (in order, cap at 5 total)
```
Priority 1 — Always suggest:
  → table
  → summary_card (highest-variance NumericMeasure)

Priority 2 — Strong signal:
  → temporal + NumericMeasure       → line_chart (or multi_line_chart if 2+ measures)
  → categorical + NumericMeasure    → bar_chart (top pairing only)
  → geographic + NumericMeasure     → bar_chart (primary), choropleth (alternative)

Priority 3 — Weaker signal:
  → no NumericMeasure exists        → count_bar by first Categorical
  → 2+ NumericMeasures              → scatter_plot (highest variance pair)
  → single NumericMeasure only      → histogram

Priority 4 — Conditional:
  → 2 categoricals + NumericMeasure → heatmap
  → OrderedCategorical              → ordered_bar (natural sort)
```

### Step 3: Return all valid suggestions ranked
- `shown_by_default: true` for top 3
- `shown_by_default: false` for remainder
- Frontend shows top 3, offers rest as view switcher alternatives
- Max 5 total from rule-based engine
- AI Phase 2 may add additional views beyond these 5
