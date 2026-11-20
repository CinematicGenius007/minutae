//! Whisper model files: `ggml-{tiny,base,small,medium}.bin` in `paths::models_dir()`.
//!
//! Downloads stream into `<file>.part` and are renamed atomically once complete and
//! verified (exact byte size and SHA-256 from the Hugging Face LFS metadata), so a model
//! file that exists is whole. An interrupted download (crash, network drop, or the future
//! being dropped) leaves the `.part` file (plus `<file>.part.etag`) and the next
//! `download` resumes it with `Range` + `If-Range`; if the file changed upstream the
//! server sends it whole and the download starts over.

use std::path::{Path, PathBuf};

use minutae_core::{Progress, Stage, WhisperModel, paths};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
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
    /// Exact size of the published file.
    pub size_bytes: u64,
    /// SHA-256 (hex) of the published file.
    pub sha256: &'static str,
    /// `"142 MB"`
    pub size_label: &'static str,
}

/// Sizes and SHA-256s from the Hugging Face API (`/api/models/ggerganov/whisper.cpp/tree/main`,
/// `lfs.size` / `lfs.oid`) for `ggerganov/whisper.cpp@main`.
pub fn model_info(model: WhisperModel) -> ModelInfo {
    let (name, approx_bytes, size_label, size_bytes, sha256) = match model {
        WhisperModel::Tiny => (
            "tiny",
            75 * 1024 * 1024,
            "75 MB",
            77_691_713,
            "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21",
        ),
        WhisperModel::Base => (
            "base",
            142 * 1024 * 1024,
            "142 MB",
            147_951_465,
            "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe",
        ),
        WhisperModel::Small => (
            "small",
            466 * 1024 * 1024,
            "466 MB",
            487_601_967,
            "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
        ),
        WhisperModel::Medium => (
            "medium",
            1_536 * 1024 * 1024,
            "1.5 GB",
            1_533_763_059,
            "6c14d5adee5f86394037b4e4e8b59f1673b6cee10e3cf0b11bbdbee79c156208",
        ),
    };
    ModelInfo {
        model,
        name,
        file_name: format!("ggml-{name}.bin"),
        approx_bytes,
        size_bytes,
        sha256,
        size_label,
    }
}

/// What a downloaded file must match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Integrity {
    pub size: u64,
    /// Lower-case hex; `None` = size check only.
    pub sha256: Option<String>,
}

impl Integrity {
    pub fn of(model: WhisperModel) -> Self {
        let info = model_info(model);
        Self {
            size: info.size_bytes,
            sha256: Some(info.sha256.to_string()),
        }
    }
}

/// What's on disk for one model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    pub info: ModelInfo,
    pub path: PathBuf,
    /// The file exists and has exactly the expected size (the SHA-256 is checked when
    /// it is downloaded).
    pub downloaded: bool,
    /// Size of the file on disk (0 if absent; may be non-zero for a corrupt file).
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
    /// Per-model integrity overrides (tests / mirrors with other builds); `None` = unchecked.
    overrides: Vec<(WhisperModel, Option<Integrity>)>,
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
            overrides: Vec::new(),
        }
    }

    /// Replace the expected size/hash for `model` (`None` disables the checks).
    pub fn with_integrity(mut self, model: WhisperModel, integrity: Option<Integrity>) -> Self {
        self.overrides.retain(|(m, _)| *m != model);
        self.overrides.push((model, integrity));
        self
    }

    /// What `model`'s file must match, if checked.
    pub fn integrity(&self, model: WhisperModel) -> Option<Integrity> {
        match self.overrides.iter().find(|(m, _)| *m == model) {
            Some((_, i)) => i.clone(),
            None => Some(Integrity::of(model)),
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
        let downloaded = match self.integrity(model) {
            Some(i) => size_bytes == i.size,
            None => size_bytes > 0,
        };
        ModelStatus {
            info: model_info(model),
            downloaded,
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
    /// A present file with the wrong size is treated as corrupt and replaced.
    pub async fn download(
        &self,
        model: WhisperModel,
        progress: impl FnMut(DownloadProgress),
    ) -> Result<PathBuf> {
        let dest = self.path(model);
        let st = self.status(model);
        if st.downloaded {
            return Ok(dest);
        }
        if st.size_bytes > 0 {
            tracing::warn!(target: "minutae_asr", "replacing corrupt model {} ({} bytes)", dest.display(), st.size_bytes);
            tokio::fs::remove_file(&dest).await?;
        }
        let integrity = self.integrity(model);
        download_file(
            &self.client,
            &self.url(model),
            &dest,
            integrity.as_ref(),
            progress,
        )
        .await?;
        Ok(dest)
    }

    /// Removes the model and any partial download.
    pub fn delete(&self, model: WhisperModel) -> Result<()> {
        let path = self.path(model);
        for p in [etag_path(&path), part_path(&path), path] {
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

fn etag_path(dest: &Path) -> PathBuf {
    let mut s = part_path(dest).into_os_string();
    s.push(".etag");
    PathBuf::from(s)
}

async fn remove_partial(part: &Path, etag: &Path) {
    tokio::fs::remove_file(part).await.ok();
    tokio::fs::remove_file(etag).await.ok();
}

/// `Content-Range: bytes start-end/total` -> (start, total).
fn parse_content_range(v: &str) -> Option<(u64, Option<u64>)> {
    let rest = v.trim().strip_prefix("bytes ")?;
    let (range, total) = rest.split_once('/')?;
    let start = range.split_once('-')?.0.trim().parse().ok()?;
    Some((start, total.trim().parse().ok()))
}

async fn sha256_file(path: &Path) -> Result<String> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || -> Result<String> {
        use std::io::Read;
        let mut f = std::fs::File::open(&path)?;
        let mut h = Sha256::new();
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
        }
        Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
    })
    .await
    .map_err(|e| AsrError::Download(e.to_string()))?
}

/// Resumable streaming download into `dest` via `dest.part` + atomic rename. With
/// `integrity`, the result must have exactly that size and SHA-256; otherwise the partial
/// file is deleted and a clear error returned.
pub async fn download_file(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    integrity: Option<&Integrity>,
    mut progress: impl FnMut(DownloadProgress),
) -> Result<()> {
    use reqwest::StatusCode;
    use reqwest::header::{CONTENT_RANGE, ETAG, IF_RANGE, RANGE};

    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let part = part_path(dest);
    let etag_file = etag_path(dest);
    let dl_err = |e: reqwest::Error| AsrError::Download(e.to_string());
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    for _ in 0..3 {
        let mut have = tokio::fs::metadata(&part)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        if integrity.is_some_and(|i| have > i.size) {
            remove_partial(&part, &etag_file).await; // can't be a prefix of the real file
            have = 0;
        }
        let etag = if have > 0 {
            tokio::fs::read_to_string(&etag_file).await.ok()
        } else {
            None
        };
        let mut req = client.get(url);
        if have > 0 {
            req = req.header(RANGE, format!("bytes={have}-"));
            if let Some(tag) = etag.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
                // Only resume if the file is still the same; otherwise get a full 200.
                req = req.header(IF_RANGE, tag);
            }
        }
        let mut resp = req.send().await.map_err(dl_err)?;
        let status = resp.status();

        let (mut file, mut done, total) = if status == StatusCode::PARTIAL_CONTENT && have > 0 {
            let range = resp
                .headers()
                .get(CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(parse_content_range);
            let Some((start, total)) = range else {
                remove_partial(&part, &etag_file).await;
                continue; // can't tell where this body starts
            };
            if start != have {
                tracing::warn!(target: "minutae_asr", "server resumed at byte {start}, expected {have}; restarting");
                remove_partial(&part, &etag_file).await;
                continue;
            }
            let total = total.or_else(|| resp.content_length().map(|l| l + have));
            let f = tokio::fs::OpenOptions::new()
                .append(true)
                .open(&part)
                .await?;
            (f, have, total)
        } else if status == StatusCode::RANGE_NOT_SATISFIABLE {
            // Stale/oversized partial: start over.
            remove_partial(&part, &etag_file).await;
            continue;
        } else if status.is_success() {
            let f = tokio::fs::File::create(&part).await?;
            // Remember the validator so a resume only continues the same file.
            match resp.headers().get(ETAG).and_then(|v| v.to_str().ok()) {
                Some(tag) if !tag.starts_with("W/") => {
                    tokio::fs::write(&etag_file, tag).await?;
                }
                _ => {
                    tokio::fs::remove_file(&etag_file).await.ok();
                }
            }
            (f, 0, resp.content_length())
        } else {
            return Err(AsrError::Download(format!("HTTP {status} for {url}")));
        };

        if let (Some(i), Some(t)) = (integrity, total)
            && t != i.size
        {
            drop(file);
            remove_partial(&part, &etag_file).await;
            return Err(AsrError::Download(format!(
                "{name}: server reports {t} bytes but the model is {} bytes; not downloading",
                i.size
            )));
        }

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

        let want = integrity.map(|i| i.size).or(total);
        if let Some(t) = want {
            if done < t {
                return Err(AsrError::Download(format!(
                    "incomplete download: {done} of {t} bytes"
                )));
            }
            if done > t {
                remove_partial(&part, &etag_file).await;
                return Err(AsrError::Download(format!(
                    "{name} is {done} bytes, expected {t}; deleted the download, please try again"
                )));
            }
        }
        if let Some(expected) = integrity.and_then(|i| i.sha256.as_deref()) {
            let actual = sha256_file(&part).await?;
            if !actual.eq_ignore_ascii_case(expected) {
                remove_partial(&part, &etag_file).await;
                return Err(AsrError::Download(format!(
                    "{name} failed its integrity check (SHA-256 {actual}, expected {expected}); deleted the download, please try again"
                )));
            }
        }
        tokio::fs::rename(&part, dest).await?;
        tokio::fs::remove_file(&etag_file).await.ok();
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;

    #[derive(Clone, Default)]
    struct Opts {
        /// Truncate the first full (non-range) response after this many bytes.
        cut_first: Option<usize>,
        etag: Option<&'static str>,
        /// Answer range requests from this offset instead of the requested one.
        wrong_range_start: Option<usize>,
    }

    struct Server {
        url: String,
        hits: Arc<AtomicUsize>,
        requests: Arc<Mutex<Vec<String>>>,
    }

    /// Minimal HTTP/1.1 server serving `body` with Range / If-Range support.
    async fn serve(body: Arc<Vec<u8>>, opts: Opts) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let (hits2, reqs2) = (hits.clone(), requests.clone());
        tokio::spawn(async move {
            loop {
                let (mut sock, _) = listener.accept().await.unwrap();
                let n = hits2.fetch_add(1, Ordering::SeqCst);
                let body = body.clone();
                let opts = opts.clone();
                let reqs = reqs2.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let len = sock.read(&mut buf).await.unwrap();
                    let req = String::from_utf8_lossy(&buf[..len]).to_lowercase();
                    reqs.lock().unwrap().push(req.clone());
                    let etag_hdr = opts
                        .etag
                        .map(|t| format!("etag: {t}\r\n"))
                        .unwrap_or_default();
                    let if_range = req.lines().find_map(|l| l.strip_prefix("if-range: "));
                    let range_ok = match (if_range, opts.etag) {
                        (Some(sent), Some(cur)) => sent.trim() == cur.to_lowercase(),
                        (Some(_), None) => false,
                        (None, _) => true,
                    };
                    let start = req
                        .lines()
                        .find_map(|l| l.strip_prefix("range: bytes="))
                        .and_then(|r| r.split('-').next()?.trim().parse::<usize>().ok())
                        .filter(|_| range_ok);
                    let resp = match start {
                        Some(s) if s >= body.len() => {
                            "HTTP/1.1 416 Range Not Satisfiable\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".as_bytes().to_vec()
                        }
                        Some(s) => {
                            let s = opts.wrong_range_start.unwrap_or(s);
                            let mut h = format!(
                                "HTTP/1.1 206 Partial Content\r\n{etag_hdr}content-length: {}\r\ncontent-range: bytes {}-{}/{}\r\nconnection: close\r\n\r\n",
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
                            let mut h = format!("HTTP/1.1 200 OK\r\n{etag_hdr}content-length: {}\r\nconnection: close\r\n\r\n", body.len()).into_bytes();
                            let cut = if n == 0 { opts.cut_first.unwrap_or(body.len()) } else { body.len() };
                            h.extend_from_slice(&body[..cut]);
                            h
                        }
                    };
                    let _ = sock.write_all(&resp).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        Server {
            url: format!("http://{addr}"),
            hits,
            requests,
        }
    }

    fn body() -> Arc<Vec<u8>> {
        Arc::new((0..100_000u32).map(|i| (i % 251) as u8).collect())
    }

    fn sha_hex(b: &[u8]) -> String {
        Sha256::digest(b)
            .iter()
            .map(|x| format!("{x:02x}"))
            .collect()
    }

    fn checked(b: &[u8]) -> Option<Integrity> {
        Some(Integrity {
            size: b.len() as u64,
            sha256: Some(sha_hex(b)),
        })
    }

    fn manager(dir: &Path, url: &str, model: WhisperModel, body: &[u8]) -> ModelManager {
        ModelManager::with_dir(dir)
            .with_base_url(url)
            .with_integrity(model, checked(body))
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
        assert_eq!(model_info(WhisperModel::Base).size_bytes, 147_951_465);
        assert_eq!(model_info(WhisperModel::Tiny).sha256.len(), 64);
        assert_eq!(m.status_all().len(), 4);
        assert_eq!(
            m.integrity(WhisperModel::Small),
            Some(Integrity::of(WhisperModel::Small))
        );
        assert_eq!(
            parse_content_range("bytes 10-99/100"),
            Some((10, Some(100)))
        );
        assert_eq!(parse_content_range("bytes 10-99/*"), Some((10, None)));
        assert_eq!(parse_content_range("garbage"), None);
    }

    #[test]
    fn status_rejects_wrong_size_file() {
        let dir = tempfile::tempdir().unwrap();
        let m = ModelManager::with_dir(dir.path());
        // A truncated/corrupt model with the right name is not "downloaded".
        std::fs::write(m.path(WhisperModel::Tiny), b"truncated").unwrap();
        let st = m.status(WhisperModel::Tiny);
        assert!(!st.downloaded);
        assert_eq!(st.size_bytes, 9);
        // `WhisperTranscriber::from_manager` relies on `status().downloaded`.
        assert!(matches!(
            crate::WhisperTranscriber::from_manager(&m, WhisperModel::Tiny),
            Err(AsrError::ModelMissing(_))
        ));
        // With checks disabled any non-empty file counts.
        let m = m.with_integrity(WhisperModel::Tiny, None);
        assert!(m.status(WhisperModel::Tiny).downloaded);
    }

    #[tokio::test]
    async fn downloads_atomically_and_deletes() {
        let dir = tempfile::tempdir().unwrap();
        let body = body();
        let srv = serve(body.clone(), Opts::default()).await;
        let m = manager(dir.path(), &srv.url, WhisperModel::Tiny, &body);
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
    async fn resumes_after_truncated_download_with_if_range() {
        let dir = tempfile::tempdir().unwrap();
        let body = body();
        let srv = serve(
            body.clone(),
            Opts {
                cut_first: Some(30_000),
                etag: Some("\"v1\""),
                ..Default::default()
            },
        )
        .await;
        let m = manager(dir.path(), &srv.url, WhisperModel::Base, &body);
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
        assert_eq!(srv.hits.load(Ordering::SeqCst), 2);
        let reqs = srv.requests.lock().unwrap();
        assert!(reqs[1].contains("if-range: \"v1\""), "{}", reqs[1]);
        assert!(reqs[1].contains("range: bytes=30000-"));
        assert!(!etag_path(&m.path(WhisperModel::Base)).exists());
    }

    #[tokio::test]
    async fn changed_etag_restarts_from_scratch() {
        let dir = tempfile::tempdir().unwrap();
        let body = body();
        let srv = serve(
            body.clone(),
            Opts {
                etag: Some("\"v2\""),
                ..Default::default()
            },
        )
        .await;
        let m = manager(dir.path(), &srv.url, WhisperModel::Base, &body);
        let dest = m.path(WhisperModel::Base);
        // A partial from an older upload of the file.
        std::fs::write(part_path(&dest), vec![7u8; 40_000]).unwrap();
        std::fs::write(etag_path(&dest), "\"v1\"").unwrap();
        m.download(WhisperModel::Base, |_| {}).await.unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), *body);
    }

    #[tokio::test]
    async fn wrong_content_range_start_is_not_appended() {
        let dir = tempfile::tempdir().unwrap();
        let body = body();
        let srv = serve(
            body.clone(),
            Opts {
                wrong_range_start: Some(0),
                ..Default::default()
            },
        )
        .await;
        let m = manager(dir.path(), &srv.url, WhisperModel::Small, &body);
        let dest = m.path(WhisperModel::Small);
        std::fs::write(part_path(&dest), &body[..20_000]).unwrap();
        m.download(WhisperModel::Small, |_| {}).await.unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), *body);
    }

    #[tokio::test]
    async fn hash_mismatch_deletes_and_reports() {
        let dir = tempfile::tempdir().unwrap();
        let body = body();
        let srv = serve(body.clone(), Opts::default()).await;
        let mut wrong = checked(&body).unwrap();
        wrong.sha256 = Some("00".repeat(32));
        let m = ModelManager::with_dir(dir.path())
            .with_base_url(srv.url.clone())
            .with_integrity(WhisperModel::Tiny, Some(wrong));
        let err = m.download(WhisperModel::Tiny, |_| {}).await.unwrap_err();
        assert!(err.to_string().contains("integrity check"), "{err}");
        let st = m.status(WhisperModel::Tiny);
        assert!(!st.downloaded && st.partial_bytes == 0 && st.size_bytes == 0);
    }

    #[tokio::test]
    async fn size_mismatch_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let body = body();
        let srv = serve(body.clone(), Opts::default()).await;
        let m = ModelManager::with_dir(dir.path())
            .with_base_url(srv.url.clone())
            .with_integrity(
                WhisperModel::Tiny,
                Some(Integrity {
                    size: 123,
                    sha256: None,
                }),
            );
        let err = m.download(WhisperModel::Tiny, |_| {}).await.unwrap_err();
        assert!(err.to_string().contains("123 bytes"), "{err}");
        assert_eq!(m.status(WhisperModel::Tiny).partial_bytes, 0);
    }

    #[tokio::test]
    async fn corrupt_final_file_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let body = body();
        let srv = serve(body.clone(), Opts::default()).await;
        let m = manager(dir.path(), &srv.url, WhisperModel::Tiny, &body);
        std::fs::write(m.path(WhisperModel::Tiny), b"half a model").unwrap();
        m.download(WhisperModel::Tiny, |_| {}).await.unwrap();
        assert_eq!(std::fs::read(m.path(WhisperModel::Tiny)).unwrap(), *body);
    }

    #[tokio::test]
    async fn oversized_partial_restarts() {
        let dir = tempfile::tempdir().unwrap();
        let body = body();
        let srv = serve(body.clone(), Opts::default()).await;
        // Unchecked, so the 416 path (not the size pre-check) is exercised.
        let m = ModelManager::with_dir(dir.path())
            .with_base_url(srv.url.clone())
            .with_integrity(WhisperModel::Small, None);
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
