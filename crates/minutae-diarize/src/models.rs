//! Diarization model files in `<models_dir>/diarize/`:
//! - pyannote segmentation 3.0 (ONNX, from the k2-fsa `speaker-segmentation-models` release, a .tar.bz2)
//! - 3D-Speaker CAM++ zh/en speaker embedding (ONNX, from the `speaker-recongition-models` release — sic)
//!
//! Same rules as the whisper models: stream into `.part`, resume with `Range`, rename atomically.

use std::path::{Path, PathBuf};

use minutae_core::{Progress, Stage, paths};
use serde::Serialize;
use tokio::io::AsyncWriteExt;

use crate::{DiarizeError, Result};

pub const SEGMENTATION_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-segmentation-models/sherpa-onnx-pyannote-segmentation-3-0.tar.bz2";
/// Member of the archive we extract.
const SEGMENTATION_MEMBER: &str = "sherpa-onnx-pyannote-segmentation-3-0/model.onnx";
const SEGMENTATION_ARCHIVE_BYTES: u64 = 6_958_444;
pub const SEGMENTATION_FILE: &str = "pyannote-segmentation-3-0.onnx";

pub const EMBEDDING_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_campplus_sv_zh_en_16k-common_advanced.onnx";
const EMBEDDING_BYTES: u64 = 28_281_165;
pub const EMBEDDING_FILE: &str = "3dspeaker_speech_campplus_sv_zh_en_16k-common_advanced.onnx";

/// Download progress across both files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
}

impl DownloadProgress {
    pub fn fraction(&self) -> f32 {
        if self.total_bytes == 0 {
            0.0
        } else {
            (self.downloaded_bytes as f64 / self.total_bytes as f64).min(1.0) as f32
        }
    }

    pub fn to_progress(&self) -> Progress {
        Progress {
            meeting_id: None,
            stage: Stage::DownloadingModel,
            fraction: self.fraction(),
            eta_seconds: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiarizeModelStatus {
    pub segmentation: bool,
    pub embedding: bool,
    /// Both present.
    pub ready: bool,
    pub size_bytes: u64,
    /// Total download size (~35 MB).
    pub download_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct DiarizeModels {
    dir: PathBuf,
    segmentation_url: String,
    embedding_url: String,
    client: reqwest::Client,
}

impl Default for DiarizeModels {
    fn default() -> Self {
        Self::new()
    }
}

impl DiarizeModels {
    /// `<paths::models_dir()>/diarize`
    pub fn new() -> Self {
        Self::with_models_dir(paths::models_dir())
    }

    /// `<models_dir>/diarize` — pass the same root you give the whisper `ModelManager`.
    pub fn with_models_dir(models_dir: impl AsRef<Path>) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(concat!("Minutae/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(std::time::Duration::from_secs(20))
            .build()
            .expect("reqwest client");
        Self {
            dir: models_dir.as_ref().join("diarize"),
            segmentation_url: SEGMENTATION_URL.into(),
            embedding_url: EMBEDDING_URL.into(),
            client,
        }
    }

    /// Override download URLs (tests / mirrors).
    pub fn with_urls(
        mut self,
        segmentation_tar_bz2: impl Into<String>,
        embedding_onnx: impl Into<String>,
    ) -> Self {
        self.segmentation_url = segmentation_tar_bz2.into();
        self.embedding_url = embedding_onnx.into();
        self
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn segmentation_path(&self) -> PathBuf {
        self.dir.join(SEGMENTATION_FILE)
    }

    pub fn embedding_path(&self) -> PathBuf {
        self.dir.join(EMBEDDING_FILE)
    }

    pub fn status(&self) -> DiarizeModelStatus {
        let size = |p: PathBuf| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        let (s, e) = (size(self.segmentation_path()), size(self.embedding_path()));
        DiarizeModelStatus {
            segmentation: s > 0,
            embedding: e > 0,
            ready: s > 0 && e > 0,
            size_bytes: s + e,
            download_bytes: SEGMENTATION_ARCHIVE_BYTES + EMBEDDING_BYTES,
        }
    }

    pub fn is_ready(&self) -> bool {
        self.status().ready
    }

    /// Download whatever is missing. Dropping the future keeps `.part` files for resume.
    pub async fn download(&self, mut progress: impl FnMut(DownloadProgress)) -> Result<()> {
        tokio::fs::create_dir_all(&self.dir).await?;
        let total = SEGMENTATION_ARCHIVE_BYTES + EMBEDDING_BYTES;
        let st = self.status();
        let mut base = 0;
        if !st.segmentation {
            let archive = self.dir.join("pyannote-segmentation-3-0.tar.bz2");
            download_file(&self.client, &self.segmentation_url, &archive, |d, _| {
                progress(DownloadProgress {
                    downloaded_bytes: d,
                    total_bytes: total,
                })
            })
            .await?;
            let dest = self.segmentation_path();
            let archive2 = archive.clone();
            tokio::task::spawn_blocking(move || {
                extract_member(&archive2, SEGMENTATION_MEMBER, &dest)
            })
            .await
            .map_err(|e| DiarizeError::Engine(e.to_string()))??;
            tokio::fs::remove_file(&archive).await.ok();
        }
        base += SEGMENTATION_ARCHIVE_BYTES;
        if !st.embedding {
            download_file(
                &self.client,
                &self.embedding_url,
                &self.embedding_path(),
                |d, _| {
                    progress(DownloadProgress {
                        downloaded_bytes: base + d,
                        total_bytes: total,
                    })
                },
            )
            .await?;
        }
        progress(DownloadProgress {
            downloaded_bytes: total,
            total_bytes: total,
        });
        Ok(())
    }

    /// Remove both models (and partial downloads).
    pub fn delete(&self) -> Result<()> {
        match std::fs::remove_dir_all(&self.dir) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

fn part_path(dest: &Path) -> PathBuf {
    let mut s = dest.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

/// Extract one member of a .tar.bz2 to `dest` (via temp file + rename).
fn extract_member(archive: &Path, member: &str, dest: &Path) -> Result<()> {
    let file = std::fs::File::open(archive)?;
    let mut tar = tar::Archive::new(bzip2::read::BzDecoder::new(std::io::BufReader::new(file)));
    for entry in tar.entries()? {
        let mut entry = entry?;
        if entry.path()?.to_string_lossy().trim_start_matches("./") == member {
            let tmp = part_path(dest);
            let mut out = std::fs::File::create(&tmp)?;
            std::io::copy(&mut entry, &mut out)?;
            out.sync_all()?;
            drop(out);
            std::fs::rename(&tmp, dest)?;
            return Ok(());
        }
    }
    Err(DiarizeError::Download(format!(
        "{member} not found in {}",
        archive.display()
    )))
}

/// Resumable streaming download; `progress(downloaded, total)`.
async fn download_file(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<()> {
    use reqwest::StatusCode;
    use reqwest::header::{CONTENT_RANGE, RANGE};

    if dest.exists() {
        return Ok(());
    }
    let part = part_path(dest);
    let dl_err = |e: reqwest::Error| DiarizeError::Download(e.to_string());
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
            let total = resp
                .headers()
                .get(CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.rsplit('/').next())
                .and_then(|t| t.parse::<u64>().ok())
                .or_else(|| resp.content_length().map(|l| l + have));
            (
                tokio::fs::OpenOptions::new()
                    .append(true)
                    .open(&part)
                    .await?,
                have,
                total,
            )
        } else if status == StatusCode::RANGE_NOT_SATISFIABLE && attempt == 0 {
            tokio::fs::remove_file(&part).await.ok();
            continue;
        } else if status.is_success() {
            (
                tokio::fs::File::create(&part).await?,
                0,
                resp.content_length(),
            )
        } else {
            return Err(DiarizeError::Download(format!("HTTP {status} for {url}")));
        };
        progress(done, total);
        let streamed: Result<()> = async {
            while let Some(chunk) = resp.chunk().await.map_err(dl_err)? {
                file.write_all(&chunk).await?;
                done += chunk.len() as u64;
                progress(done, total);
            }
            Ok(())
        }
        .await;
        file.flush().await?;
        file.sync_all().await?;
        drop(file);
        streamed?;
        if let Some(t) = total
            && t != done
        {
            return Err(DiarizeError::Download(format!(
                "incomplete download: {done} of {t} bytes"
            )));
        }
        tokio::fs::rename(&part, dest).await?;
        return Ok(());
    }
    Err(DiarizeError::Download(format!(
        "server refused range request for {url}"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_and_status() {
        let dir = tempfile::tempdir().unwrap();
        let m = DiarizeModels::with_models_dir(dir.path());
        assert_eq!(m.dir(), dir.path().join("diarize"));
        assert!(!m.is_ready());
        std::fs::create_dir_all(m.dir()).unwrap();
        std::fs::write(m.segmentation_path(), b"x").unwrap();
        let st = m.status();
        assert!(st.segmentation && !st.embedding && !st.ready);
        std::fs::write(m.embedding_path(), b"y").unwrap();
        assert!(m.is_ready());
        m.delete().unwrap();
        assert!(!m.status().segmentation);
        m.delete().unwrap();
    }

    #[test]
    fn extracts_member_from_tar_bz2() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("a.tar.bz2");
        {
            let f = std::fs::File::create(&archive).unwrap();
            let enc = bzip2::write::BzEncoder::new(f, bzip2::Compression::fast());
            let mut b = tar::Builder::new(enc);
            for (name, data) in [
                (
                    "sherpa-onnx-pyannote-segmentation-3-0/README.md",
                    &b"readme"[..],
                ),
                (SEGMENTATION_MEMBER, &b"onnx-bytes"[..]),
            ] {
                let mut h = tar::Header::new_gnu();
                h.set_size(data.len() as u64);
                h.set_mode(0o644);
                h.set_cksum();
                b.append_data(&mut h, name, data).unwrap();
            }
            b.into_inner().unwrap().finish().unwrap();
        }
        let dest = dir.path().join("seg.onnx");
        extract_member(&archive, SEGMENTATION_MEMBER, &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"onnx-bytes");
        assert!(extract_member(&archive, "nope", &dir.path().join("n")).is_err());
    }
}
