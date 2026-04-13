import { ParsedCSV, ViewSuggestion } from "../types";

interface Props {
  data: ParsedCSV;
  suggestion: ViewSuggestion;
}

export default function SummaryCard({ data, suggestion }: Props) {
  const metric = suggestion.metric;
  if (!metric) return null;

  const colIndex = data.columns.findIndex((c) => c.name === metric);
  if (colIndex === -1) return null;

  const values = data.sample_rows.map((row) => parseFloat(row[colIndex])).filter((n) => !isNaN(n));

  if (values.length === 0) return null;

  let value: number;
  let label: string;

  const numerator = values.reduce((a, b) => a + b, 0);

  if (suggestion.denominator) {
    // Ratio: Sum(metric) / Sum(denominator)
    const denomIndex = data.columns.findIndex((c) => c.name === suggestion.denominator);
    if (denomIndex !== -1) {
      const denomValues = data.sample_rows.map((row) => parseFloat(row[denomIndex])).filter((n) => !isNaN(n));
      const denomSum = denomValues.reduce((a, b) => a + b, 0);
      value = denomSum > 0 ? numerator / denomSum : 0;
    } else {
      value = numerator;
    }
  } else if (suggestion.aggregate === "Avg") {
    value = numerator / values.length;
  } else if (suggestion.aggregate === "Min") {
    value = Math.min(...values);
  } else if (suggestion.aggregate === "Max") {
    value = Math.max(...values);
  } else if (suggestion.aggregate === "Count") {
    value = values.length;
  } else {
    value = numerator;
  }

  label = suggestion.title || `${suggestion.aggregate === "Avg" ? "Avg" : "Total"} ${metric}`;

  // Format with commas and 2 decimal places
  const formatted = value.toLocaleString(undefined, {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  });

  return (
    <div className="view-card summary-card">
      <div className="summary-value">{formatted}</div>
      <div className="summary-label">{label}</div>
    </div>
  );
}
