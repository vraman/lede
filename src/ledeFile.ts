import { ViewSuggestion } from "./types";
import { JSONContent } from "@tiptap/react";

export interface LedeMeta {
  title: string;
  created: string;
  sources: string[];
}

/** Serialize Tiptap editor JSON + metadata into a .lede file string. */
export function serializeFromEditor(meta: LedeMeta, editorJSON: JSONContent): string {
  const lines: string[] = [];
  lines.push("---");
  lines.push(`title: ${meta.title}`);
  lines.push(`created: ${meta.created}`);
  lines.push("sources:");
  for (const src of meta.sources) {
    lines.push(`  - ${src}`);
  }
  lines.push("---");
  lines.push("");

  const nodes = editorJSON.content || [];
  for (const node of nodes) {
    if (node.type === "dataview") {
      const vc = node.attrs?.viewConfig as ViewSuggestion | undefined;
      if (!vc) continue;
      lines.push("```dataview");
      if (meta.sources.length > 0) {
        lines.push(`source: ${meta.sources[0]}`);
      }
      lines.push(`view: ${viewTypeToString(vc.view_type)}`);
      if (vc.metric) lines.push(`metric: ${vc.metric}`);
      lines.push(`aggregate: ${vc.aggregate.toLowerCase()}`);
      if (vc.group_by) lines.push(`group_by: ${vc.group_by}`);
      if (vc.sort) lines.push(`sort: ${vc.sort}`);
      if (vc.limit) lines.push(`limit: ${vc.limit}`);
      if (vc.x_col) lines.push(`x_col: ${vc.x_col}`);
      if (vc.y_col) lines.push(`y_col: ${vc.y_col}`);
      lines.push(`suggested_by: ${vc.suggested_by || "rule"}`);
      if (vc.insight) {
        lines.push(`insight: ${vc.insight}`);
      }
      if (vc.insight_edited) {
        lines.push("insight_edited: true");
      }
      lines.push("```");
      lines.push("");
    } else if (node.type === "heading") {
      const level = node.attrs?.level || 1;
      const text = extractText(node);
      lines.push(`${"#".repeat(level)} ${text}`);
      lines.push("");
    } else if (node.type === "paragraph") {
      const text = extractText(node);
      lines.push(text);
      lines.push("");
    }
  }

  return lines.join("\n");
}

/** Parse a .lede file string into Tiptap JSONContent for the editor. */
export function parseToEditorJSON(content: string): { meta: LedeMeta; json: JSONContent } {
  const meta: LedeMeta = { title: "", created: "", sources: [] };

  const fmMatch = content.match(/^---\n([\s\S]*?)\n---/);
  if (fmMatch) {
    const fm = fmMatch[1];
    const titleMatch = fm.match(/title:\s*(.+)/);
    if (titleMatch) meta.title = titleMatch[1].trim();
    const createdMatch = fm.match(/created:\s*(.+)/);
    if (createdMatch) meta.created = createdMatch[1].trim();
    const sourceMatches = fm.matchAll(/^\s+-\s+(.+)$/gm);
    for (const m of sourceMatches) {
      meta.sources.push(m[1].trim());
    }
  }

  const body = content.replace(/^---\n[\s\S]*?\n---\n*/, "");
  const nodes: JSONContent[] = [];

  // Split body into segments: dataview blocks and text
  const segments = body.split(/(```dataview\n[\s\S]*?```)/g);

  for (const seg of segments) {
    if (seg.startsWith("```dataview")) {
      // Parse dataview block
      const blockContent = seg.replace(/^```dataview\n/, "").replace(/```$/, "");
      const view = parseDataviewBlock(blockContent);

      // Ensure a paragraph before if last node was also a dataview
      if (nodes.length > 0 && nodes[nodes.length - 1].type === "dataview") {
        nodes.push({ type: "paragraph" });
      }

      nodes.push({
        type: "dataview",
        attrs: { viewConfig: view },
      });
    } else {
      // Text segment — split into paragraphs and headings
      const textLines = seg.split("\n");
      let currentPara: string[] = [];

      const flushPara = () => {
        const text = currentPara.join("\n").trim();
        if (text) {
          nodes.push({
            type: "paragraph",
            content: [{ type: "text", text }],
          });
        }
        currentPara = [];
      };

      for (const line of textLines) {
        const headingMatch = line.match(/^(#{1,3})\s+(.+)$/);
        if (headingMatch) {
          flushPara();
          nodes.push({
            type: "heading",
            attrs: { level: headingMatch[1].length },
            content: [{ type: "text", text: headingMatch[2] }],
          });
        } else if (line.trim() === "") {
          flushPara();
        } else {
          currentPara.push(line);
        }
      }
      flushPara();
    }
  }

  // Ensure document starts and ends with a paragraph, and consecutive dataviews have paragraphs between
  const padded: JSONContent[] = [];
  for (let i = 0; i < nodes.length; i++) {
    const node = nodes[i];
    if (node.type === "dataview") {
      // Add paragraph before if first or after another dataview
      if (padded.length === 0 || padded[padded.length - 1].type === "dataview") {
        padded.push({ type: "paragraph" });
      }
    }
    padded.push(node);
  }
  // End with a paragraph
  if (padded.length === 0 || padded[padded.length - 1].type !== "paragraph") {
    padded.push({ type: "paragraph" });
  }

  return {
    meta,
    json: { type: "doc", content: padded },
  };
}

// --- Helpers ---

function extractText(node: JSONContent): string {
  if (!node.content) return "";
  return node.content
    .map((child) => {
      if (child.type === "text") return child.text || "";
      return extractText(child);
    })
    .join("");
}

function parseDataviewBlock(block: string): ViewSuggestion {
  const view: ViewSuggestion = {
    view_type: "Table" as any,
    title: "",
    metric: null,
    group_by: null,
    aggregate: "Sum" as any,
    sort: null,
    limit: null,
    x_col: null,
    y_col: null,
    shown_by_default: true,
    suggested_by: "rule",
    insight: null,
    insight_edited: false,
    denominator: null,
  };

  for (const line of block.split("\n")) {
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
      case "x_col":
        view.x_col = val.trim();
        break;
      case "y_col":
        view.y_col = val.trim();
        break;
      case "suggested_by":
        view.suggested_by = val.trim();
        break;
      case "insight":
        if (val.trim() !== "null") {
          view.insight = val.trim();
        }
        break;
      case "insight_edited":
        view.insight_edited = val.trim() === "true";
        break;
      // time_column: ignored (removed)
    }
  }
  return view;
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
    BoxPlot: "box_plot",
    OrderedBar: "ordered_bar",
    // RankedList removed — BarChart with limit handles this
    MultiLineChart: "multi_line_chart",
    Choropleth: "choropleth",
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
    box_plot: "BoxPlot",
    ordered_bar: "OrderedBar",
    ranked_list: "BarChart",
    multi_line_chart: "MultiLineChart",
    choropleth: "Choropleth",
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
