import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ModelStatus } from "../types";

interface RecentFile {
  path: string;
  title: string;
  last_opened: string;
}

interface Props {
  onOpenFile: (path: string) => void;
  onNewNotebook: () => void;
  loading: boolean;
  error: string | null;
}

export default function HomeScreen({ onOpenFile, onNewNotebook, loading, error }: Props) {
  const [recents, setRecents] = useState<RecentFile[]>([]);
  const [downloadProgress, setDownloadProgress] = useState<number | null>(null);

  useEffect(() => {
    invoke<RecentFile[]>("get_recent_files")
      .then(setRecents)
      .catch(() => {});

    // Check model status and start download if needed
    invoke<ModelStatus>("check_model_status")
      .then((status) => {
        console.log("[model] status:", status);
        if (!status.downloaded && !status.downloading) {
          console.log("[model] starting download...");
          invoke("start_model_download").catch((e) =>
            console.error("[model] start_model_download failed:", e)
          );
          setDownloadProgress(0);
        } else if (status.downloading) {
          setDownloadProgress(status.progress);
        }
      })
      .catch((e) => console.error("[model] check_model_status failed:", e));

    // Listen for download progress events
    const unlistenProgress = listen<number>("model-download-progress", (event) => {
      setDownloadProgress(event.payload);
    });
    const unlistenComplete = listen("model-download-complete", () => {
      console.log("[model] download complete");
      setDownloadProgress(null);
    });
    const unlistenError = listen<string>("model-download-error", (event) => {
      console.error("[model] download error:", event.payload);
      setDownloadProgress(null);
    });

    return () => {
      unlistenProgress.then((fn) => fn());
      unlistenComplete.then((fn) => fn());
      unlistenError.then((fn) => fn());
    };
  }, []);

  const formatDate = (iso: string) => {
    try {
      const d = new Date(iso);
      const now = new Date();
      const diffMs = now.getTime() - d.getTime();
      const diffDays = Math.floor(diffMs / (1000 * 60 * 60 * 24));
      if (diffDays === 0) return "Today";
      if (diffDays === 1) return "Yesterday";
      if (diffDays < 7) return `${diffDays} days ago`;
      return d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
    } catch {
      return "";
    }
  };

  const filename = (path: string) => path.split("/").pop() || path;

  return (
    <main className="app">
      <div className="home-screen">
        <div className="home-header">
          <h1 className="home-title">Lede</h1>
          <p className="home-tagline">Don't bury the lede in your data.</p>
        </div>

        <div className="home-actions">
          <button className="home-new-btn" onClick={onNewNotebook}>
            New Notebook
          </button>
        </div>

        {recents.length > 0 && (
          <div className="home-recents">
            <h2 className="home-section-title">Recent</h2>
            <ul className="recent-list">
              {recents.map((r) => (
                <li key={r.path}>
                  <button
                    className="recent-item"
                    onClick={() => onOpenFile(r.path)}
                  >
                    <span className="recent-title">{r.title}</span>
                    <span className="recent-meta">
                      <span className="recent-filename">{filename(r.path)}</span>
                      <span className="recent-date">{formatDate(r.last_opened)}</span>
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          </div>
        )}

        {loading && <p className="home-status">Loading...</p>}
        {error && <p className="error">{error}</p>}

        <p className="home-hint">
          Drop a CSV or .lede file anywhere, or click New Notebook
        </p>

        {downloadProgress !== null && (
          <div className="model-download-banner">
            <span className="model-download-text">
              Setting up AI insights...
            </span>
            <div className="model-download-bar">
              <div
                className="model-download-fill"
                style={{ width: `${Math.round(downloadProgress * 100)}%` }}
              />
            </div>
            <span className="model-download-pct">
              {Math.round(downloadProgress * 100)}%
            </span>
          </div>
        )}
      </div>
    </main>
  );
}
