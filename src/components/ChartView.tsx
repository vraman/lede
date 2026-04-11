import { useRef, useEffect } from "react";
import * as Plot from "@observablehq/plot";
import { ParsedCSV, ViewSuggestion } from "../types";

interface Props {
  data: ParsedCSV;
  suggestion: ViewSuggestion;
}

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

function buildRows(data: ParsedCSV): Record<string, string | number>[] {
  return data.sample_rows.map((row) => {
    const obj: Record<string, string | number> = {};
    data.columns.forEach((col, i) => {
      const raw = row[i];
      const num = parseFloat(raw);
      obj[col.name] = isNaN(num) ? raw : num;
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

export default function ChartView({ data, suggestion }: Props) {
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
          chart = Plot.plot({
            marginLeft: 100,
            x: { label: metric },
            y: { label: groupBy },
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
          chart = Plot.plot({
            x: { label: groupBy, type: "point" },
            y: { label: metric },
            marks: [
              Plot.lineY(agg, { x: groupBy, y: metric, stroke: "#64748b" }),
              Plot.dot(agg, { x: groupBy, y: metric, fill: "#64748b" }),
            ],
          });
          break;
        }

        case "ScatterPlot": {
          const xCol = suggestion.x_col!;
          const yCol = suggestion.y_col!;
          chart = Plot.plot({
            x: { label: xCol },
            y: { label: yCol },
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
            x: { label: metric },
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
            marginLeft: 140,
            x: { label: "Count" },
            y: {
              label: groupBy,
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
            marginLeft: 100,
            x: { label: metric },
            y: { label: groupBy },
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

  const title =
    suggestion.view_type === "ScatterPlot"
      ? `${suggestion.x_col} vs ${suggestion.y_col}`
      : suggestion.view_type === "CountBar" || suggestion.view_type === "OrderedBar"
        ? `${suggestion.group_by} (count)`
        : `${suggestion.metric} by ${suggestion.group_by || "value"}`;

  return (
    <div className="view-card chart-view">
      <h3 className="chart-title">{title}</h3>
      <div ref={containerRef} />
    </div>
  );
}
