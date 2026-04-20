# .lede File Format Spec

## Overview
`.lede` files are plain markdown with two additions:
1. YAML frontmatter for document metadata
2. Fenced `dataview` code blocks for embedded views

Opening a `.lede` file in any text editor shows readable prose
with view configs visible as code blocks. No binary data, no
proprietary format. Files are human-readable forever.

## Full Example

````markdown
---
title: Q1 Sales Analysis
created: 2025-04-10
sources:
  - sales_2025.csv
---

Revenue was strong this quarter, driven by the West region.

```dataview
source: sales_2025.csv
view: bar_chart
metric: revenue
aggregate: sum
group_by: region
sort: desc
limit: 10
title: Revenue by region
suggested_by: ai
last_verified: 2025-04-10
insight: "West region accounts for 54% of revenue despite only 38% of orders"
insight_edited: false
```

The West spike is worth investigating — enterprise deals in March
account for most of it.

```dataview
source: sales_2025.csv
view: summary_card
metric: revenue
aggregate: sum
title: Total revenue
suggested_by: rule
last_verified: 2025-04-10
insight: null
insight_edited: false
```

Looking at the trend, growth has been consistent since Q1.

```lede-insights
- Widget Pro generates 40% more revenue per order than Widget Lite
- Revenue is volatile week-to-week — Jan 12 was the strongest day at $224.97
```
````

## Frontmatter Fields

```yaml
---
title: string           # document title, editable inline
created: ISO date       # creation date, set once
sources:                # list of CSV filenames referenced
  - filename.csv
---
```

## Dataview Block Fields

### Required
```yaml
source: filename.csv    # CSV file this view reads from
view: bar_chart         # view type from ontology
```

### View-specific (see view-ontology.md)
```yaml
metric: column_name
aggregate: sum | avg | count | min | max
group_by: column_name
time_column: column_name
time_grain: day | week | month | quarter | year
x_metric: column_name
y_metric: column_name
metrics: [col1, col2]
columns: [col1, col2, col3]
limit: 10
sort: desc | asc | natural
normalize: false
compare: previous_period
```

### Metadata (on every block)
```yaml
title: string           # human-readable, overrides auto-generated
suggested_by: rule | ai | user
last_verified: ISO date # when app last verified this view's data
insight: string | null  # AI-generated observation, null if none
insight_edited: bool    # true if user has modified the AI insight
```

## lede-insights Block

Standalone AI-generated observations not tied to a specific view.
Rendered as a subtle callout above the first view.

```
```lede-insights
- Observation one with specific values
- Observation two with specific values
```
```

## Document Order

The document is a sequence of:
1. Prose (markdown paragraphs, headings, lists)
2. Dataview blocks (charts, cards, tables)
3. More prose
4. More dataview blocks
...

Order is significant — preserved exactly on save and restore.

## Serialization Rules

When saving from the Tiptap editor:
- Walk nodes in document order
- `paragraph` / `heading` nodes → markdown text
- `dataview` nodes → fenced dataview code block
- Empty paragraphs between consecutive dataview nodes → preserved
  (creates visual breathing room when reopened)

When loading a `.lede` file:
- Parse YAML frontmatter
- Split body on ` ```dataview ` and ` ```lede-insights ` fences
- Text segments → Tiptap paragraph nodes
- Dataview segments → Tiptap DataviewNode with parsed YAML config
- Ensure at least one empty paragraph between consecutive dataview nodes

## Source Resolution

`source` field contains just the filename (e.g., `sales_2025.csv`).

Resolution order:
1. Same directory as the `.lede` file
2. Tauri app data directory
3. Show "source not found" placeholder if neither exists

Never store absolute paths — `.lede` files should be portable
between machines.

## Staleness Detection (future)

`last_verified` enables staleness detection:
- When CSV changes after `last_verified`, flag the view
- AI re-checks whether `insight` is still supported by new data
- Flagged views show a subtle "data updated" indicator
- User can dismiss or request re-analysis
