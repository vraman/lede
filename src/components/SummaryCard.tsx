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

  let value: number;
  let label: string;

  if (suggestion.aggregate === "Avg") {
    value = values.reduce((a, b) => a + b, 0) / values.length;
    label = `Avg ${metric}`;
  } else {
    value = values.reduce((a, b) => a + b, 0);
    label = `Total ${metric}`;
  }

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
