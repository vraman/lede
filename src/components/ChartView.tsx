import { useRef, useEffect, useState } from "react";
import * as Plot from "@observablehq/plot";
import * as topojson from "topojson-client";
import type { Topology } from "topojson-specification";
import { ParsedCSV, ViewSuggestion } from "../types";
import { query as duckQuery, isLoaded as isDuckLoaded } from "../duckdb";
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

const US_STATE_ABBREVS: Record<string, string> = {
  al: "Alabama", ak: "Alaska", az: "Arizona", ar: "Arkansas", ca: "California",
  co: "Colorado", ct: "Connecticut", de: "Delaware", fl: "Florida", ga: "Georgia",
  hi: "Hawaii", id: "Idaho", il: "Illinois", in: "Indiana", ia: "Iowa",
  ks: "Kansas", ky: "Kentucky", la: "Louisiana", me: "Maine", md: "Maryland",
  ma: "Massachusetts", mi: "Michigan", mn: "Minnesota", ms: "Mississippi", mo: "Missouri",
  mt: "Montana", ne: "Nebraska", nv: "Nevada", nh: "New Hampshire", nj: "New Jersey",
  nm: "New Mexico", ny: "New York", nc: "North Carolina", nd: "North Dakota", oh: "Ohio",
  ok: "Oklahoma", or: "Oregon", pa: "Pennsylvania", ri: "Rhode Island", sc: "South Carolina",
  sd: "South Dakota", tn: "Tennessee", tx: "Texas", ut: "Utah", vt: "Vermont",
  va: "Virginia", wa: "Washington", wv: "West Virginia", wi: "Wisconsin", wy: "Wyoming",
  dc: "District of Columbia",
};

const US_STATE_NAMES = new Set(Object.values(US_STATE_ABBREVS).map(n => n.toLowerCase()));

/** Normalize a geo value to the canonical name used in TopoJSON properties. */
export function normalizeGeoName(raw: string): string {
  const lower = raw.trim().toLowerCase();
  // Check if it's a US state abbreviation
  if (US_STATE_ABBREVS[lower]) return US_STATE_ABBREVS[lower];
  // Title-case it for matching
  return lower.replace(/\b\w/g, c => c.toUpperCase());
}

/** Detect whether sample values look like US states. */
export function looksLikeUSStates(values: string[]): boolean {
  let matches = 0;
  for (const v of values) {
    const lower = v.trim().toLowerCase();
    if (US_STATE_NAMES.has(lower) || US_STATE_ABBREVS[lower]) matches++;
  }
  return matches > values.length * 0.5;
}

interface GeoCache {
  data: unknown;
  key: string;
}

export function truncLabel(s: string, max = 40): string {
  return s.length > max ? s.slice(0, max - 1) + "\u2026" : s;
}

/** Try to parse a date string into a Date object. Returns null on failure. */
export function tryParseDate(s: string): Date | null {
  // Try ISO format first (fast path)
  const d = new Date(s);
  if (!isNaN(d.getTime())) return d;
  // Try M/D/YYYY H:M:S format
  const m = s.match(/^(\d{1,2})\/(\d{1,2})\/(\d{4})\s*(\d{1,2}):(\d{2})(?::(\d{2}))?/);
  if (m) return new Date(+m[3], +m[1] - 1, +m[2], +m[4], +m[5], +(m[6] || 0));
  return null;
}

export function buildRows(data: ParsedCSV): Record<string, string | number>[] {
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

export function aggregate(
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
    } else if (aggType === "Min") {
      value = Math.min(...vals);
    } else if (aggType === "Max") {
      value = Math.max(...vals);
    } else {
      value = vals.reduce((a, b) => a + b, 0);
    }
    return { [groupBy]: key, [metric]: value };
  });
}

export function countByGroup(
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
  BoxPlot: "Box Plot",
  Heatmap: "Heatmap",
  Choropleth: "Map",
};

export default function ChartView({ data, suggestion, alternatives = [], onSwap, onChangeAggregate }: Props) {
  const { sourceFilename } = useDataContext();
  const containerRef = useRef<HTMLDivElement>(null);
  const geoCacheRef = useRef<GeoCache>({ data: null, key: "" });
  const [geoData, setGeoData] = useState<unknown>(null);

  // Async geo data loading for Choropleth
  useEffect(() => {
    if (suggestion.view_type !== "Choropleth") return;
    const geoCol = suggestion.group_by;
    if (!geoCol) return;

    const sampleValues = data.sample_rows.slice(0, 20).map(
      (row) => row[data.columns.findIndex((c) => c.name === geoCol)] || ""
    );
    const isUS = looksLikeUSStates(sampleValues);
    const url = isUS ? "/geo/us-states-10m.json" : "/geo/countries-110m.json";

    if (geoCacheRef.current.key === url && geoCacheRef.current.data) {
      setGeoData(geoCacheRef.current.data);
      return;
    }

    fetch(url)
      .then((r) => r.json())
      .then((topo) => {
        geoCacheRef.current = { data: topo, key: url };
        setGeoData(topo);
      })
      .catch((e) => console.error("Failed to load geo data:", e));
  }, [suggestion.view_type, suggestion.group_by, data]);

  useEffect(() => {
    if (!containerRef.current) return;
    let cancelled = false;

    /** Query DuckDB for aggregated data, fall back to sample rows. */
    async function queryAgg(
      groupBy: string,
      metric: string,
      aggType: string,
      limit?: number | null,
    ): Promise<Record<string, string | number>[]> {
      if (isDuckLoaded()) {
        const aggFn = aggType === "Avg" ? "AVG" : aggType === "Count" ? "COUNT" : aggType === "Min" ? "MIN" : aggType === "Max" ? "MAX" : "SUM";
        const limitClause = limit ? `LIMIT ${limit}` : "";
        const sql = `SELECT "${groupBy}", ${aggFn}("${metric}") as "${metric}" FROM csv WHERE "${metric}" IS NOT NULL GROUP BY "${groupBy}" ORDER BY "${metric}" DESC ${limitClause}`;
        const rows = await duckQuery(sql);
        return rows as Record<string, string | number>[];
      }
      // Fallback to sample rows
      const rows = buildRows(data);
      let agg = aggregate(rows, groupBy, metric, aggType);
      agg.sort((a, b) => Number(b[metric]) - Number(a[metric]));
      if (limit) agg = agg.slice(0, limit);
      return agg;
    }

    async function queryCount(groupBy: string): Promise<Record<string, string | number>[]> {
      if (isDuckLoaded()) {
        const sql = `SELECT "${groupBy}", COUNT(*) as count FROM csv WHERE "${groupBy}" IS NOT NULL AND "${groupBy}" != '' GROUP BY "${groupBy}" ORDER BY count DESC`;
        return await duckQuery(sql) as Record<string, string | number>[];
      }
      return countByGroup(buildRows(data), groupBy);
    }

    async function renderChart() {
      if (!containerRef.current || cancelled) return;

      const rows = buildRows(data);
      let chart: SVGSVGElement | HTMLElement | undefined;

      try {
        switch (suggestion.view_type) {
          case "BarChart": {
            const groupBy = suggestion.group_by!;
            const metric = suggestion.metric!;
            const agg = await queryAgg(groupBy, metric, suggestion.aggregate, suggestion.limit);
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
          const agg = await queryAgg(groupBy, metric, suggestion.aggregate);
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
          const xCol = suggestion.x_col || suggestion.metric;
          const yCol = suggestion.y_col || suggestion.group_by;
          if (!xCol || !yCol) break;
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

        case "BoxPlot": {
          const metric = suggestion.metric!;
          const groupBy = suggestion.group_by;
          if (groupBy) {
            chart = Plot.plot({
              height: 250,
              marginLeft: 100,
              x: { label: truncLabel(metric) },
              y: { label: truncLabel(groupBy) },
              marks: [
                Plot.boxX(rows, { x: metric, y: groupBy, fill: "#64748b", fillOpacity: 0.5 }),
              ],
            });
          } else {
            chart = Plot.plot({
              height: 200,
              x: { label: truncLabel(metric) },
              marks: [
                Plot.boxX(rows, { x: metric, fill: "#64748b", fillOpacity: 0.5 }),
              ],
            });
          }
          break;
        }

        case "CountBar":
        case "OrderedBar": {
          const groupBy = suggestion.group_by!;
          const counted = await queryCount(groupBy);
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

        case "Heatmap": {
          const xCol = suggestion.x_col || suggestion.group_by;
          const yCol = suggestion.y_col || suggestion.metric;
          if (xCol && yCol) {
            // If both are categorical, show count heatmap
            // If one is numeric, use it as the fill value
            const yIsNumeric = data.columns.find((c) => c.name === yCol && NUMERIC_TYPES.has(c.col_type));
            if (yIsNumeric && suggestion.group_by) {
              // Single dimension + numeric metric: not a real heatmap, skip
              break;
            } else {
              // Two categoricals — count heatmap
              chart = Plot.plot({
                height: 250,
                marginLeft: 120,
                color: { scheme: "YlOrRd", legend: true, label: "count" },
                marks: [
                  Plot.cell(
                    rows,
                    Plot.group({ fill: "count" }, { x: xCol, y: yCol })
                  ),
                ],
              });
            }
          }
          break;
        }

        case "Choropleth": {
          if (!geoData) break; // still loading
          const geoCol = suggestion.group_by;
          const metric = suggestion.metric;
          if (!geoCol || !metric) break;

          const agg = await queryAgg(geoCol, metric, suggestion.aggregate);
          const lookup = new Map<string, number>();
          for (const row of agg) {
            lookup.set(normalizeGeoName(String(row[geoCol])), Number(row[metric]));
          }

          const topo = geoData as Topology;
          const objectKey = Object.keys(topo.objects).find(k => k !== "land" && k !== "nation") || Object.keys(topo.objects)[0];
          const featureCollection = topojson.feature(topo, topo.objects[objectKey]) as unknown as GeoJSON.FeatureCollection;
          const geoObj = topo.objects[objectKey] as Parameters<typeof topojson.mesh>[1];
          const meshData = topojson.mesh(topo, geoObj);

          // Detect which geo level we're rendering
          const isUS = objectKey === "states";
          const projection = isUS ? "albers-usa" : "equirectangular";

          // Enrich features with data values
          const enrichedFeatures = featureCollection.features.map((f) => ({
            ...f,
            properties: {
              ...f.properties,
              _value: lookup.get(f.properties?.name || "") ?? null,
            },
          }));

          chart = Plot.plot({
            projection,
            height: isUS ? 350 : 300,
            color: {
              scheme: "YlOrRd",
              legend: true,
              label: truncLabel(metric),
              unknown: "#f0f0f0",
            },
            marks: [
              Plot.geo(enrichedFeatures, {
                fill: (d: GeoJSON.Feature) => (d.properties as Record<string, unknown>)?._value as number | null,
                stroke: "white",
                strokeWidth: 0.5,
              }),
              Plot.geo(meshData, { stroke: "white", strokeWidth: 0.5, fill: "none" }),
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

    if (chart && !cancelled && containerRef.current) {
      containerRef.current.innerHTML = "";
      containerRef.current.appendChild(chart);
    }
    } // end renderChart

    renderChart();

    return () => {
      cancelled = true;
      if (containerRef.current) {
        containerRef.current.innerHTML = "";
      }
    };
  }, [data, suggestion, geoData]);

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
