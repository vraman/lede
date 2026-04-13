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
  | "BoxPlot"
  | "OrderedBar"
  | "MultiLineChart"
  | "Choropleth";

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
  suggested_by: string;
  insight: string | null;
  insight_edited: boolean;
}

export interface ParsedCSV {
  columns: ColumnMeta[];
  row_count: number;
  sample_rows: string[][];
  suggestions: ViewSuggestion[];
}

export interface ModelStatus {
  downloaded: boolean;
  downloading: boolean;
  progress: number;
}

export interface AiViewSuggestion {
  view_type: string;
  title: string;
  metric: string | null;
  group_by: string | null;
  aggregate: string | null;
  insight: string | null;
  insight_kind: string | null;
  insight_params: Record<string, unknown> | null;
  x_col: string | null;
  y_col: string | null;
  sort: string | null;
  limit: number | null;
}

export interface AiInsightsResponse {
  suggested_views: AiViewSuggestion[];
  insights: string[];
  suggested_title: string | null;
}
