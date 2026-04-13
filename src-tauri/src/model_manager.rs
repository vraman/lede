use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};

const MODEL_FILENAME: &str = "gemma-3-4b-it-Q4_K_M.gguf";
const MODEL_URL: &str = "https://huggingface.co/bartowski/google_gemma-3-4b-it-GGUF/resolve/main/google_gemma-3-4b-it-Q4_K_M.gguf";
/// Minimum file size to consider download complete (~2GB)
const MIN_MODEL_SIZE: u64 = 2_000_000_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelStatus {
    pub downloaded: bool,
    pub downloading: bool,
    pub progress: f32,
}

pub struct ModelManager {
    model_dir: PathBuf,
    downloading: AtomicBool,
    progress: std::sync::Mutex<f32>,
}

impl ModelManager {
    pub fn new(app_data_dir: PathBuf) -> Self {
        let model_dir = app_data_dir.join("lede").join("models");
        Self {
            model_dir,
            downloading: AtomicBool::new(false),
            progress: std::sync::Mutex::new(0.0),
        }
    }

    pub fn model_path(&self) -> PathBuf {
        self.model_dir.join(MODEL_FILENAME)
    }

    pub fn is_downloaded(&self) -> bool {
        let path = self.model_path();
        if let Ok(meta) = std::fs::metadata(&path) {
            meta.len() > MIN_MODEL_SIZE
        } else {
            false
        }
    }

    pub fn status(&self) -> ModelStatus {
        ModelStatus {
            downloaded: self.is_downloaded(),
            downloading: self.downloading.load(Ordering::Relaxed),
            progress: *self.progress.lock().unwrap(),
        }
    }

    pub async fn download(&self, app: AppHandle) -> Result<(), String> {
        if self.is_downloaded() {
            return Ok(());
        }
        if self.downloading.swap(true, Ordering::SeqCst) {
            return Err("Download already in progress".to_string());
        }

        let result = self.do_download(&app).await;

        self.downloading.store(false, Ordering::SeqCst);
        match &result {
            Ok(()) => {
                let _ = app.emit("model-download-complete", ());
            }
            Err(e) => {
                let _ = app.emit("model-download-error", e.clone());
            }
        }
        result
    }

    async fn do_download(&self, app: &AppHandle) -> Result<(), String> {
        use futures_util::StreamExt;

        std::fs::create_dir_all(&self.model_dir)
            .map_err(|e| format!("Failed to create model directory: {}", e))?;

        let partial_path = self.model_dir.join(format!("{}.partial", MODEL_FILENAME));
        let final_path = self.model_path();

        let client = reqwest::Client::new();
        let response = client
            .get(MODEL_URL)
            .send()
            .await
            .map_err(|e| format!("Download request failed: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("Download failed with status: {}", response.status()));
        }

        let total_size = response.content_length().unwrap_or(0);
        let mut downloaded: u64 = 0;

        let mut file = tokio::fs::File::create(&partial_path)
            .await
            .map_err(|e| format!("Failed to create file: {}", e))?;

        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("Download stream error: {}", e))?;
            tokio::io::AsyncWriteExt::write_all(&mut file, &chunk)
                .await
                .map_err(|e| format!("Failed to write chunk: {}", e))?;

            downloaded += chunk.len() as u64;
            if total_size > 0 {
                let progress = downloaded as f32 / total_size as f32;
                *self.progress.lock().unwrap() = progress;
                let _ = app.emit("model-download-progress", progress);
            }
        }

        tokio::io::AsyncWriteExt::flush(&mut file)
            .await
            .map_err(|e| format!("Failed to flush file: {}", e))?;
        drop(file);

        tokio::fs::rename(&partial_path, &final_path)
            .await
            .map_err(|e| format!("Failed to rename partial file: {}", e))?;

        *self.progress.lock().unwrap() = 1.0;
        Ok(())
    }
}

#[tauri::command]
pub async fn check_model_status(app: AppHandle) -> Result<ModelStatus, String> {
    let mgr = app.state::<Arc<ModelManager>>();
    Ok(mgr.status())
}

#[tauri::command]
pub async fn start_model_download(app: AppHandle) -> Result<(), String> {
    let mgr = app.state::<Arc<ModelManager>>();
    let mgr = Arc::clone(&mgr);
    let app_clone = app.clone();
    tokio::spawn(async move {
        if let Err(e) = mgr.download(app_clone).await {
            eprintln!("[model_manager] download failed: {}", e);
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn make_manager() -> (ModelManager, TempDir) {
        let tmp = TempDir::new().unwrap();
        let mgr = ModelManager::new(tmp.path().to_path_buf());
        (mgr, tmp)
    }

    #[test]
    fn test_is_downloaded_false_when_absent() {
        let (mgr, _tmp) = make_manager();
        assert!(!mgr.is_downloaded());
    }

    #[test]
    fn test_is_downloaded_false_when_too_small() {
        let (mgr, _tmp) = make_manager();
        fs::create_dir_all(mgr.model_path().parent().unwrap()).unwrap();
        fs::write(mgr.model_path(), "small file").unwrap();
        assert!(!mgr.is_downloaded());
    }

    #[test]
    fn test_model_path_correct() {
        let (mgr, _tmp) = make_manager();
        let path = mgr.model_path();
        assert!(path.ends_with("lede/models/gemma-3-4b-it-Q4_K_M.gguf"));
    }

    #[test]
    fn test_status_not_downloading_initially() {
        let (mgr, _tmp) = make_manager();
        let status = mgr.status();
        assert!(!status.downloaded);
        assert!(!status.downloading);
        assert_eq!(status.progress, 0.0);
    }
}
