import { useRef, useEffect } from "react";
import * as Plot from "@observablehq/plot";
import { ParsedCSV, ViewSuggestion } from "../types";
import { useDataContext } from "../DataContext";

interface Props {
  data: ParsedCSV;
  suggestion: ViewSuggestion;
  alternatives?: ViewSuggestion[];
  onSwap?: (alt: ViewSuggestion) => void;
  onChangeAggregate?: (agg: ViewSuggestion["aggregate"]) => void;
}

const ALL_AGGREGATES: ViewSuggestion["aggregate"][] = ["Sum", "Avg", "Min", "Max", "Count"];

const ORDINAL_ORDERS: Record<string, string[]> = {
  satisfaction: [
    "Strongly Disagree",
    "Disagree",
    "Neutral",
    "Agree",
    "Strongly Agree",
  ],
  likelihood_to_recommend: ["Never", "Rarely", "Sometimes", "Often", "Always"],
};

const NUMERIC_TYPES = new Set([
  "NumericMeasure",
  "NumericRatio",
  "NumericSigned",
]);

function truncLabel(s: string, max = 40): string {
  return s.length > max ? s.slice(0, max - 1) + "\u2026" : s;
}

/** Try to parse a date string into a Date object. Returns null on failure. */
function tryParseDate(s: string): Date | null {
  // Try ISO format first (fast path)
  const d = new Date(s);
  if (!isNaN(d.getTime())) return d;
  // Try M/D/YYYY H:M:S format
  const m = s.match(/^(\d{1,2})\/(\d{1,2})\/(\d{4})\s*(\d{1,2}):(\d{2})(?::(\d{2}))?/);
  if (m) return new Date(+m[3], +m[1] - 1, +m[2], +m[4], +m[5], +(m[6] || 0));
  return null;
}

function buildRows(data: ParsedCSV): Record<string, string | number>[] {
  return data.sample_rows.map((row) => {
    const obj: Record<string, string | number> = {};
    data.columns.forEach((col, i) => {
      const raw = row[i];
      if (NUMERIC_TYPES.has(col.col_type)) {
        const num = parseFloat(raw);
        obj[col.name] = isNaN(num) ? raw : num;
      } else {
        obj[col.name] = raw;
      }
    });
    return obj;
  });
}

function aggregate(
  rows: Record<string, string | number>[],
  groupBy: string,
  metric: string,
  aggType: string
): Record<string, string | number>[] {
  const groups: Record<string, number[]> = {};
  for (const row of rows) {
    const key = String(row[groupBy]);
    if (!groups[key]) groups[key] = [];
    const val = Number(row[metric]);
    if (!isNaN(val)) groups[key].push(val);
  }

  return Object.entries(groups).map(([key, vals]) => {
    let value: number;
    if (aggType === "Avg") {
      value = vals.reduce((a, b) => a + b, 0) / vals.length;
    } else if (aggType === "Count") {
      value = vals.length;
    } else {
      value = vals.reduce((a, b) => a + b, 0);
    }
    return { [groupBy]: key, [metric]: value };
  });
}

function countByGroup(
  rows: Record<string, string | number>[],
  groupBy: string
): Record<string, string | number>[] {
  const counts: Record<string, number> = {};
  for (const row of rows) {
    const key = String(row[groupBy]);
    counts[key] = (counts[key] || 0) + 1;
  }
  return Object.entries(counts).map(([key, count]) => ({
    [groupBy]: key,
    count,
  }));
}

const VIEW_TYPE_LABELS: Record<string, string> = {
  LineChart: "Line",
  BarChart: "Bar",
  CountBar: "Count",
  OrderedBar: "Ordered",
  ScatterPlot: "Scatter",
  Histogram: "Histogram",
  RankedList: "Ranked",
  Heatmap: "Heatmap",
};

export default function ChartView({ data, suggestion, alternatives = [], onSwap, onChangeAggregate }: Props) {
  const { sourceFilename } = useDataContext();
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!containerRef.current) return;

    const rows = buildRows(data);
    let chart: SVGSVGElement | HTMLElement | undefined;

    try {
      switch (suggestion.view_type) {
        case "BarChart": {
          const groupBy = suggestion.group_by!;
          const metric = suggestion.metric!;
          const agg = aggregate(rows, groupBy, metric, suggestion.aggregate);
          const barHeight = Math.max(200, agg.length * 24);
          chart = Plot.plot({
            height: barHeight,
            marginLeft: 100,
            x: { label: truncLabel(metric) },
            y: { label: truncLabel(groupBy) },
            marks: [
              Plot.barX(agg, {
                y: groupBy,
                x: metric,
                fill: "#64748b",
                sort: { y: "-x" },
              }),
              Plot.ruleX([0]),
            ],
          });
          break;
        }

        case "LineChart": {
          const groupBy = suggestion.group_by!;
          const metric = suggestion.metric!;
          const agg = aggregate(rows, groupBy, metric, suggestion.aggregate);
          // Check if groupBy column is Temporal
          const groupCol = data.columns.find((c) => c.name === groupBy);
          const isTemporal = groupCol?.col_type === "Temporal";

          if (isTemporal) {
            // Parse dates for proper time scale
            const dateKey = "__date__";
            const dated = agg
              .map((row) => ({ ...row, [dateKey]: tryParseDate(String(row[groupBy])) }))
              .filter((row) => row[dateKey] !== null) as Record<string, any>[];
            dated.sort((a, b) => (a[dateKey] as Date).getTime() - (b[dateKey] as Date).getTime());
            chart = Plot.plot({
              height: 200,
              x: { label: truncLabel(groupBy), type: "time" },
              y: { label: truncLabel(metric), zero: true },
              marks: [
                Plot.lineY(dated, { x: dateKey, y: metric, stroke: "#64748b" }),
                Plot.dot(dated, { x: dateKey, y: metric, fill: "#64748b", r: 3 }),
              ],
            });
          } else {
            agg.sort((a, b) =>
              String(a[groupBy]).localeCompare(String(b[groupBy]))
            );
            chart = Plot.plot({
              height: 200,
              x: { label: truncLabel(groupBy), type: "point" },
              y: { label: truncLabel(metric), zero: true },
              marks: [
                Plot.lineY(agg, { x: groupBy, y: metric, stroke: "#64748b" }),
                Plot.dot(agg, { x: groupBy, y: metric, fill: "#64748b", r: 3 }),
              ],
            });
          }
          break;
        }

        case "ScatterPlot": {
          const xCol = suggestion.x_col!;
          const yCol = suggestion.y_col!;
          chart = Plot.plot({
            height: 200,
            x: { label: truncLabel(xCol) },
            y: { label: truncLabel(yCol) },
            marks: [
              Plot.dot(rows, {
                x: xCol,
                y: yCol,
                fill: "#64748b",
                r: 4,
              }),
            ],
          });
          break;
        }

        case "Histogram": {
          const metric = suggestion.metric!;
          chart = Plot.plot({
            height: 200,
            x: { label: truncLabel(metric) },
            marks: [
              Plot.rectY(rows, { ...Plot.binX({ y: "count" }, { x: metric }), fill: "#64748b" }),
              Plot.ruleY([0]),
            ],
          });
          break;
        }

        case "CountBar":
        case "OrderedBar": {
          const groupBy = suggestion.group_by!;
          const counted = countByGroup(rows, groupBy);
          const domain = ORDINAL_ORDERS[groupBy];
          chart = Plot.plot({
            height: 200,
            marginLeft: 140,
            x: { label: "Count" },
            y: {
              label: truncLabel(groupBy),
              domain: domain || undefined,
            },
            marks: [
              Plot.barX(counted, {
                y: groupBy,
                x: "count",
                fill: "#64748b",
                sort: domain ? undefined : { y: "-x" },
              }),
              Plot.ruleX([0]),
            ],
          });
          break;
        }

        case "RankedList": {
          const groupBy = suggestion.group_by!;
          const metric = suggestion.metric!;
          const agg = aggregate(rows, groupBy, metric, suggestion.aggregate);
          agg.sort((a, b) => Number(b[metric]) - Number(a[metric]));
          const limited = agg.slice(0, suggestion.limit || 10);
          chart = Plot.plot({
            height: 200,
            marginLeft: 100,
            x: { label: truncLabel(metric) },
            y: { label: truncLabel(groupBy) },
            marks: [
              Plot.barX(limited, {
                y: groupBy,
                x: metric,
                fill: "#64748b",
                sort: { y: "-x" },
              }),
              Plot.ruleX([0]),
            ],
          });
          break;
        }

        default:
          break;
      }
    } catch (e) {
      console.error("Chart render error:", e);
    }

    if (chart) {
      containerRef.current.innerHTML = "";
      containerRef.current.appendChild(chart);
    }

    return () => {
      if (containerRef.current) {
        containerRef.current.innerHTML = "";
      }
    };
  }, [data, suggestion]);

  const hasMetricAgg = suggestion.metric && suggestion.group_by
    && suggestion.view_type !== "CountBar"
    && suggestion.view_type !== "OrderedBar"
    && suggestion.view_type !== "Histogram";

  const title = suggestion.title || suggestion.metric || suggestion.group_by || "Chart";

  const subtitle = sourceFilename
    ? `${sourceFilename} · ${data.row_count.toLocaleString()} rows`
    : null;

  return (
    <div className="view-card chart-view">
      <div className="chart-header">
        <div className="chart-title-group">
          <h3 className="chart-title">{title}</h3>
          {subtitle && <span className="chart-subtitle">{subtitle}</span>}
        </div>
        <div className="chart-controls">
          {hasMetricAgg && onChangeAggregate && (
            <select
              className="agg-select"
              value={suggestion.aggregate}
              onChange={(e) => onChangeAggregate(e.target.value as ViewSuggestion["aggregate"])}
            >
              {ALL_AGGREGATES.map((agg) => (
                <option key={agg} value={agg}>{agg.toLowerCase()}</option>
              ))}
            </select>
          )}
          {alternatives.length > 0 && (
            <div className="view-switcher">
              <span className="view-chip active">
                {VIEW_TYPE_LABELS[suggestion.view_type] || suggestion.view_type}
              </span>
              {alternatives.map((alt, i) => (
                <button
                  key={i}
                  className="view-chip"
                  onClick={() => onSwap?.(alt)}
                >
                  {VIEW_TYPE_LABELS[alt.view_type] || alt.view_type}
                </button>
              ))}
            </div>
          )}
        </div>
      </div>
      <div ref={containerRef} />
    </div>
  );
}
