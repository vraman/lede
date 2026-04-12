# Column Types — Lede Classifier Spec

## Column Type Enum

```rust
pub enum ColumnType {
    Temporal,           // dates, timestamps
    Categorical,        // text, or low-cardinality numeric
    OrderedCategorical, // Likert scales, star ratings
    Geographic,         // state, country, city, zip, region
    Identifier,         // high cardinality text — never group by
    NumericMeasure,     // numbers to aggregate
    NumericDimension,   // numbers to group by, never aggregate
    NumericRatio,       // percentages, rates — default aggregate: avg
    NumericSigned,      // contains negative values — flag for user
}
```

## Classification Rules (in order)

### 1. Temporal
Parses as date using any of these formats:
- `%Y-%m-%d` (ISO)
- `%m/%d/%Y` (American)
- `%m/%d/%Y %H:%M:%S` (American with time)
- `%m/%d/%Y %H:%M`
- `%d/%m/%Y` (European)
- Unix timestamp (integer, large value)

### 2. Geographic
Column name (case-insensitive) is in:
`[state, country, city, zip, region, country_code, lat, lng, latitude, longitude]`
→ Subtype of Categorical. Suggest choropleth (future) not bar_chart.

### 3. Identifier
Name is in INDEX_NAMES:
`[index, idx, row, row_number, row_num, rn, no, #, id]`
AND numeric with unique_count/total > 0.9
→ Never group by. Never use as metric.

High-cardinality text (unique_count/total > 0.9) → Identifier

### 4. OrderedCategorical
Values (lowercased) are a subset of a known ordinal set:
- `[strongly agree, agree, neutral, disagree, strongly disagree]`
- `[never, rarely, sometimes, often, always]`
- `[poor, fair, good, very good, excellent]`
- `[1, 2, 3, 4, 5]` (when all values are in this exact set)
→ Sort by natural order, not alphabetical.

### 5. NumericDimension
Column is numeric AND any of:
- unique_count / total < 0.05 (very low cardinality)
- All integers AND (max - min) < 500 AND unique_count/total < 0.3
- Name contains (word boundary): `[year, zip, code, rating, score, rank, grade, level, tier]`
→ Group by, never aggregate. Treated like Categorical in suggestion engine.

### 6. NumericRatio
All values between 0.0–1.0
OR all values between 0–100 AND name contains `[rate, pct, percent, share, ratio]`
→ Default aggregate: avg not sum.

### 7. NumericSigned
Any value < 0 (after removing sentinel -1 values if >20% are exactly -1)
→ Flag for user. Still usable as metric.

### 8. NumericMeasure
All other numeric columns.

### 9. Categorical
All other text columns.

## Sampling Strategy
- Classify using first 200 values per column (SAMPLE_SIZE = 200)
- Parse up to first 5000 rows (MAX_CLASSIFY_ROWS = 5000)
- unique_count uses ALL rows (needed for cardinality checks)
- Row count uses streaming scan of full file

## Sentinel Detection
If >20% of numeric values are exactly -1:
- Remove them and re-evaluate remaining values
- If remaining is <50% numeric → Categorical
- Otherwise classify normally without sentinels

## Known Edge Cases
- Sequential integers named "index" → Identifier (not NumericMeasure)
- Founding years (1990–2020) → NumericDimension (low range integers)
- NPS scores (0–10) → NumericDimension (name contains "score")
- Market share (0.0–1.0) → NumericRatio
- Signed P&L columns → NumericSigned
