import { useState, useEffect } from "react";
import { ParsedCSV, ViewSuggestion } from "../types";
import { query as duckQuery, isLoaded as isDuckLoaded } from "../duckdb";

interface Props {
  data: ParsedCSV;
  suggestion: ViewSuggestion;
}

export default function SummaryCard({ data, suggestion }: Props) {
  const metric = suggestion.metric;
  const [computedValue, setComputedValue] = useState<number | null>(null);

  useEffect(() => {
    if (!metric) return;

    async function compute() {
      if (isDuckLoaded()) {
        try {
          let sql: string;
          if (suggestion.denominator) {
            sql = `SELECT SUM("${metric}") / NULLIF(SUM("${suggestion.denominator}"), 0) as val FROM csv`;
          } else {
            const fn = suggestion.aggregate === "Avg" ? "AVG"
              : suggestion.aggregate === "Min" ? "MIN"
              : suggestion.aggregate === "Max" ? "MAX"
              : suggestion.aggregate === "Count" ? "COUNT"
              : "SUM";
            sql = `SELECT ${fn}("${metric}") as val FROM csv`;
          }
          const rows = await duckQuery(sql);
          if (rows.length > 0 && rows[0].val != null) {
            setComputedValue(Number(rows[0].val));
          }
          return;
        } catch {
          // Fall through to sample-based computation
        }
      }

      // Fallback: compute from sample rows
      const colIndex = data.columns.findIndex((c) => c.name === metric);
      if (colIndex === -1) return;
      const values = data.sample_rows.map((row) => parseFloat(row[colIndex])).filter((n) => !isNaN(n));
      if (values.length === 0) return;

      const numerator = values.reduce((a, b) => a + b, 0);
      let val: number;

      if (suggestion.denominator) {
        const denomIndex = data.columns.findIndex((c) => c.name === suggestion.denominator);
        if (denomIndex !== -1) {
          const denomValues = data.sample_rows.map((row) => parseFloat(row[denomIndex])).filter((n) => !isNaN(n));
          const denomSum = denomValues.reduce((a, b) => a + b, 0);
          val = denomSum > 0 ? numerator / denomSum : 0;
        } else {
          val = numerator;
        }
      } else if (suggestion.aggregate === "Avg") {
        val = numerator / values.length;
      } else if (suggestion.aggregate === "Min") {
        val = Math.min(...values);
      } else if (suggestion.aggregate === "Max") {
        val = Math.max(...values);
      } else if (suggestion.aggregate === "Count") {
        val = values.length;
      } else {
        val = numerator;
      }
      setComputedValue(val);
    }

    compute();
  }, [data, suggestion, metric]);

  if (!metric || computedValue === null) return null;

  const label = suggestion.title || `${suggestion.aggregate === "Avg" ? "Avg" : "Total"} ${metric}`;

  const formatted = computedValue.toLocaleString(undefined, {
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
