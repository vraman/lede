import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { save, open } from "@tauri-apps/plugin-dialog";
import { writeTextFile, readTextFile } from "@tauri-apps/plugin-fs";
import { ParsedCSV, ViewSuggestion } from "./types";
import SummaryCard from "./components/SummaryCard";
import ChartView from "./components/ChartView";
import DataTable from "./components/DataTable";
import ProseEditor from "./components/ProseEditor";
import { serializeLede, parseLede, LedeDocument } from "./ledeFile";
import "./App.css";

function App() {
  const [data, setData] = useState<ParsedCSV | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [prose, setProse] = useState("<p>Write your analysis here...</p>");
  const [sourcePath, setSourcePath] = useState<string>("");
  const [visibleViews, setVisibleViews] = useState<ViewSuggestion[]>([]);
  // Ref to pass saved views from .lede file to the data-load useEffect
  const ledeViewsRef = useRef<ViewSuggestion[] | null>(null);

  // Progressive rendering: show views one at a time
  useEffect(() => {
    if (!data) return;

    // If loading from a .lede file, use saved views instead of auto-generated
    if (ledeViewsRef.current) {
      setVisibleViews(ledeViewsRef.current);
      ledeViewsRef.current = null;
      return;
    }

    setVisibleViews([]);
    const defaults = data.suggestions.filter((s) => s.shown_by_default);
    const summaryCards = defaults.filter((s) => s.view_type === "SummaryCard");
    const others = defaults.filter((s) => s.view_type !== "SummaryCard");
    const ordered = [...summaryCards, ...others];

    const timers: ReturnType<typeof setTimeout>[] = [];
    ordered.forEach((view, i) => {
      timers.push(
        setTimeout(() => {
          setVisibleViews((prev) => [...prev, view]);
        }, i * 100)
      );
    });

    return () => timers.forEach(clearTimeout);
  }, [data]);

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
        const path = typeof selected === "string" ? selected : selected;
        if (String(path).endsWith(".lede")) {
          await loadLedeFile(String(path));
        } else {
          await loadCSV(String(path));
        }
      }
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const loadCSV = async (path: string) => {
    setLoading(true);
    setError(null);
    try {
      const result = await invoke<ParsedCSV>("parse_csv", { path });
      setData(result);
      setSourcePath(path);
      setProse("<p>Write your analysis here...</p>");
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  const loadLedeFile = async (path: string) => {
    try {
      const content = await readTextFile(path);
      const doc = parseLede(content);
      setProse(doc.prose || "<p>Write your analysis here...</p>");

      // Stage saved views so the useEffect picks them up when data arrives
      if (doc.views.length > 0) {
        ledeViewsRef.current = doc.views;
      }

      // If there are source files, try to load the first one
      if (doc.sources.length > 0) {
        const csvPath = doc.sources[0];
        // Try absolute path first, then relative to the lede file
        try {
          const result = await invoke<ParsedCSV>("parse_csv", { path: csvPath });
          setData(result);
          setSourcePath(csvPath);
        } catch {
          // Try relative path
          const dir = path.substring(0, path.lastIndexOf("/") + 1);
          try {
            const result = await invoke<ParsedCSV>("parse_csv", {
              path: dir + csvPath,
            });
            setData(result);
            setSourcePath(dir + csvPath);
          } catch (e) {
            setError(`Could not load source CSV: ${csvPath}`);
          }
        }
      }
    } catch (e) {
      setError(String(e));
    }
  };

  // Cmd+S to save
  useEffect(() => {
    const handleKeyDown = async (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "s") {
        e.preventDefault();
        await handleSave();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [data, prose, sourcePath, visibleViews]);

  const handleSave = async () => {
    if (!data) return;

    const filename = sourcePath.split("/").pop()?.replace(".csv", "") || "untitled";
    const today = new Date().toISOString().split("T")[0];

    const doc: LedeDocument = {
      title: filename,
      created: today,
      sources: sourcePath ? [sourcePath.split("/").pop() || sourcePath] : [],
      prose,
      views: visibleViews,
    };

    const content = serializeLede(doc);

    try {
      const savePath = await save({
        filters: [{ name: "Lede", extensions: ["lede"] }],
        defaultPath: `${filename}.lede`,
      });
      if (savePath) {
        await writeTextFile(savePath, content);
      }
    } catch (e) {
      setError(String(e));
    }
  };

  // Handle Tauri drag-and-drop via event listener
  useEffect(() => {
    const unlisten = setupDragDrop();
    return () => {
      unlisten.then((fn) => fn?.());
    };
  }, []);

  const setupDragDrop = async () => {
    try {
      const { getCurrentWebviewWindow } = await import("@tauri-apps/api/webviewWindow");
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
      <main className="app">
        <div
          className={`drop-zone ${loading ? "loading" : ""}`}
          onClick={handleFilePick}
        >
          <div className="drop-zone-content">
            <h1>Lede</h1>
            <p className="tagline">Don't bury the lede in your data.</p>
            {loading ? (
              <p className="drop-hint">Loading...</p>
            ) : (
              <p className="drop-hint">
                Drop a CSV file here, or click to browse
              </p>
            )}
            {error && <p className="error">{error}</p>}
          </div>
        </div>
      </main>
    );
  }

  const handleSwapView = (index: number, newView: ViewSuggestion) => {
    setVisibleViews((prev) => {
      const updated = [...prev];
      updated[index] = newView;
      return updated;
    });
  };

  const handleChangeAggregate = (index: number, agg: ViewSuggestion["aggregate"]) => {
    setVisibleViews((prev) => {
      const updated = [...prev];
      updated[index] = { ...updated[index], aggregate: agg };
      return updated;
    });
  };

  const summaryCards = visibleViews.filter((v) => v.view_type === "SummaryCard");
  const charts = visibleViews.filter(
    (v) => v.view_type !== "SummaryCard" && v.view_type !== "Table"
  );

  return (
    <main className="app workspace">
      <header className="app-header">
        <h1>Lede</h1>
        <div className="header-actions">
          <button onClick={handleFilePick} className="btn-secondary">
            Open
          </button>
          <button onClick={handleSave} className="btn-primary">
            Save
          </button>
        </div>
      </header>

      {error && <p className="error">{error}</p>}

      <div className="workspace-layout">
        <div className="views-panel">
          {summaryCards.length > 0 && (
            <div className="summary-row">
              {summaryCards.map((s, i) => (
                <SummaryCard key={i} data={data} suggestion={s} />
              ))}
            </div>
          )}

          <div className="charts-grid">
            {charts.map((s) => {
              const viewIndex = visibleViews.indexOf(s);
              const alternatives = data.suggestions.filter(
                (alt) =>
                  alt.view_type !== s.view_type &&
                  alt.metric === s.metric &&
                  alt.group_by === s.group_by &&
                  alt.view_type !== "SummaryCard"
              );
              return (
                <ChartView
                  key={`${viewIndex}-${s.view_type}`}
                  data={data}
                  suggestion={s}
                  alternatives={alternatives}
                  onSwap={(alt) => handleSwapView(viewIndex, alt)}
                  onChangeAggregate={(agg) => handleChangeAggregate(viewIndex, agg)}
                />
              );
            })}
          </div>

          <DataTable data={data} />
        </div>

        <div className="prose-panel">
          <ProseEditor content={prose} onUpdate={setProse} />
        </div>
      </div>
    </main>
  );
}

export default App;
