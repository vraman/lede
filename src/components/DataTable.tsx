import { useState, useEffect } from "react";
import { ParsedCSV } from "../types";
import { query as duckQuery, isLoaded as isDuckLoaded } from "../duckdb";

interface Props {
  data: ParsedCSV;
}

const PAGE_SIZE = 50;

export default function DataTable({ data }: Props) {
  const [rows, setRows] = useState<Record<string, unknown>[]>([]);
  const [page, setPage] = useState(0);
  const [totalRows, setTotalRows] = useState(data.row_count);

  useEffect(() => {
    setPage(0);
  }, [data]);

  useEffect(() => {
    async function fetchPage() {
      if (isDuckLoaded()) {
        try {
          const offset = page * PAGE_SIZE;
          const result = await duckQuery(
            `SELECT * FROM csv LIMIT ${PAGE_SIZE} OFFSET ${offset}`
          );
          setRows(result);
          return;
        } catch {
          // Fall through to sample rows
        }
      }
      // Fallback: use sample_rows
      const start = page * PAGE_SIZE;
      const sliced = data.sample_rows.slice(start, start + PAGE_SIZE);
      setRows(
        sliced.map((row) => {
          const obj: Record<string, unknown> = {};
          data.columns.forEach((col, i) => {
            obj[col.name] = row[i];
          });
          return obj;
        })
      );
      setTotalRows(data.sample_rows.length);
    }
    fetchPage();
  }, [data, page]);

  const totalPages = Math.ceil(totalRows / PAGE_SIZE);
  const colNames = data.columns.map((c) => c.name);

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
                  <span className="col-type">{data.columns[i].col_type}</span>
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
