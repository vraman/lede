import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { query as duckQuery } from "../duckdb";
import { useDataContext } from "../DataContext";

export default function QueryBar() {
  const { data } = useDataContext();
  const [question, setQuestion] = useState("");
  const [sql, setSql] = useState<string | null>(null);
  const [results, setResults] = useState<Record<string, unknown>[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  if (!data) return null;

  const handleQuery = async () => {
    if (!question.trim()) return;
    setLoading(true);
    setError(null);
    setResults(null);
    setSql(null);

    try {
      // Ask the AI model to translate the question to SQL
      const columns = data.columns.map(
        (c) => `${c.name} (${c.col_type})`
      );
      const generatedSql = await invoke<string>("natural_language_to_sql", {
        question: question.trim(),
        columns: columns,
        tableName: "csv",
      });

      setSql(generatedSql);

      // Run the SQL against DuckDB
      const rows = await duckQuery(generatedSql);
      setResults(rows);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="query-bar">
      <div className="query-input-row">
        <input
          className="query-input"
          placeholder="Ask a question about your data..."
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
          {loading ? "..." : "Ask"}
        </button>
      </div>

      {sql && (
        <div className="query-sql">
          <code>{sql}</code>
        </div>
      )}

      {error && <div className="query-error">{error}</div>}

      {results && results.length > 0 && (
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
                    <td key={j}>{String(v ?? "")}</td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
          {results.length > 100 && (
            <p className="query-truncated">
              Showing first 100 of {results.length} results
            </p>
          )}
        </div>
      )}

      {results && results.length === 0 && (
        <div className="query-empty">No results</div>
      )}
    </div>
  );
}
