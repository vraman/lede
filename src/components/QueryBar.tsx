import { useState, useRef, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import * as Plot from "@observablehq/plot";
import { query as duckQuery } from "../duckdb";
import { useDataContext } from "../DataContext";

type ResultView = "auto" | "table" | "chart";

export default function QueryBar() {
  const { data } = useDataContext();
  const [question, setQuestion] = useState("");
  const [sql, setSql] = useState<string | null>(null);
  const [results, setResults] = useState<Record<string, unknown>[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [view, setView] = useState<ResultView>("auto");
  const chartRef = useRef<HTMLDivElement>(null);

  if (!data) return null;

  const handleQuery = async () => {
    if (!question.trim()) return;
    setLoading(true);
    setError(null);
    setResults(null);
    setSql(null);
    setView("auto");

    try {
      const columns = data.columns.map((c) => ({
        name: c.name,
        col_type: c.col_type,
      }));
      const generatedSql = await invoke<string>("translate_query", {
        question: question.trim(),
        columns,
        tableName: "csv",
      });
      setSql(generatedSql);
      const rows = await duckQuery(generatedSql);
      setResults(rows);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  const resultShape = results && results.length > 0 ? detectShape(results) : null;

  // Render chart when results change
  useEffect(() => {
    if (!chartRef.current || !results || results.length === 0) return;
    if (!resultShape || resultShape.type === "scalar" || resultShape.type === "table") return;
    if (view === "table") return;

    const keys = Object.keys(results[0]);
    let chart: SVGSVGElement | HTMLElement | undefined;

    if (resultShape.type === "bar") {
      const [groupCol, metricCol] = [keys[0], keys[1]];
      const height = Math.max(200, results.length * 24);
      chart = Plot.plot({
        height,
        marginLeft: 120,
        x: { label: metricCol },
        y: { label: groupCol },
        marks: [
          Plot.barX(results, {
            y: groupCol,
            x: metricCol,
            fill: "#64748b",
            sort: { y: "-x" },
          }),
          Plot.ruleX([0]),
        ],
      });
    } else if (resultShape.type === "line") {
      const [timeCol, metricCol] = [keys[0], keys[1]];
      chart = Plot.plot({
        height: 200,
        x: { label: timeCol },
        y: { label: metricCol },
        marks: [
          Plot.lineY(results, { x: timeCol, y: metricCol, stroke: "#64748b" }),
          Plot.dot(results, { x: timeCol, y: metricCol, fill: "#64748b", r: 3 }),
        ],
      });
    }

    if (chart) {
      chartRef.current.innerHTML = "";
      chartRef.current.appendChild(chart);
    }

    return () => {
      if (chartRef.current) chartRef.current.innerHTML = "";
    };
  }, [results, view, resultShape]);

  const canChart = resultShape && (resultShape.type === "bar" || resultShape.type === "line");
  const showChart = canChart && view !== "table";
  const showTable = !showChart;

  return (
    <div className="query-bar">
      <div className="query-input-row">
        <input
          className="query-input"
          placeholder="Ask about your data..."
          value={question}
          onChange={(e) => setQuestion(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") handleQuery();
          }}
        />
        <button
          className="query-run-btn"
          onClick={handleQuery}
          disabled={loading || !question.trim()}
        >
          {loading ? "..." : "Go"}
        </button>
      </div>

      {sql && (
        <div className="query-sql-row">
          <input
            className="query-sql-input"
            value={sql}
            onChange={(e) => setSql(e.target.value)}
            onKeyDown={async (e) => {
              if (e.key === "Enter" && sql) {
                setLoading(true);
                setError(null);
                try {
                  const rows = await duckQuery(sql);
                  setResults(rows);
                } catch (err) {
                  setError(String(err));
                  setResults(null);
                } finally {
                  setLoading(false);
                }
              }
            }}
          />
          <button
            className="query-run-btn"
            onClick={async () => {
              if (!sql) return;
              setLoading(true);
              setError(null);
              try {
                const rows = await duckQuery(sql);
                setResults(rows);
              } catch (err) {
                setError(String(err));
                setResults(null);
              } finally {
                setLoading(false);
              }
            }}
          >Run</button>
        </div>
      )}
      {error && <div className="query-error">{error}</div>}

      {results && results.length > 0 && resultShape?.type === "scalar" && (
        <div className="query-scalar">
          <span className="query-scalar-value">
            {formatValue(Object.values(results[0])[0])}
          </span>
        </div>
      )}

      {results && results.length > 0 && (resultShape?.type === "bar" || resultShape?.type === "line") && (
        <div className="query-viz-controls">
          <button
            className={`query-viz-btn ${view !== "table" ? "active" : ""}`}
            onClick={() => setView("auto")}
          >Chart</button>
          <button
            className={`query-viz-btn ${view === "table" ? "active" : ""}`}
            onClick={() => setView("table")}
          >Table</button>
        </div>
      )}

      {showChart && <div ref={chartRef} className="query-chart" />}

      {results && showTable && resultShape?.type !== "scalar" && (
        <div className="query-results">
          <table>
            <thead>
              <tr>
                {Object.keys(results[0]).map((k) => (
                  <th key={k}>{k}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {results.slice(0, 100).map((row, i) => (
                <tr key={i}>
                  {Object.values(row).map((v, j) => (
                    <td key={j}>{formatValue(v)}</td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
          {results.length > 100 && (
            <p className="query-truncated">Showing first 100 of {results.length} results</p>
          )}
        </div>
      )}

      {results && results.length === 0 && (
        <div className="query-empty">No results</div>
      )}
    </div>
  );
}

type ShapeType = "scalar" | "bar" | "line" | "table";

function detectShape(rows: Record<string, unknown>[]): { type: ShapeType } {
  const keys = Object.keys(rows[0]);
  if (rows.length === 1 && keys.length === 1) return { type: "scalar" };
  if (keys.length === 2 && rows.length > 1) {
    const secondIsNumeric = rows.every((r) => r[keys[1]] === null || typeof r[keys[1]] === "number");
    if (secondIsNumeric) {
      const firstLooksYears = rows.every((r) => {
        const n = Number(r[keys[0]]);
        return !isNaN(n) && n >= 1900 && n <= 2100;
      });
      return { type: firstLooksYears ? "line" : "bar" };
    }
  }
  return { type: "table" };
}

function formatValue(v: unknown): string {
  if (v === null || v === undefined) return "";
  if (typeof v === "number") return v.toLocaleString(undefined, { maximumFractionDigits: 2 });
  return String(v);
}
