import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { save, open } from "@tauri-apps/plugin-dialog";
import { writeTextFile, readTextFile } from "@tauri-apps/plugin-fs";
import { JSONContent } from "@tiptap/react";
import { Editor } from "@tiptap/react";
import { ParsedCSV, ViewSuggestion } from "./types";
import { DataContext } from "./DataContext";
import ProseEditor from "./components/ProseEditor";
import HomeScreen from "./components/HomeScreen";
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
    try {
      const result = await invoke<ParsedCSV>("parse_csv", { path });
      setData(result);
      setSourcePath(path);
      const name = path.split("/").pop()?.replace(".csv", "") || "Untitled";
      setDocTitle(name);
      setEditorContent(buildInitialDoc(result.suggestions));
      metaRef.current = null;
      setSavePath("");
      invoke("add_recent_file", { path, title: name }).catch(() => {});
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
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
    <DataContext.Provider value={{ data, suggestions: data.suggestions, sourceFilename }}>
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
            {saveStatus === "saved" && (
              <span className="save-indicator">Saved</span>
            )}
            <button onClick={handleFilePick} className="btn-open">
              Open
            </button>
            <button onClick={() => doSave()} className="btn-save">
              Save
            </button>
          </div>
        </header>

        {error && <p className="error">{error}</p>}

        <div className="editor-layout">
          <ProseEditor
            initialContent={editorContent}
            onUpdate={setEditorContent}
            editorRef={(editor) => {
              editorInstanceRef.current = editor;
            }}
          />
        </div>
      </main>
    </DataContext.Provider>
  );
}

export default App;
