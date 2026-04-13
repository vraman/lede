import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ModelStatus } from "../types";

const DISMISS_KEY = "lede-ai-dismiss-count";
const ENABLED_KEY = "lede-ai-enabled";

interface Props {
  onEnabled: () => void;
}

export default function AiOptIn({ onEnabled }: Props) {
  const [visible, setVisible] = useState(false);
  const [downloading, setDownloading] = useState(false);
  const [progress, setProgress] = useState(0);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    // Don't show if already enabled
    if (localStorage.getItem(ENABLED_KEY) === "true") {
      return;
    }

    // Don't show if dismissed too many times
    const dismissals = parseInt(localStorage.getItem(DISMISS_KEY) || "0", 10);
    if (dismissals >= 3) {
      return;
    }

    // Check if model is already downloaded (user may have enabled before)
    invoke<ModelStatus>("check_model_status")
      .then((status) => {
        if (status.downloaded) {
          localStorage.setItem(ENABLED_KEY, "true");
          onEnabled();
        } else if (status.downloading) {
          setDownloading(true);
          setProgress(status.progress);
          setVisible(true);
        } else {
          setVisible(true);
        }
      })
      .catch(() => setVisible(true));

    const unlistenProgress = listen<number>("model-download-progress", (event) => {
      setProgress(event.payload);
    });
    const unlistenComplete = listen("model-download-complete", () => {
      setDownloading(false);
      setVisible(false);
      localStorage.setItem(ENABLED_KEY, "true");
      onEnabled();
    });
    const unlistenError = listen<string>("model-download-error", () => {
      setDownloading(false);
      setError("Download failed. You can try again later.");
    });

    return () => {
      unlistenProgress.then((fn) => fn());
      unlistenComplete.then((fn) => fn());
      unlistenError.then((fn) => fn());
    };
  }, [onEnabled]);

  if (!visible) return null;

  const handleEnable = () => {
    setDownloading(true);
    setError(null);
    invoke("start_model_download").catch(() => {
      setError("Could not start download. Check your internet connection.");
      setDownloading(false);
    });
  };

  const handleDismiss = () => {
    const count = parseInt(localStorage.getItem(DISMISS_KEY) || "0", 10) + 1;
    localStorage.setItem(DISMISS_KEY, String(count));
    setVisible(false);
  };

  return (
    <div className="ai-optin-card">
      {downloading ? (
        <>
          <span className="ai-optin-text">
            Downloading AI model (2.5 GB) — charts work while this finishes
          </span>
          <div className="model-download-bar">
            <div
              className="model-download-fill"
              style={{ width: `${Math.round(progress * 100)}%` }}
            />
          </div>
          <span className="model-download-pct">
            {Math.round(progress * 100)}%
          </span>
        </>
      ) : (
        <>
          <div className="ai-optin-header">Want smarter insights?</div>
          <p className="ai-optin-text">
            Lede can analyze your data locally to suggest charts and find patterns.
            One-time download (2.5 GB), stays on your computer.
          </p>
          <div className="ai-optin-actions">
            <button className="ai-optin-enable" onClick={handleEnable}>
              Enable
            </button>
            <button className="ai-optin-dismiss" onClick={handleDismiss}>
              Not now
            </button>
          </div>
          {error && <p className="ai-optin-error">{error}</p>}
        </>
      )}
    </div>
  );
}

export function isAiEnabled(): boolean {
  return localStorage.getItem(ENABLED_KEY) === "true";
}
