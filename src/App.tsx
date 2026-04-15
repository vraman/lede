import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { save, open } from "@tauri-apps/plugin-dialog";
import { writeTextFile, readTextFile } from "@tauri-apps/plugin-fs";
import { JSONContent } from "@tiptap/react";
import { Editor } from "@tiptap/react";
import { ParsedCSV, ViewSuggestion, AiInsightsResponse, AiViewSuggestion } from "./types";
import { DataContext } from "./DataContext";
import ProseEditor from "./components/ProseEditor";
import DataTable from "./components/DataTable";
import HomeScreen from "./components/HomeScreen";
import AiOptIn, { isAiEnabled } from "./components/AiOptIn";
import QueryBar from "./components/QueryBar";
import { loadCSV as loadDuckDB } from "./duckdb";
import {
  serializeFromEditor,
  parseToEditorJSON,
  LedeMeta,
} from "./ledeFile";
import "./App.css";

/** Build an initial Tiptap document from suggestions. */
function buildInitialDoc(suggestions: ViewSuggestion[]): JSONContent {
  const defaults = suggestions.filter((s) => s.shown_by_default);
  const summaryCards = defaults.filter((s) => s.view_type === "SummaryCard");
  const charts = defaults.filter(
    (s) => s.view_type !== "SummaryCard" && s.view_type !== "Table"
  );

  const nodes: JSONContent[] = [];
  nodes.push({ type: "paragraph" });

  for (const card of summaryCards) {
    nodes.push({ type: "dataview", attrs: { viewConfig: card } });
    nodes.push({ type: "paragraph" });
  }

  for (const chart of charts) {
    nodes.push({ type: "dataview", attrs: { viewConfig: chart } });
    nodes.push({ type: "paragraph" });
  }

  if (nodes[nodes.length - 1]?.type !== "paragraph") {
    nodes.push({ type: "paragraph" });
  }

  return { type: "doc", content: nodes };
}

const VALID_VIEW_TYPES = new Set<string>([
  "Table", "SummaryCard", "LineChart", "BarChart", "CountBar",
  "ScatterPlot", "Histogram", "Heatmap", "OrderedBar", "MultiLineChart", "PointMap",
  "Choropleth",
]);

const VALID_AGGREGATES = new Set<string>(["Sum", "Avg", "Count", "Min", "Max"]);

/** Map AI view_type strings (which may be wrong) to valid ViewType values. */
function normalizeViewType(raw: string): ViewSuggestion["view_type"] {
  if (VALID_VIEW_TYPES.has(raw)) return raw as ViewSuggestion["view_type"];
  // Common AI mistakes
  const lower = raw.toLowerCase().replace(/[\s_-]/g, "");
  const map: Record<string, ViewSuggestion["view_type"]> = {
    barchart: "BarChart",
    linechart: "LineChart",
    scatterplot: "ScatterPlot",
    histogram: "Histogram",
    boxplot: "BoxPlot",
    piechart: "BarChart",
    heatmap: "Heatmap",
    table: "Table",
    summarycard: "SummaryCard",
    countbar: "CountBar",
    orderedbar: "OrderedBar",
    rankedlist: "BarChart",
    multilinechart: "MultiLineChart",
    choropleth: "Choropleth",
    pointmap: "PointMap",
  };
  return map[lower] || "BarChart";
}

/** Convert an AI view suggestion to a full ViewSuggestion.
 *  The AI now provides the correct fields per view type (validated in Rust).
 *  This function only adds frontend metadata. */
function aiViewToSuggestion(aiView: AiViewSuggestion): ViewSuggestion {
  return {
    view_type: normalizeViewType(aiView.view_type),
    title: aiView.title,
    metric: aiView.metric ?? null,
    group_by: aiView.group_by ?? null,
    aggregate: VALID_AGGREGATES.has(aiView.aggregate || "")
      ? (aiView.aggregate as ViewSuggestion["aggregate"])
      : "Sum",
    sort: aiView.sort ?? null,
    limit: aiView.limit ?? null,
    x_col: aiView.x_col ?? null,
    y_col: aiView.y_col ?? null,
    shown_by_default: false,
    suggested_by: "ai",
    insight: aiView.insight ?? null,
    insight_edited: false,
    denominator: null,
    filter_column: aiView.filter_column ?? null,
    filter_value: aiView.filter_value ?? null,
  };
}

function App() {
  const [data, setData] = useState<ParsedCSV | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [sourcePath, setSourcePath] = useState<string>("");
  const [docTitle, setDocTitle] = useState<string>("");
  const [editorContent, setEditorContent] = useState<JSONContent>({
    type: "doc",
    content: [{ type: "paragraph" }],
  });
  const [saveStatus, setSaveStatus] = useState<"" | "saving" | "saved">("");
  const [savePath, setSavePath] = useState<string>("");
  const editorInstanceRef = useRef<Editor | null>(null);
  const metaRef = useRef<LedeMeta | null>(null);
  const [editingTitle, setEditingTitle] = useState(false);
  const [aiLoading, setAiLoading] = useState(false);
  const [aiEnabled, setAiEnabled] = useState(isAiEnabled());
  const aiCancelledRef = useRef(false);
  const [allSuggestions, setAllSuggestions] = useState<ViewSuggestion[]>([]);
  const [duckDbReady, setDuckDbReady] = useState(false);

  const handleFilePick = useCallback(async () => {
    try {
      const selected = await open({
        multiple: false,
        filters: [
          { name: "CSV", extensions: ["csv"] },
          { name: "Lede", extensions: ["lede"] },
        ],
      });
      if (selected) {
        const path = String(selected);
        if (path.endsWith(".lede")) {
          await loadLedeFile(path);
        } else {
          await loadCSV(path);
        }
      }
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const loadCSV = async (path: string) => {
    setLoading(true);
    setError(null);
    setDuckDbReady(false);
    const t0 = performance.now();
    try {
      // Start DuckDB load immediately — runs in parallel with parse_csv
      const t2 = performance.now();
      const duckPromise = loadDuckDB(path).then(async () => {
        console.log(`[perf] DuckDB load: ${(performance.now() - t2).toFixed(0)}ms`);
        // Get real row count from DuckDB
        try {
          const countRows = await invoke<Record<string, unknown>[]>("query_duckdb", {
            sql: "SELECT COUNT(*) as cnt FROM csv",
          });
          if (countRows.length > 0) {
            const realCount = Number(countRows[0].cnt);
            setData((prev) => prev ? { ...prev, row_count: realCount } : prev);
          }
        } catch { /* ok, use parse_csv count */ }
        setDuckDbReady(true);
      }).catch((e) => {
        console.error(`[perf] DuckDB load failed: ${e}`);
      });

      const result = await invoke<ParsedCSV>("parse_csv", { path });
      console.log(`[perf] parse_csv: ${(performance.now() - t0).toFixed(0)}ms`);

      setData(result);
      setSourcePath(path);
      const name = path.split("/").pop()?.replace(".csv", "") || "Untitled";
      setDocTitle(name);
      setEditorContent(buildInitialDoc(result.suggestions));
      setAllSuggestions(result.suggestions);
      metaRef.current = null;
      setSavePath("");

      invoke("add_recent_file", { path, title: name }).catch(() => {});

      // Wait for DuckDB if it hasn't finished yet (usually it has by now)
      await duckPromise;

      // Phase 2: Request AI insights asynchronously (only if enabled)
      if (aiEnabled) {
        const t3 = performance.now();
        requestAiInsights(path).then(() => {
          console.log(`[perf] AI insights: ${(performance.now() - t3).toFixed(0)}ms`);
        });
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
      console.log(`[perf] total loadCSV: ${(performance.now() - t0).toFixed(0)}ms`);
    }
  };

  const handleAiToggle = () => {
    const next = !aiEnabled;
    setAiEnabled(next);
    localStorage.setItem("lede-ai-enabled", next ? "true" : "false");
    if (!next) {
      aiCancelledRef.current = true;
      setAiLoading(false);
      const editor = editorInstanceRef.current;
      if (editor) {
        const nodesToRemove: number[] = [];
        editor.state.doc.descendants((node, pos) => {
          if (node.type.name === "dataview") {
            const vc = node.attrs.viewConfig;
            if (vc && vc.suggested_by === "ai") {
              nodesToRemove.push(pos);
            }
          }
        });
        let tr = editor.state.tr;
        for (const pos of nodesToRemove.reverse()) {
          const node = tr.doc.nodeAt(pos);
          if (node) tr = tr.delete(pos, pos + node.nodeSize);
        }
        if (nodesToRemove.length > 0) editor.view.dispatch(tr);
        setAllSuggestions((prev) => prev.filter((s) => s.suggested_by !== "ai"));
      }
    } else if (sourcePath) {
      requestAiInsights(sourcePath);
    }
  };

  /** Phase 2: Request AI insights and merge into editor. */
  const requestAiInsights = async (csvPath: string) => {
    aiCancelledRef.current = false;
    setAiLoading(true);
    try {
      const response = await invoke<AiInsightsResponse>("generate_ai_insights", {
        path: csvPath,
      });
      // Discard results if AI was toggled off while inference was running
      if (aiCancelledRef.current) return;

      // Split AI views into alternatives (same metric+group_by) vs novel (new cards)
      const editor = editorInstanceRef.current;
      if (editor && response.suggested_views.length > 0) {
        // Collect existing metric+group_by combos from Phase 1
        const existingCombos = new Set<string>();
        const existingKeys = new Set<string>();
        editor.state.doc.descendants((node) => {
          if (node.type.name === "dataview") {
            const vc = node.attrs.viewConfig;
            if (vc) {
              existingCombos.add(`${vc.metric || ""}|${vc.group_by || ""}`);
              existingKeys.add(`${vc.metric || ""}|${vc.group_by || ""}|${vc.view_type}`);
            }
          }
        });

        const novelNodes: JSONContent[] = [];
        const newAlternatives: ViewSuggestion[] = [];

        for (const aiView of response.suggested_views) {
          const viewConfig = aiViewToSuggestion(aiView);
          const combo = `${viewConfig.metric || ""}|${viewConfig.group_by || ""}`;
          const key = `${combo}|${viewConfig.view_type}`;

          if (existingKeys.has(key)) {
            // Exact duplicate — skip entirely
            continue;
          } else if (existingCombos.has(combo)) {
            // Same metric+group_by, different view_type — add as alternative toggle
            newAlternatives.push(viewConfig);
          } else {
            // Genuinely new combo — insert as new card
            novelNodes.push({ type: "paragraph" });
            novelNodes.push({ type: "dataview", attrs: { viewConfig } });
          }
        }

        // Merge new alternatives into suggestions so switcher chips pick them up,
        // and swap existing Phase 1 views to the AI-suggested view type
        if (newAlternatives.length > 0) {
          setAllSuggestions((prev) => [...prev, ...newAlternatives]);

          // Build lookup: metric+group_by → AI viewConfig
          const aiSwapMap = new Map<string, ViewSuggestion>();
          for (const alt of newAlternatives) {
            const combo = `${alt.metric || ""}|${alt.group_by || ""}`;
            aiSwapMap.set(combo, alt);
          }

          // Walk editor nodes — add AI insight to matching Phase 1 views
          // but do NOT swap the view type or config (that was breaking charts)
          editor.state.doc.descendants((node, pos) => {
            if (node.type.name === "dataview") {
              const vc = node.attrs.viewConfig;
              if (vc && vc.suggested_by !== "ai") {
                const combo = `${vc.metric || ""}|${vc.group_by || ""}`;
                const aiAlt = aiSwapMap.get(combo);
                if (aiAlt && aiAlt.insight) {
                  // Only add the AI insight, keep everything else
                  const tr = editor.state.tr.setNodeAttribute(pos, "viewConfig", {
                    ...vc,
                    insight: aiAlt.insight,
                  });
                  editor.view.dispatch(tr);
                }
              }
            }
          });
        }

        // Insert standalone insights
        if (response.insights.length > 0) {
          novelNodes.push({ type: "paragraph" });
          novelNodes.push({
            type: "paragraph",
            content: [
              {
                type: "text",
                marks: [{ type: "italic" }],
                text: response.insights.join(" \u2022 "),
              },
            ],
          });
        }

        // Append novel views at end of document
        if (novelNodes.length > 0) {
          const endPos = editor.state.doc.content.size - 1;
          editor
            .chain()
            .focus()
            .insertContentAt(endPos, novelNodes)
            .run();
        }
      }
      // Auto-set document title if still the filename default
      if (response.suggested_title) {
        const currentIsDefault = docTitle === sourcePath.split("/").pop()?.replace(".csv", "");
        if (currentIsDefault) {
          setDocTitle(response.suggested_title);
        }
      }

      // Update summary card if AI suggests a better one
      if (response.suggested_summary) {
        const editor = editorInstanceRef.current;
        if (editor) {
          const summary = { ...response.suggested_summary };
          // Strip "by X" from summary title — summary card shows a single number, not per-group
          summary.title = summary.title.replace(/\s+by\s+\w+.*/i, "");
          editor.state.doc.descendants((node, pos) => {
            if (node.type.name === "dataview") {
              const vc = node.attrs.viewConfig;
              if (vc && vc.view_type === "SummaryCard" && vc.suggested_by !== "ai") {
                const tr = editor.state.tr.setNodeAttribute(pos, "viewConfig", {
                  ...vc,
                  title: summary.title,
                  metric: summary.metric,
                  aggregate: summary.aggregate,
                  denominator: summary.denominator || null,
                  suggested_by: "ai",
                });
                editor.view.dispatch(tr);
                return false; // stop after first
              }
            }
          });
        }
      }
    } catch (e) {
      console.error("[ai] generate_ai_insights failed:", e);
    } finally {
      setAiLoading(false);
    }
  };

  const loadLedeFile = async (path: string) => {
    try {
      const content = await readTextFile(path);
      const { meta, json } = parseToEditorJSON(content);
      metaRef.current = meta;
      setSavePath(path);

      const title =
        meta.title || path.split("/").pop()?.replace(".lede", "") || "Untitled";
      setDocTitle(title);
      invoke("add_recent_file", { path, title }).catch(() => {});

      if (meta.sources.length > 0) {
        const csvPath = meta.sources[0];
        try {
          const result = await invoke<ParsedCSV>("parse_csv", {
            path: csvPath,
          });
          setData(result);
          setSourcePath(csvPath);
        } catch {
          const dir = path.substring(0, path.lastIndexOf("/") + 1);
          try {
            const result = await invoke<ParsedCSV>("parse_csv", {
              path: dir + csvPath,
            });
            setData(result);
            setSourcePath(dir + csvPath);
          } catch {
            setError(`Could not load source CSV: ${csvPath}`);
          }
        }
      }

      setEditorContent(json);
    } catch (e) {
      setError(String(e));
    }
  };

  const doSave = useCallback(async (targetPath?: string) => {
    if (!data) return;

    const editor = editorInstanceRef.current;
    const currentJSON = editor ? editor.getJSON() : editorContent;

    const filename =
      sourcePath.split("/").pop()?.replace(".csv", "") || "untitled";
    const today = new Date().toISOString().split("T")[0];

    const meta: LedeMeta = {
      title: docTitle || filename,
      created: metaRef.current?.created || today,
      sources: metaRef.current?.sources || (sourcePath
        ? [sourcePath.split("/").pop() || sourcePath]
        : []),
    };
    metaRef.current = meta;

    const fileContent = serializeFromEditor(meta, currentJSON);
    let finalPath = targetPath || savePath;

    try {
      if (!finalPath) {
        const picked = await save({
          filters: [{ name: "Lede", extensions: ["lede"] }],
          defaultPath: `${filename}.lede`,
        });
        if (!picked) return;
        finalPath = picked;
        setSavePath(finalPath);
      }

      setSaveStatus("saving");
      await writeTextFile(finalPath, fileContent);
      invoke("add_recent_file", {
        path: finalPath,
        title: meta.title || filename,
      }).catch(() => {});
      setSaveStatus("saved");
      setTimeout(() => setSaveStatus(""), 2000);
    } catch (e) {
      setError(String(e));
      setSaveStatus("");
    }
  }, [data, sourcePath, editorContent, docTitle, savePath]);

  // Cmd+S to save
  useEffect(() => {
    const handleKeyDown = async (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "s") {
        e.preventDefault();
        await doSave();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [doSave]);

  // Tauri drag-and-drop
  useEffect(() => {
    const unlisten = setupDragDrop();
    return () => {
      unlisten.then((fn) => fn?.());
    };
  }, []);

  const setupDragDrop = async () => {
    try {
      const { getCurrentWebviewWindow } = await import(
        "@tauri-apps/api/webviewWindow"
      );
      const currentWindow = getCurrentWebviewWindow();
      const unlisten = await currentWindow.onDragDropEvent((event) => {
        if (event.payload.type === "drop") {
          const paths = event.payload.paths;
          if (paths.length > 0) {
            const path = paths[0];
            if (path.endsWith(".csv")) {
              loadCSV(path);
            } else if (path.endsWith(".lede")) {
              loadLedeFile(path);
            }
          }
        }
      });
      return unlisten;
    } catch {
      return undefined;
    }
  };

  if (!data) {
    return (
      <HomeScreen
        onOpenFile={(path) => {
          if (path.endsWith(".lede")) {
            loadLedeFile(path);
          } else {
            loadCSV(path);
          }
        }}
        onNewNotebook={handleFilePick}
        loading={loading}
        error={error}
      />
    );
  }

  const sourceFilename = sourcePath.split("/").pop() || "";

  return (
    <DataContext.Provider value={{ data, suggestions: allSuggestions, sourceFilename, aiLoading, duckDbReady }}>
      <main className="app workspace">
        <header className="app-header">
          {editingTitle ? (
            <input
              className="title-input"
              value={docTitle}
              onChange={(e) => setDocTitle(e.target.value)}
              onBlur={() => setEditingTitle(false)}
              onKeyDown={(e) => {
                if (e.key === "Enter") setEditingTitle(false);
              }}
              autoFocus
            />
          ) : (
            <h1
              className={`doc-title ${!docTitle ? "untitled" : ""}`}
              onClick={() => setEditingTitle(true)}
            >
              {docTitle || "Untitled Notebook"}
            </h1>
          )}
          <div className="header-actions">
            {aiLoading && (
              <span className="ai-loading-indicator">AI analyzing...</span>
            )}
            {saveStatus === "saved" && (
              <span className="save-indicator">Saved</span>
            )}
            <button onClick={handleFilePick} className="btn-open">
              Open
            </button>
            <button onClick={() => doSave()} className="btn-save">
              Save
            </button>
            <button className={`btn-ai-toggle ${aiEnabled ? "active" : ""}`} onClick={handleAiToggle}>
              AI {aiEnabled ? "On" : "Off"}
            </button>
          </div>
        </header>

        {error && <p className="error">{error}</p>}

        {!aiEnabled && (
          <AiOptIn onEnabled={() => {
            setAiEnabled(true);
            if (sourcePath) requestAiInsights(sourcePath);
          }} />
        )}

        <div className="editor-layout">
          <ProseEditor
            initialContent={editorContent}
            onUpdate={setEditorContent}
            editorRef={(editor) => {
              editorInstanceRef.current = editor;
            }}
          />
          {duckDbReady && <QueryBar />}
          <DataTable data={data} duckDbReady={duckDbReady} />
        </div>
      </main>
    </DataContext.Provider>
  );
}

export default App;
