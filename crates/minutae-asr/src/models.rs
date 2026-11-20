//! Whisper model files: `ggml-{tiny,base,small,medium}.bin` in `paths::models_dir()`.
//!
//! Downloads stream into `<file>.part` and are renamed atomically when complete, so a
//! model file that exists is always whole. An interrupted download (crash, network
//! drop, or the future being dropped) leaves the `.part` file and the next
//! `download` resumes it with an HTTP `Range` request.

use std::path::{Path, PathBuf};

use minutae_core::{Progress, Stage, WhisperModel, paths};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;

use crate::{AsrError, Result};

pub const DEFAULT_BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

/// Static metadata about a model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub model: WhisperModel,
    /// `"base"`
    pub name: &'static str,
    /// `"ggml-base.bin"`
    pub file_name: String,
    /// Approximate download size in bytes (for the UI before download starts).
    pub approx_bytes: u64,
    /// `"142 MB"`
    pub size_label: &'static str,
}

pub fn model_info(model: WhisperModel) -> ModelInfo {
    let (name, approx_bytes, size_label) = match model {
        WhisperModel::Tiny => ("tiny", 75 * 1024 * 1024, "75 MB"),
        WhisperModel::Base => ("base", 142 * 1024 * 1024, "142 MB"),
        WhisperModel::Small => ("small", 466 * 1024 * 1024, "466 MB"),
        WhisperModel::Medium => ("medium", 1_536 * 1024 * 1024, "1.5 GB"),
    };
    ModelInfo {
        model,
        name,
        file_name: format!("ggml-{name}.bin"),
        approx_bytes,
        size_label,
    }
}

/// What's on disk for one model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    pub info: ModelInfo,
    pub path: PathBuf,
    pub downloaded: bool,
    /// Size of the finished file (0 if not downloaded).
    pub size_bytes: u64,
    /// Bytes of an interrupted download that `download` will resume from.
    pub partial_bytes: u64,
}

/// Download progress tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
}

impl DownloadProgress {
    pub fn fraction(&self) -> Option<f32> {
        self.total_bytes
            .filter(|t| *t > 0)
            .map(|t| (self.downloaded_bytes as f64 / t as f64).min(1.0) as f32)
    }

    /// Convert to the shared UI progress type.
    pub fn to_progress(&self) -> Progress {
        Progress {
            meeting_id: None,
            stage: Stage::DownloadingModel,
            fraction: self.fraction().unwrap_or(0.0),
            eta_seconds: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModelManager {
    dir: PathBuf,
    base_url: String,
    client: reqwest::Client,
}

impl Default for ModelManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ModelManager {
    /// Uses `minutae_core::paths::models_dir()`.
    pub fn new() -> Self {
        Self::with_dir(paths::models_dir())
    }

    /// Custom directory (tests, portable installs).
    pub fn with_dir(dir: impl Into<PathBuf>) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(concat!("Minutae/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(std::time::Duration::from_secs(20))
            .build()
            .expect("reqwest client");
        Self {
            dir: dir.into(),
            base_url: DEFAULT_BASE_URL.to_string(),
            client,
        }
    }

    /// Override the download host (tests / mirrors). No trailing slash.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into().trim_end_matches('/').to_string();
        self
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn path(&self, model: WhisperModel) -> PathBuf {
        self.dir.join(model_info(model).file_name)
    }

    pub fn url(&self, model: WhisperModel) -> String {
        format!("{}/{}", self.base_url, model_info(model).file_name)
    }

    pub fn status(&self, model: WhisperModel) -> ModelStatus {
        let path = self.path(model);
        let size_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let partial_bytes = std::fs::metadata(part_path(&path))
            .map(|m| m.len())
            .unwrap_or(0);
        ModelStatus {
            info: model_info(model),
            downloaded: size_bytes > 0,
            path,
            size_bytes,
            partial_bytes,
        }
    }

    pub fn status_all(&self) -> Vec<ModelStatus> {
        [
            WhisperModel::Tiny,
            WhisperModel::Base,
            WhisperModel::Small,
            WhisperModel::Medium,
        ]
        .into_iter()
        .map(|m| self.status(m))
        .collect()
    }

    /// Download (or resume) a model. Returns the final path. No-op if already present.
    pub async fn download(
        &self,
        model: WhisperModel,
        progress: impl FnMut(DownloadProgress),
    ) -> Result<PathBuf> {
        let dest = self.path(model);
        if self.status(model).downloaded {
            return Ok(dest);
        }
        download_file(&self.client, &self.url(model), &dest, progress).await?;
        Ok(dest)
    }

    /// Removes the model and any partial download.
    pub fn delete(&self, model: WhisperModel) -> Result<()> {
        let path = self.path(model);
        for p in [part_path(&path), path] {
            match std::fs::remove_file(&p) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }
}

pub(crate) fn part_path(dest: &Path) -> PathBuf {
    let mut s = dest.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

/// Resumable streaming download into `dest` via `dest.part` + atomic rename.
pub async fn download_file(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    mut progress: impl FnMut(DownloadProgress),
) -> Result<()> {
    use reqwest::StatusCode;
    use reqwest::header::{CONTENT_RANGE, RANGE};

    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let part = part_path(dest);
    let dl_err = |e: reqwest::Error| AsrError::Download(e.to_string());

    for attempt in 0..2 {
        let have = tokio::fs::metadata(&part)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        let mut req = client.get(url);
        if have > 0 {
            req = req.header(RANGE, format!("bytes={have}-"));
        }
        let mut resp = req.send().await.map_err(dl_err)?;
        let status = resp.status();

        let (mut file, mut done, total) = if status == StatusCode::PARTIAL_CONTENT && have > 0 {
            // Content-Range: bytes start-end/total
            let total = resp
                .headers()
                .get(CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.rsplit('/').next())
                .and_then(|t| t.parse::<u64>().ok())
                .or_else(|| resp.content_length().map(|l| l + have));
            let f = tokio::fs::OpenOptions::new()
                .append(true)
                .open(&part)
                .await?;
            (f, have, total)
        } else if status == StatusCode::RANGE_NOT_SATISFIABLE && attempt == 0 {
            // Stale/oversized partial: start over.
            tokio::fs::remove_file(&part).await.ok();
            continue;
        } else if status.is_success() {
            let f = tokio::fs::File::create(&part).await?;
            (f, 0, resp.content_length())
        } else {
            return Err(AsrError::Download(format!("HTTP {status} for {url}")));
        };

        progress(DownloadProgress {
            downloaded_bytes: done,
            total_bytes: total,
        });
        let streamed: Result<()> = async {
            while let Some(chunk) = resp.chunk().await.map_err(dl_err)? {
                file.write_all(&chunk).await?;
                done += chunk.len() as u64;
                progress(DownloadProgress {
                    downloaded_bytes: done,
                    total_bytes: total,
                });
            }
            Ok(())
        }
        .await;
        // Persist whatever arrived so a failed download can be resumed.
        file.flush().await?;
        file.sync_all().await?;
        drop(file);
        streamed?;

        if let Some(t) = total
            && t != done
        {
            return Err(AsrError::Download(format!(
                "incomplete download: {done} of {t} bytes"
            )));
        }
        tokio::fs::rename(&part, dest).await?;
        tracing::info!(target: "minutae_asr", "downloaded {} ({done} bytes)", dest.display());
        return Ok(());
    }
    Err(AsrError::Download(format!(
        "server refused range request for {url}"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;

    /// Minimal HTTP/1.1 server serving `body` with Range support.
    /// If `cut_first` is set, the first full (non-range) response is truncated after that many bytes.
    async fn serve(body: Arc<Vec<u8>>, cut_first: Option<usize>) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        let hits2 = hits.clone();
        tokio::spawn(async move {
            loop {
                let (mut sock, _) = listener.accept().await.unwrap();
                let n = hits2.fetch_add(1, Ordering::SeqCst);
                let body = body.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let len = sock.read(&mut buf).await.unwrap();
                    let req = String::from_utf8_lossy(&buf[..len]).to_lowercase();
                    let start = req
                        .lines()
                        .find_map(|l| l.strip_prefix("range: bytes="))
                        .and_then(|r| r.split('-').next()?.trim().parse::<usize>().ok());
                    let resp = match start {
                        Some(s) if s >= body.len() => {
                            "HTTP/1.1 416 Range Not Satisfiable\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".as_bytes().to_vec()
                        }
                        Some(s) => {
                            let mut h = format!(
                                "HTTP/1.1 206 Partial Content\r\ncontent-length: {}\r\ncontent-range: bytes {}-{}/{}\r\nconnection: close\r\n\r\n",
                                body.len() - s,
                                s,
                                body.len() - 1,
                                body.len()
                            )
                            .into_bytes();
                            h.extend_from_slice(&body[s..]);
                            h
                        }
                        None => {
                            let mut h = format!("HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", body.len()).into_bytes();
                            let cut = if n == 0 { cut_first.unwrap_or(body.len()) } else { body.len() };
                            h.extend_from_slice(&body[..cut]);
                            h
                        }
                    };
                    let _ = sock.write_all(&resp).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        (format!("http://{addr}"), hits)
    }

    fn body() -> Arc<Vec<u8>> {
        Arc::new((0..100_000u32).map(|i| (i % 251) as u8).collect())
    }

    #[test]
    fn info_and_paths() {
        let m = ModelManager::with_dir("/x");
        assert_eq!(
            m.path(WhisperModel::Base),
            PathBuf::from("/x/ggml-base.bin")
        );
        assert_eq!(
            m.url(WhisperModel::Tiny),
            format!("{DEFAULT_BASE_URL}/ggml-tiny.bin")
        );
        assert_eq!(model_info(WhisperModel::Medium).size_label, "1.5 GB");
        assert_eq!(m.status_all().len(), 4);
    }

    #[tokio::test]
    async fn downloads_atomically_and_deletes() {
        let dir = tempfile::tempdir().unwrap();
        let body = body();
        let (url, _) = serve(body.clone(), None).await;
        let m = ModelManager::with_dir(dir.path()).with_base_url(url);
        assert!(!m.status(WhisperModel::Tiny).downloaded);
        let mut last = None;
        let path = m
            .download(WhisperModel::Tiny, |p| last = Some(p))
            .await
            .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), *body);
        assert_eq!(last.unwrap().fraction(), Some(1.0));
        let st = m.status(WhisperModel::Tiny);
        assert!(st.downloaded && st.partial_bytes == 0 && st.size_bytes == body.len() as u64);
        m.delete(WhisperModel::Tiny).unwrap();
        assert!(!m.status(WhisperModel::Tiny).downloaded);
        m.delete(WhisperModel::Tiny).unwrap(); // idempotent
    }

    #[tokio::test]
    async fn resumes_after_truncated_download() {
        let dir = tempfile::tempdir().unwrap();
        let body = body();
        let (url, hits) = serve(body.clone(), Some(30_000)).await;
        let m = ModelManager::with_dir(dir.path()).with_base_url(url);
        let err = m.download(WhisperModel::Base, |_| {}).await.unwrap_err();
        assert!(matches!(err, AsrError::Download(_)), "{err:?}");
        let st = m.status(WhisperModel::Base);
        assert!(!st.downloaded);
        assert_eq!(st.partial_bytes, 30_000);
        let mut first = None;
        m.download(WhisperModel::Base, |p| {
            first.get_or_insert(p);
        })
        .await
        .unwrap();
        assert_eq!(
            first.unwrap().downloaded_bytes,
            30_000,
            "resumed from the partial file"
        );
        assert_eq!(std::fs::read(m.path(WhisperModel::Base)).unwrap(), *body);
        assert_eq!(hits.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn oversized_partial_restarts() {
        let dir = tempfile::tempdir().unwrap();
        let body = body();
        let (url, _) = serve(body.clone(), None).await;
        let m = ModelManager::with_dir(dir.path()).with_base_url(url);
        std::fs::write(part_path(&m.path(WhisperModel::Small)), vec![0u8; 200_000]).unwrap();
        m.download(WhisperModel::Small, |_| {}).await.unwrap();
        assert_eq!(std::fs::read(m.path(WhisperModel::Small)).unwrap(), *body);
    }

    #[tokio::test]
    async fn http_error_is_reported() {
        let m = ModelManager::with_dir(tempfile::tempdir().unwrap().path())
            .with_base_url("http://127.0.0.1:9");
        assert!(matches!(
            m.download(WhisperModel::Tiny, |_| {}).await,
            Err(AsrError::Download(_))
        ));
    }
}
