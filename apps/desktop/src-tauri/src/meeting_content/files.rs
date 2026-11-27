//! Handle-relative operations on recorder-owned files.

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, BufRead, BufReader, Read};
use std::path::{Component, Path, PathBuf};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use minutae_core::MeetingId;

const AUDIO_FILES: [&str; 6] = [
    "mic.wav",
    "system.wav",
    minutae_aec::CLEAN_MIC_FILE,
    "mic.wav.tmp",
    "system.wav.tmp",
    "mic.clean.wav.part",
];
pub trait MeetingFiles {
    fn audio_bytes(&self, directory: &Path) -> io::Result<Option<u64>>;
    fn delete_audio(&self, directory: &Path, id: MeetingId) -> io::Result<()>;
    fn markdown_bytes(&self, path: &Path) -> io::Result<Option<u64>>;
    fn delete_markdown(&self, path: &Path, id: MeetingId) -> io::Result<()>;
    fn check_path(&self, path: &Path) -> io::Result<()>;
}

pub struct LocalMeetingFiles(pub PathBuf);

pub(super) fn unsafe_path() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "Only meeting files inside Minutae’s data directory can be deleted.",
    )
}

pub(super) fn present<T>(result: io::Result<T>) -> io::Result<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

impl LocalMeetingFiles {
    /// Open each directory component without following symlinks. All destructive
    /// operations below use a directory handle and a single child name.
    fn directory(&self, path: &Path) -> io::Result<Option<Dir>> {
        let relative = path.strip_prefix(&self.0).map_err(|_| unsafe_path())?;
        if relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err(unsafe_path());
        }
        let mut dir = match (self.0.parent(), self.0.file_name()) {
            (Some(parent), Some(name)) => {
                let parent = if parent.as_os_str().is_empty() {
                    Path::new(".")
                } else {
                    parent
                };
                Dir::open_ambient_dir(parent, cap_std::ambient_authority())?
                    .open_dir_nofollow(name)?
            }
            _ => Dir::open_ambient_dir(&self.0, cap_std::ambient_authority())?,
        };
        for part in relative.components() {
            let Some(next) = present(dir.open_dir_nofollow(part.as_os_str()))? else {
                return Ok(None);
            };
            dir = next;
        }
        Ok(Some(dir))
    }
}

fn file_bytes(metadata: fs::Metadata) -> io::Result<u64> {
    if metadata.is_file() {
        Ok(metadata.len())
    } else {
        Err(unsafe_path())
    }
}

/// WAV files produced by the recorder, including chunks still being written.
pub fn audio_sizes(directory: &Path) -> io::Result<(u64, usize)> {
    let mut bytes = 0;
    let mut tracks = BTreeSet::new();
    let Some(dir) = present(fs::symlink_metadata(directory))? else {
        return Ok((0, 0));
    };
    if !dir.is_dir() {
        return Err(unsafe_path());
    }
    for name in AUDIO_FILES {
        if let Some(meta) = present(fs::symlink_metadata(directory.join(name)))? {
            bytes += file_bytes(meta)?;
            if matches!(name, "mic.wav" | "system.wav") {
                tracks.insert(name);
            }
        }
    }
    let chunks = directory.join("chunks");
    if let Some(meta) = present(fs::symlink_metadata(&chunks))? {
        if !meta.is_dir() {
            return Err(unsafe_path());
        }
        for e in fs::read_dir(chunks)? {
            let e = e?;
            if let Some(track) = chunk_track(&e.file_name().to_string_lossy()) {
                // Capture may rotate a chunk while the inventory is being read.
                if let Some(meta) = present(fs::symlink_metadata(e.path()))? {
                    bytes += file_bytes(meta)?;
                    tracks.insert(track);
                }
            }
        }
    }
    Ok((bytes, tracks.len()))
}

fn chunk_track(name: &str) -> Option<&'static str> {
    let (track, seq) = name.strip_suffix(".wav")?.split_once('-')?;
    if seq.len() < 6 || !seq.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    match track {
        "mic" => Some("mic.wav"),
        "system" => Some("system.wav"),
        _ => None,
    }
}

impl MeetingFiles for LocalMeetingFiles {
    fn audio_bytes(&self, directory: &Path) -> io::Result<Option<u64>> {
        let (bytes, tracks) = audio_sizes(directory)?;
        Ok((bytes > 0 || tracks > 0).then_some(bytes))
    }

    fn delete_audio(&self, directory: &Path, id: MeetingId) -> io::Result<()> {
        let name = id.to_string();
        if directory.file_name().and_then(|n| n.to_str()) != Some(&name) {
            return Err(unsafe_path());
        }
        let parent = directory.parent().ok_or_else(unsafe_path)?;
        let Some(parent) = self.directory(parent)? else {
            return Ok(());
        };
        let Some(dir) = present(parent.open_dir_nofollow(&name))? else {
            return Ok(());
        };
        // Only touch names owned by this recorder. Unlinking a file or symlink
        // through a directory handle never follows its target.
        for file in AUDIO_FILES {
            present(dir.remove_file_or_symlink(file))?;
        }
        if let Some(chunks) = present(dir.open_dir_nofollow("chunks"))? {
            for entry in chunks.entries()? {
                let entry = entry?;
                let name = entry.file_name();
                if chunk_track(&name.to_string_lossy()).is_some()
                    || matches!(
                        name.to_str(),
                        Some(
                            "mic.stitched"
                                | "system.stitched"
                                | "mic.stitched.tmp"
                                | "system.stitched.tmp"
                        )
                    )
                {
                    present(chunks.remove_file_or_symlink(entry.file_name()))?;
                }
            }
            drop(chunks);
            remove_empty(&dir, "chunks")?;
        }
        drop(dir);
        remove_empty(&parent, &name)
    }

    fn markdown_bytes(&self, path: &Path) -> io::Result<Option<u64>> {
        present(fs::symlink_metadata(path))?
            .map(file_bytes)
            .transpose()
    }

    fn delete_markdown(&self, path: &Path, id: MeetingId) -> io::Result<()> {
        let Some(dir) = self.directory(path.parent().ok_or_else(unsafe_path)?)? else {
            return Ok(());
        };
        let name = path.file_name().ok_or_else(unsafe_path)?;
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let Some(file) = present(dir.open_with(name, &options))? else {
            return Ok(());
        };
        if !file.metadata()?.is_file() || !has_summary_id(&file, id)? {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "The Markdown file does not identify this meeting.",
            ));
        }
        drop(file);
        present(dir.remove_file(name))?;
        Ok(())
    }

    fn check_path(&self, path: &Path) -> io::Result<()> {
        self.directory(path).map(|_| ())
    }
}

fn remove_empty(dir: &Dir, name: &str) -> io::Result<()> {
    match dir.remove_dir(name) {
        Ok(()) => Ok(()),
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::DirectoryNotEmpty
            ) =>
        {
            Ok(())
        }
        Err(e) => Err(e),
    }
}

fn has_summary_id(file: impl Read, id: MeetingId) -> io::Result<bool> {
    // Only the frontmatter written by Minutae is an ownership proof.
    let mut header = BufReader::new(file.take(4096)).lines();
    if header.next().transpose()?.as_deref() != Some("---") {
        return Ok(false);
    }
    for line in header {
        let line = line?;
        if line == "---" {
            break;
        }
        if let Some(value) = line.strip_prefix("id: ") {
            return Ok(uuid::Uuid::parse_str(value.trim()).ok() == Some(id));
        }
    }
    Ok(false)
}

pub fn summary_belongs_to(path: &Path, id: MeetingId) -> io::Result<bool> {
    if !fs::symlink_metadata(path)?.is_file() {
        return Ok(false);
    }
    has_summary_id(fs::File::open(path)?, id)
}
