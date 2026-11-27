//! Diarization model files in `<models_dir>/diarize/`:
//! - pyannote segmentation 3.0 (ONNX, from the k2-fsa `speaker-segmentation-models` release, a .tar.bz2)
//! - 3D-Speaker CAM++ zh/en speaker embedding (ONNX, from the `speaker-recongition-models` release — sic)
//!
//! Same rules as the whisper models: stream into `.part`, resume with `Range`, rename atomically.
//! Every file is checked against its published byte size (download, archive before
//! extraction, extracted model, and `status`), and each fresh download against its pinned
//! SHA-256 (the files are parsed by native ONNX code, so a swapped release asset must not
//! load). A file that fails is deleted so the next attempt starts clean instead of reusing
//! a corrupt archive forever.

use std::path::{Path, PathBuf};

use minutae_core::{Progress, Stage, paths};
use serde::Serialize;
use tokio::io::AsyncWriteExt;

use crate::{DiarizeError, Result};

pub const SEGMENTATION_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-segmentation-models/sherpa-onnx-pyannote-segmentation-3-0.tar.bz2";
/// Member of the archive we extract.
const SEGMENTATION_MEMBER: &str = "sherpa-onnx-pyannote-segmentation-3-0/model.onnx";
const SEGMENTATION_ARCHIVE_BYTES: u64 = 6_958_444;
const SEGMENTATION_ARCHIVE_SHA256: &str =
    "24615ee884c897d9d2ba09bb4d30da6bb1b15e685065962db5b02e76e4996488";
/// Size of `model.onnx` inside the archive.
const SEGMENTATION_MODEL_BYTES: u64 = 5_992_913;
const SEGMENTATION_MODEL_SHA256: &str =
    "220ad67ca923bef2fa91f2390c786097bf305bceb5e261d4af67b38e938e1079";
const SEGMENTATION_ARCHIVE: &str = "pyannote-segmentation-3-0.tar.bz2";
pub const SEGMENTATION_FILE: &str = "pyannote-segmentation-3-0.onnx";

pub const EMBEDDING_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_campplus_sv_zh_en_16k-common_advanced.onnx";
const EMBEDDING_BYTES: u64 = 28_281_165;
const EMBEDDING_SHA256: &str = "53cab20ba8f2eb2f6e2d89ecc18e88ef3222169b61b4419cbc13666744a4574b";
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
    /// Check files against the published sizes (off only for tests with fake files).
    check_sizes: bool,
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
            check_sizes: true,
        }
    }

    /// Disable the byte-size checks (tests / mirrors serving other builds).
    pub fn with_size_checks(mut self, check: bool) -> Self {
        self.check_sizes = check;
        self
    }

    fn expect(&self, bytes: u64) -> Option<u64> {
        self.check_sizes.then_some(bytes)
    }

    /// File present with the expected size (any non-empty size when unchecked).
    fn valid(&self, path: &Path, bytes: u64) -> bool {
        let len = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        match self.expect(bytes) {
            Some(want) => len == want,
            None => len > 0,
        }
    }

    /// `path` hashes to `sha256` (always true when checks are off). Deletes the file when it
    /// doesn't, so the next attempt downloads it again.
    async fn verify_hash(&self, path: &Path, sha256: &'static str, what: &str) -> Result<()> {
        if !self.check_sizes {
            return Ok(());
        }
        let p = path.to_path_buf();
        let got = tokio::task::spawn_blocking(move || sha256_file(&p))
            .await
            .map_err(|e| DiarizeError::Engine(e.to_string()))??;
        if got == sha256 {
            return Ok(());
        }
        remove_if_exists(path).await?;
        Err(DiarizeError::Download(format!(
            "the {what} doesn't match its published checksum; deleted it, please try again"
        )))
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
        let seg_ok = self.valid(&self.segmentation_path(), SEGMENTATION_MODEL_BYTES);
        let emb_ok = self.valid(&self.embedding_path(), EMBEDDING_BYTES);
        DiarizeModelStatus {
            segmentation: seg_ok,
            embedding: emb_ok,
            ready: seg_ok && emb_ok,
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
            remove_if_exists(&self.segmentation_path()).await?; // wrong size = corrupt
            let archive = self.dir.join(SEGMENTATION_ARCHIVE);
            if archive.exists() && !self.valid(&archive, SEGMENTATION_ARCHIVE_BYTES) {
                remove_if_exists(&archive).await?;
            }
            download_file(
                &self.client,
                &self.segmentation_url,
                &archive,
                self.expect(SEGMENTATION_ARCHIVE_BYTES),
                |d, _| {
                    progress(DownloadProgress {
                        downloaded_bytes: d,
                        total_bytes: total,
                    })
                },
            )
            .await?;
            self.verify_hash(
                &archive,
                SEGMENTATION_ARCHIVE_SHA256,
                "speaker segmentation download",
            )
            .await?;
            let dest = self.segmentation_path();
            let (archive2, dest2) = (archive.clone(), dest.clone());
            let extracted = tokio::task::spawn_blocking(move || {
                extract_member(&archive2, SEGMENTATION_MEMBER, &dest2)
            })
            .await
            .map_err(|e| DiarizeError::Engine(e.to_string()))
            .and_then(|r| r);
            // Never keep an archive that didn't yield a valid model: re-download next time.
            tokio::fs::remove_file(&archive).await.ok();
            if let Err(e) = extracted {
                return Err(DiarizeError::Download(format!(
                    "the speaker segmentation archive is damaged ({e}); deleted it, please try again"
                )));
            }
            if !self.valid(&dest, SEGMENTATION_MODEL_BYTES) {
                remove_if_exists(&dest).await?;
                return Err(DiarizeError::Download(
                    "the extracted speaker segmentation model has the wrong size; deleted it, please try again".into(),
                ));
            }
            self.verify_hash(
                &dest,
                SEGMENTATION_MODEL_SHA256,
                "speaker segmentation model",
            )
            .await?;
        }
        base += SEGMENTATION_ARCHIVE_BYTES;
        if !st.embedding {
            remove_if_exists(&self.embedding_path()).await?;
            download_file(
                &self.client,
                &self.embedding_url,
                &self.embedding_path(),
                self.expect(EMBEDDING_BYTES),
                |d, _| {
                    progress(DownloadProgress {
                        downloaded_bytes: base + d,
                        total_bytes: total,
                    })
                },
            )
            .await?;
            self.verify_hash(
                &self.embedding_path(),
                EMBEDDING_SHA256,
                "speaker embedding model",
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

/// Lower-case hex SHA-256 of a file, streamed.
fn sha256_file(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    std::io::copy(&mut f, &mut h)?;
    Ok(format!("{:x}", h.finalize()))
}

async fn remove_if_exists(path: &Path) -> Result<()> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Names reserved for speaker models and their in-progress downloads.
pub fn download_files() -> Vec<String> {
    [SEGMENTATION_FILE, EMBEDDING_FILE, SEGMENTATION_ARCHIVE]
        .into_iter()
        .flat_map(|name| [name.to_owned(), format!("{name}.part")])
        .collect()
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

/// Resumable streaming download; `progress(downloaded, total)`. With `expected`, the file
/// must have exactly that many bytes; otherwise the partial download is deleted.
async fn download_file(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    expected: Option<u64>,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<()> {
    use reqwest::StatusCode;
    use reqwest::header::{CONTENT_RANGE, RANGE};

    if dest.exists() {
        return Ok(());
    }
    let part = part_path(dest);
    let dl_err = |e: reqwest::Error| DiarizeError::Download(e.to_string());
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    for attempt in 0..3 {
        let mut have = tokio::fs::metadata(&part)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        if expected.is_some_and(|e| have > e) {
            tokio::fs::remove_file(&part).await.ok();
            have = 0;
        }
        let mut req = client.get(url);
        if have > 0 {
            req = req.header(RANGE, format!("bytes={have}-"));
        }
        let mut resp = req.send().await.map_err(dl_err)?;
        let status = resp.status();
        let (mut file, mut done, total) = if status == StatusCode::PARTIAL_CONTENT && have > 0 {
            let range = resp
                .headers()
                .get(CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().strip_prefix("bytes "))
                .and_then(|v| v.split_once('/'))
                .map(|(r, t)| (r.split('-').next().and_then(|s| s.parse::<u64>().ok()), t));
            if range.and_then(|(start, _)| start) != Some(have) {
                // The body doesn't continue our partial file: start over.
                tokio::fs::remove_file(&part).await.ok();
                continue;
            }
            let total = range
                .and_then(|(_, t)| t.parse::<u64>().ok())
                .or_else(|| resp.content_length().map(|l| l + have));
            (
                tokio::fs::OpenOptions::new()
                    .append(true)
                    .open(&part)
                    .await?,
                have,
                total,
            )
        } else if status == StatusCode::RANGE_NOT_SATISFIABLE && attempt < 2 {
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
        if let (Some(e), Some(t)) = (expected, total)
            && e != t
        {
            drop(file);
            tokio::fs::remove_file(&part).await.ok();
            return Err(DiarizeError::Download(format!(
                "{name}: server reports {t} bytes, expected {e}; not downloading"
            )));
        }
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
        if let Some(t) = expected.or(total) {
            if done < t {
                return Err(DiarizeError::Download(format!(
                    "incomplete download: {done} of {t} bytes"
                )));
            }
            if done > t {
                tokio::fs::remove_file(&part).await.ok();
                return Err(DiarizeError::Download(format!(
                    "{name} is {done} bytes, expected {t}; deleted the download, please try again"
                )));
            }
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
        let checked = DiarizeModels::with_models_dir(dir.path());
        let m = checked.clone().with_size_checks(false);
        assert_eq!(m.dir(), dir.path().join("diarize"));
        assert!(!m.is_ready());
        std::fs::create_dir_all(m.dir()).unwrap();
        std::fs::write(m.segmentation_path(), b"x").unwrap();
        let st = m.status();
        assert!(st.segmentation && !st.embedding && !st.ready);
        std::fs::write(m.embedding_path(), b"y").unwrap();
        assert!(m.is_ready());
        // With the real size checks, truncated files are not ready.
        let st = checked.status();
        assert!(!st.segmentation && !st.embedding && !st.ready);
        m.delete().unwrap();
        assert!(!m.status().segmentation);
        m.delete().unwrap();
    }

    #[tokio::test]
    async fn corrupt_archive_is_deleted_not_reused() {
        let dir = tempfile::tempdir().unwrap();
        let m = DiarizeModels::with_models_dir(dir.path())
            .with_urls(
                "http://127.0.0.1:9/seg.tar.bz2",
                "http://127.0.0.1:9/emb.onnx",
            )
            .with_size_checks(false);
        std::fs::create_dir_all(m.dir()).unwrap();
        let archive = m.dir().join(SEGMENTATION_ARCHIVE);
        std::fs::write(&archive, b"not a bzip2 archive").unwrap();
        // The leftover archive is used (download skipped), fails to extract, and is removed.
        let err = m.download(|_| {}).await.unwrap_err();
        assert!(err.to_string().contains("damaged"), "{err}");
        assert!(!archive.exists());
        assert!(!m.status().segmentation);
    }

    #[tokio::test]
    async fn wrong_size_archive_is_discarded_before_extracting() {
        let dir = tempfile::tempdir().unwrap();
        let m = DiarizeModels::with_models_dir(dir.path()).with_urls(
            "http://127.0.0.1:9/seg.tar.bz2",
            "http://127.0.0.1:9/emb.onnx",
        );
        std::fs::create_dir_all(m.dir()).unwrap();
        let archive = m.dir().join(SEGMENTATION_ARCHIVE);
        std::fs::write(&archive, b"truncated").unwrap();
        // Wrong size: deleted, then re-downloaded (which fails here: nothing listens).
        assert!(matches!(
            m.download(|_| {}).await,
            Err(DiarizeError::Download(_))
        ));
        assert!(!archive.exists());
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
