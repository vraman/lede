import { useState, useEffect } from "react";
import { ParsedCSV, ViewSuggestion } from "../types";
import { query as duckQuery } from "../duckdb";
import { useDataContext } from "../DataContext";

interface Props {
  data: ParsedCSV;
  suggestion: ViewSuggestion;
}

export default function SummaryCard({ data, suggestion }: Props) {
  const { duckDbReady } = useDataContext();
  const metric = suggestion.metric;
  const [computedValue, setComputedValue] = useState<number | null>(null);

  useEffect(() => {
    if (!metric || !duckDbReady) return;

    async function compute() {
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
      } catch {
        setComputedValue(null);
      }
    }

    compute();
  }, [data, suggestion, metric, duckDbReady]);

  if (!metric) return null;

  const label = suggestion.title || `${suggestion.aggregate === "Avg" ? "Avg" : "Total"} ${metric}`;

  if (computedValue === null) {
    return (
      <div className="view-card summary-card">
        <div className="summary-value" style={{ opacity: 0.3 }}>--</div>
        <div className="summary-label">{label}</div>
      </div>
    );
  }

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
