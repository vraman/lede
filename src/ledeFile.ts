import { ViewSuggestion } from "./types";

export interface LedeDocument {
  title: string;
  created: string;
  sources: string[];
  prose: string;
  views: ViewSuggestion[];
}

export function serializeLede(doc: LedeDocument): string {
  const lines: string[] = [];
  lines.push("---");
  lines.push(`title: ${doc.title}`);
  lines.push(`created: ${doc.created}`);
  lines.push("sources:");
  for (const src of doc.sources) {
    lines.push(`  - ${src}`);
  }
  lines.push("---");
  lines.push("");

  // Convert HTML prose to simple markdown-ish content
  // Strip tags for basic conversion
  const proseText = doc.prose
    .replace(/<h1[^>]*>(.*?)<\/h1>/gi, "# $1\n")
    .replace(/<h2[^>]*>(.*?)<\/h2>/gi, "## $1\n")
    .replace(/<h3[^>]*>(.*?)<\/h3>/gi, "### $1\n")
    .replace(/<p[^>]*>(.*?)<\/p>/gi, "$1\n")
    .replace(/<br\s*\/?>/gi, "\n")
    .replace(/<[^>]+>/g, "")
    .trim();

  if (proseText) {
    lines.push(proseText);
    lines.push("");
  }

  for (const view of doc.views) {
    lines.push("```dataview");
    if (doc.sources.length > 0) {
      lines.push(`source: ${doc.sources[0]}`);
    }
    lines.push(`view: ${viewTypeToString(view.view_type)}`);
    if (view.metric) lines.push(`metric: ${view.metric}`);
    lines.push(`aggregate: ${view.aggregate.toLowerCase()}`);
    if (view.group_by) lines.push(`group_by: ${view.group_by}`);
    if (view.sort) lines.push(`sort: ${view.sort}`);
    if (view.limit) lines.push(`limit: ${view.limit}`);
    lines.push("suggested_by: rule");
    lines.push(`last_verified: ${doc.created}`);
    lines.push("insight: null");
    lines.push("```");
    lines.push("");
  }

  return lines.join("\n");
}

function viewTypeToString(vt: string): string {
  const map: Record<string, string> = {
    Table: "table",
    SummaryCard: "summary_card",
    LineChart: "line_chart",
    BarChart: "bar_chart",
    CountBar: "count_bar",
    ScatterPlot: "scatter_plot",
    Histogram: "histogram",
    Heatmap: "heatmap",
    OrderedBar: "ordered_bar",
    RankedList: "ranked_list",
    MultiLineChart: "multi_line_chart",
  };
  return map[vt] || vt.toLowerCase();
}

function stringToViewType(s: string): string {
  const map: Record<string, string> = {
    table: "Table",
    summary_card: "SummaryCard",
    line_chart: "LineChart",
    bar_chart: "BarChart",
    count_bar: "CountBar",
    scatter_plot: "ScatterPlot",
    histogram: "Histogram",
    heatmap: "Heatmap",
    ordered_bar: "OrderedBar",
    ranked_list: "RankedList",
    multi_line_chart: "MultiLineChart",
  };
  return map[s] || s;
}

function stringToAggregate(s: string): string {
  const map: Record<string, string> = {
    sum: "Sum",
    avg: "Avg",
    count: "Count",
    min: "Min",
    max: "Max",
  };
  return map[s] || "Sum";
}

export function parseLede(content: string): LedeDocument {
  const doc: LedeDocument = {
    title: "",
    created: "",
    sources: [],
    prose: "",
    views: [],
  };

  // Parse frontmatter
  const fmMatch = content.match(/^---\n([\s\S]*?)\n---/);
  if (fmMatch) {
    const fm = fmMatch[1];
    const titleMatch = fm.match(/title:\s*(.+)/);
    if (titleMatch) doc.title = titleMatch[1].trim();
    const createdMatch = fm.match(/created:\s*(.+)/);
    if (createdMatch) doc.created = createdMatch[1].trim();
    const sourceMatches = fm.matchAll(/^\s+-\s+(.+)$/gm);
    for (const m of sourceMatches) {
      doc.sources.push(m[1].trim());
    }
  }

  // Extract body after frontmatter
  const body = content.replace(/^---\n[\s\S]*?\n---\n*/, "");

  // Extract dataview blocks
  const dataviewRegex = /```dataview\n([\s\S]*?)```/g;
  let match;
  while ((match = dataviewRegex.exec(body)) !== null) {
    const block = match[1];
    const view: ViewSuggestion = {
      view_type: "Table" as any,
      metric: null,
      group_by: null,
      aggregate: "Sum" as any,
      sort: null,
      limit: null,
      x_col: null,
      y_col: null,
      shown_by_default: true,
    };

    const lines = block.split("\n");
    for (const line of lines) {
      const kv = line.match(/^(\w+):\s*(.+)$/);
      if (!kv) continue;
      const [, key, val] = kv;
      switch (key) {
        case "view":
          view.view_type = stringToViewType(val.trim()) as any;
          break;
        case "metric":
          view.metric = val.trim();
          break;
        case "aggregate":
          view.aggregate = stringToAggregate(val.trim()) as any;
          break;
        case "group_by":
          view.group_by = val.trim();
          break;
        case "sort":
          view.sort = val.trim();
          break;
        case "limit":
          view.limit = parseInt(val.trim(), 10) || null;
          break;
      }
    }

    doc.views.push(view);
  }

  // Extract prose (everything that's not a dataview block)
  const proseText = body
    .replace(/```dataview\n[\s\S]*?```\n*/g, "")
    .trim();
  if (proseText) {
    doc.prose = `<p>${proseText.replace(/\n\n/g, "</p><p>").replace(/\n/g, "<br>")}</p>`;
  }

  return doc;
}
