//! Handle-relative operations on recorder-owned files.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

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
    fn audio_bytes(
        &self,
        directory: &Path,
        id: MeetingId,
        required: bool,
    ) -> io::Result<Option<u64>>;
    fn track_exists(&self, path: &Path, id: MeetingId) -> bool;
    fn delete_audio(&self, directory: &Path, id: MeetingId) -> io::Result<()>;
    fn markdown_bytes(&self, path: &Path, id: MeetingId) -> io::Result<Option<u64>>;
    fn delete_markdown(&self, path: &Path, id: MeetingId) -> io::Result<()>;
}

pub struct LocalMeetingFiles(pub PathBuf);

pub(super) fn unsafe_path() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "This path is not a regular meeting-owned file or folder.",
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
    // Callers supply paths from settings, saved meeting records, or the active
    // recorder. A custom storage root can itself resolve through symlinks.
    // The built-in roots are opened beneath the app data directory as before.
    fn storage_folder(&self, path: &Path, default: &str) -> io::Result<Dir> {
        if path == self.0.join(default) {
            return open_default_data_dir(&self.0)?.open_dir_nofollow(default);
        }
        open_ambient(path)
    }

    fn audio_folder(&self, directory: &Path, id: MeetingId) -> io::Result<(Dir, String)> {
        let name = id.to_string();
        if directory.file_name().and_then(|n| n.to_str()) != Some(&name) {
            return Err(unsafe_path());
        }
        let root = directory.parent().ok_or_else(unsafe_path)?;
        let parent = self
            .storage_folder(root, "recordings")
            .map_err(|e| unavailable(root, e))?;
        Ok((parent, name))
    }

    pub fn recording_sizes(&self, directory: &Path, id: MeetingId) -> io::Result<(u64, usize)> {
        let (parent, name) = self.audio_folder(directory, id)?;
        let Some(dir) = present(parent.open_dir_nofollow(name))? else {
            return Ok((0, 0));
        };
        audio_sizes(&dir).map(|(bytes, tracks)| (bytes.unwrap_or(0), tracks))
    }

    pub fn find_markdown(&self, root: &Path, id: MeetingId) -> Option<PathBuf> {
        let dir = self.storage_folder(root, "summaries").ok()?;
        let suffix = format!("-{}.md", &id.simple().to_string()[..8]);
        dir.entries().ok()?.flatten().find_map(|entry| {
            let name = entry.file_name();
            (name.to_str().is_some_and(|n| n.ends_with(&suffix))
                && summary_bytes(&dir, &name, id).is_ok_and(|bytes| bytes.is_some()))
            .then(|| root.join(name))
        })
    }

    pub fn check_summaries_folder(&self, root: &Path) -> io::Result<()> {
        self.summary_folder(root)?
            .entries()
            .map(|_| ())
            .map_err(|e| unavailable(root, e))
    }

    fn summary_folder(&self, root: &Path) -> io::Result<Dir> {
        self.storage_folder(root, "summaries")
            .map_err(|e| unavailable(root, e))
    }
}

fn open_ambient(path: &Path) -> io::Result<Dir> {
    let path = if path.as_os_str().is_empty() {
        Path::new(".")
    } else {
        path
    };
    Dir::open_ambient_dir(path, cap_std::ambient_authority())
}

fn open_default_data_dir(path: &Path) -> io::Result<Dir> {
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => open_ambient(parent)?.open_dir_nofollow(name),
        _ => open_ambient(path),
    }
}

fn unavailable(root: &Path, error: io::Error) -> io::Error {
    io::Error::new(
        error.kind(),
        format!(
            "The storage folder ‘{}’ is unavailable: {error}",
            root.display()
        ),
    )
}

fn audio_file_bytes(metadata: cap_std::fs::Metadata) -> io::Result<u64> {
    if metadata.is_file() {
        Ok(metadata.len())
    } else if metadata.is_symlink() {
        // The recognized link itself can be unlinked without touching its target.
        Ok(0)
    } else {
        Err(unsafe_path())
    }
}

/// WAV files produced by the recorder, including chunks still being written.
fn audio_sizes(dir: &Dir) -> io::Result<(Option<u64>, usize)> {
    let mut bytes = None;
    let mut tracks = BTreeSet::new();
    for name in AUDIO_FILES {
        if let Some(meta) = present(dir.symlink_metadata(name))? {
            bytes = Some(bytes.unwrap_or(0) + audio_file_bytes(meta)?);
            if matches!(name, "mic.wav" | "system.wav") {
                tracks.insert(name);
            }
        }
    }
    if let Some(chunks) = present(dir.open_dir_nofollow("chunks"))? {
        for e in chunks.entries()? {
            let e = e?;
            let name = e.file_name();
            let track = chunk_track(&name.to_string_lossy());
            if track.is_some() || stitch_marker(&name) {
                // Capture may rotate a chunk while the inventory is being read.
                if let Some(meta) = present(chunks.symlink_metadata(name))? {
                    bytes = Some(bytes.unwrap_or(0) + audio_file_bytes(meta)?);
                    tracks.extend(track);
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

fn stitch_marker(name: &OsStr) -> bool {
    matches!(
        name.to_str(),
        Some("mic.stitched" | "system.stitched" | "mic.stitched.tmp" | "system.stitched.tmp")
    )
}

impl MeetingFiles for LocalMeetingFiles {
    fn audio_bytes(
        &self,
        directory: &Path,
        id: MeetingId,
        required: bool,
    ) -> io::Result<Option<u64>> {
        let (parent, name) = match self.audio_folder(directory, id) {
            Err(e) if !required && e.kind() == io::ErrorKind::NotFound => return Ok(None),
            result => result?,
        };
        let Some(dir) = present(parent.open_dir_nofollow(name))? else {
            return Ok(None);
        };
        audio_sizes(&dir).map(|(bytes, _)| bytes)
    }

    fn track_exists(&self, path: &Path, id: MeetingId) -> bool {
        let Some((directory, name)) = path.parent().zip(path.file_name()) else {
            return false;
        };
        self.audio_folder(directory, id)
            .and_then(|(parent, meeting)| parent.open_dir_nofollow(meeting))
            .and_then(|dir| dir.symlink_metadata(name))
            .is_ok_and(|metadata| metadata.is_file())
    }

    fn delete_audio(&self, directory: &Path, id: MeetingId) -> io::Result<()> {
        let (parent, name) = self.audio_folder(directory, id)?;
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
                if chunk_track(&name.to_string_lossy()).is_some() || stitch_marker(&name) {
                    present(chunks.remove_file_or_symlink(name))?;
                }
            }
            drop(chunks);
            remove_empty(&dir, "chunks")?;
        }
        drop(dir);
        remove_empty(&parent, &name)
    }

    fn markdown_bytes(&self, path: &Path, id: MeetingId) -> io::Result<Option<u64>> {
        let dir = self.summary_folder(path.parent().ok_or_else(unsafe_path)?)?;
        let name = path.file_name().ok_or_else(unsafe_path)?;
        summary_bytes(&dir, name, id)
    }

    fn delete_markdown(&self, path: &Path, id: MeetingId) -> io::Result<()> {
        let dir = self.summary_folder(path.parent().ok_or_else(unsafe_path)?)?;
        let name = path.file_name().ok_or_else(unsafe_path)?;
        if summary_bytes(&dir, name, id)?.is_some() {
            present(dir.remove_file(name))?;
        }
        Ok(())
    }
}

fn summary_bytes(dir: &Dir, name: &OsStr, id: MeetingId) -> io::Result<Option<u64>> {
    let Some(metadata) = present(dir.symlink_metadata(name))? else {
        return Ok(None);
    };
    if !metadata.is_file() {
        return Err(unsafe_path());
    }
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let Some(file) = present(dir.open_with(name, &options))? else {
        return Ok(None);
    };
    let metadata = file.metadata()?;
    if !metadata.is_file() || !has_summary_id(&file, id)? {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "The Markdown file does not identify this meeting.",
        ));
    }
    Ok(Some(metadata.len()))
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
