import { useState, useEffect } from "react";
import { ParsedCSV } from "../types";
import { query as duckQuery } from "../duckdb";

interface Props {
  data: ParsedCSV;
  duckDbReady: boolean;
}

const PAGE_SIZE = 50;

export default function DataTable({ data, duckDbReady }: Props) {
  const [rows, setRows] = useState<Record<string, unknown>[]>([]);
  const [colNames, setColNames] = useState<string[]>(data.columns.map((c) => c.name));
  const [page, setPage] = useState(0);
  const [totalRows, setTotalRows] = useState(data.row_count);

  // Reset page when data changes
  useEffect(() => {
    setPage(0);
    setColNames(data.columns.map((c) => c.name));
  }, [data]);

  useEffect(() => {
    async function fetchPage() {
      if (duckDbReady) {
        try {
          const offset = page * PAGE_SIZE;
          const result = await duckQuery(
            `SELECT * FROM csv LIMIT ${PAGE_SIZE} OFFSET ${offset}`
          );
          if (result.length > 0) {
            setColNames(Object.keys(result[0]));
          }
          setRows(result);
          setTotalRows(data.row_count);
          return;
        } catch {
          // Fall through to sample rows
        }
      }
      // Fallback: use sample_rows
      const start = page * PAGE_SIZE;
      const sliced = data.sample_rows.slice(start, start + PAGE_SIZE);
      const cols = data.columns.map((c) => c.name);
      setColNames(cols);
      setRows(
        sliced.map((row) => {
          const obj: Record<string, unknown> = {};
          cols.forEach((name, i) => {
            obj[name] = row[i];
          });
          return obj;
        })
      );
      setTotalRows(data.sample_rows.length);
    }
    fetchPage();
  }, [data, page, duckDbReady]);

  const totalPages = Math.ceil(totalRows / PAGE_SIZE);

  return (
    <div className="view-card table-view">
      <div className="table-header">
        <h3 className="chart-title">
          Data ({totalRows.toLocaleString()} rows)
        </h3>
        {totalPages > 1 && (
          <div className="table-pagination">
            <button
              className="table-page-btn"
              disabled={page === 0}
              onClick={() => setPage(page - 1)}
            >
              Prev
            </button>
            <span className="table-page-info">
              {page * PAGE_SIZE + 1}–{Math.min((page + 1) * PAGE_SIZE, totalRows)} of {totalRows.toLocaleString()}
            </span>
            <button
              className="table-page-btn"
              disabled={page >= totalPages - 1}
              onClick={() => setPage(page + 1)}
            >
              Next
            </button>
          </div>
        )}
      </div>
      <div className="table-scroll">
        <table>
          <thead>
            <tr>
              {colNames.map((name, i) => (
                <th key={name}>
                  {name}
                  {data.columns[i] && (
                    <span className="col-type">{data.columns[i].col_type}</span>
                  )}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {rows.map((row, i) => (
              <tr key={i}>
                {colNames.map((name) => (
                  <td key={name}>{String(row[name] ?? "")}</td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
