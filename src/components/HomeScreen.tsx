import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

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

  useEffect(() => {
    invoke<RecentFile[]>("get_recent_files")
      .then(setRecents)
      .catch(() => {});
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
      </div>
    </main>
  );
}
