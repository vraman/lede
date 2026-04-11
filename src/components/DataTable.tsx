import { ParsedCSV } from "../types";

interface Props {
  data: ParsedCSV;
}

export default function DataTable({ data }: Props) {
  return (
    <div className="view-card table-view">
      <h3 className="chart-title">Data Preview ({data.row_count} rows)</h3>
      <div className="table-scroll">
        <table>
          <thead>
            <tr>
              {data.columns.map((col) => (
                <th key={col.name}>
                  {col.name}
                  <span className="col-type">{col.col_type}</span>
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {data.sample_rows.slice(0, 10).map((row, i) => (
              <tr key={i}>
                {row.map((cell, j) => (
                  <td key={j}>{cell}</td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
