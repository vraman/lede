export type ColumnType =
  | "Temporal"
  | "Categorical"
  | "OrderedCategorical"
  | "Geographic"
  | "Identifier"
  | "NumericMeasure"
  | "NumericRatio"
  | "NumericSigned"
  | "NumericDimension";

export interface ColumnMeta {
  name: string;
  col_type: ColumnType;
  unique_count: number;
  total_count: number;
  sample_values: string[];
}

export type ViewType =
  | "Table"
  | "SummaryCard"
  | "LineChart"
  | "BarChart"
  | "CountBar"
  | "ScatterPlot"
  | "Histogram"
  | "Heatmap"
  | "OrderedBar"
  | "RankedList"
  | "MultiLineChart";

export type Aggregate = "Sum" | "Avg" | "Count" | "Min" | "Max";

export interface ViewSuggestion {
  view_type: ViewType;
  title: string;
  metric: string | null;
  group_by: string | null;
  aggregate: Aggregate;
  sort: string | null;
  limit: number | null;
  x_col: string | null;
  y_col: string | null;
  shown_by_default: boolean;
}

export interface ParsedCSV {
  columns: ColumnMeta[];
  row_count: number;
  sample_rows: string[][];
  suggestions: ViewSuggestion[];
}
