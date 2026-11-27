//! Handle-relative operations on recorder-owned files.

use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, DirBuilder, File, OpenOptions};
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
        imported: Option<&str>,
    ) -> io::Result<Option<u64>>;
    fn track_exists(&self, path: &Path, id: MeetingId) -> bool;
    fn delete_audio(
        &self,
        directory: &Path,
        id: MeetingId,
        imported: Option<&str>,
    ) -> io::Result<()>;
    fn markdown_bytes(&self, path: &Path, id: MeetingId) -> io::Result<Option<u64>>;
    fn delete_markdown(&self, path: &Path, id: MeetingId) -> io::Result<()>;
}

pub struct LocalMeetingFiles;

pub(super) fn unsafe_path() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "This path is not a regular meeting-owned file or folder.",
    )
}

/// What sits at an export path can't be shown to be this meeting's file. Unlike
/// a disk failure, this is a reason to leave it alone and carry on.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct NotOwned(&'static str);

fn unowned(kind: io::ErrorKind, reason: &'static str) -> io::Error {
    io::Error::new(kind, NotOwned(reason))
}

pub(super) fn not_owned(error: &io::Error) -> bool {
    error.get_ref().is_some_and(|e| e.is::<NotOwned>())
}

pub(super) fn present<T>(result: io::Result<T>) -> io::Result<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

impl LocalMeetingFiles {
    fn audio_folder(&self, directory: &Path, id: MeetingId) -> io::Result<(Dir, String)> {
        let name = id.to_string();
        if directory.file_name().and_then(|n| n.to_str()) != Some(&name) {
            return Err(unsafe_path());
        }
        let root = directory.parent().ok_or_else(unsafe_path)?;
        let parent = open_ambient(root).map_err(|e| unavailable(root, e))?;
        Ok((parent, name))
    }

    pub fn recording_sizes(&self, directory: &Path, id: MeetingId) -> io::Result<(u64, usize)> {
        let (parent, name) = self.audio_folder(directory, id)?;
        let Some(dir) = present(parent.open_dir_nofollow(name))? else {
            return Ok((0, 0));
        };
        audio_sizes(&dir, None).map(|(bytes, tracks)| (bytes.unwrap_or(0), tracks))
    }

    #[cfg(test)]
    pub fn find_markdown(&self, root: &Path, id: MeetingId) -> Option<PathBuf> {
        self.find_markdowns(root, id).ok()?.into_iter().next()
    }

    pub fn find_markdowns(&self, root: &Path, id: MeetingId) -> io::Result<Vec<PathBuf>> {
        let dir = self.summary_folder(root)?;
        let suffix = format!("-{}.md", &id.simple().to_string()[..8]);
        let names = summary_names(&dir).map_err(|e| unavailable(root, e))?;
        Ok(names
            .into_iter()
            .filter(|name| {
                name.to_str().is_some_and(|n| n.ends_with(&suffix))
                    && export_metadata(&dir, name, id).is_ok_and(|bytes| bytes.is_some())
            })
            .map(|name| root.join(name))
            .collect())
    }

    pub fn scan_markdowns(
        &self,
        root: &Path,
        ids: &[MeetingId],
    ) -> io::Result<Vec<(MeetingId, PathBuf)>> {
        let dir = self.summary_folder(root)?;
        let mut prefixes = std::collections::BTreeMap::<String, Vec<MeetingId>>::new();
        for id in ids {
            prefixes
                .entry(id.simple().to_string()[..8].to_owned())
                .or_default()
                .push(*id);
        }
        let mut found = Vec::new();
        for name in summary_names(&dir).map_err(|e| unavailable(root, e))? {
            if let Some(ids) = name
                .to_str()
                .and_then(|s| s.strip_suffix(".md"))
                .and_then(|s| s.rsplit_once('-'))
                .and_then(|(_, prefix)| prefixes.get(prefix))
            {
                for &id in ids {
                    if export_metadata(&dir, &name, id).is_ok_and(|size| size.is_some()) {
                        found.push((id, root.join(&name)));
                    }
                }
            }
        }
        Ok(found)
    }

    fn summary_folder(&self, root: &Path) -> io::Result<Dir> {
        open_ambient(root).map_err(|e| unavailable(root, e))
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

#[derive(Debug, thiserror::Error)]
#[error("The storage folder ‘{root}’ is unavailable: {source}")]
struct StorageUnavailable {
    root: PathBuf,
    source: io::Error,
}

fn unavailable(root: &Path, error: io::Error) -> io::Error {
    io::Error::new(
        error.kind(),
        StorageUnavailable {
            root: root.into(),
            source: error,
        },
    )
}

pub(super) fn storage_unavailable(error: &io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(|e| e.is::<StorageUnavailable>())
}

fn audio_file_bytes(metadata: cap_std::fs::Metadata) -> io::Result<Option<u64>> {
    if metadata.is_file() {
        Ok(Some(metadata.len()))
    } else if metadata.is_symlink() {
        Ok(None)
    } else {
        Err(unsafe_path())
    }
}

fn remove_audio_file(dir: &Dir, name: impl AsRef<Path>) -> io::Result<()> {
    if let Some(meta) = present(dir.symlink_metadata(name.as_ref()))?
        && audio_file_bytes(meta)?.is_some()
    {
        present(dir.remove_file(name))?;
    }
    Ok(())
}

/// WAV files produced by the recorder, including chunks still being written.
fn audio_sizes(dir: &Dir, imported: Option<&str>) -> io::Result<(Option<u64>, usize)> {
    let mut bytes = None;
    let mut tracks = BTreeSet::new();
    for name in AUDIO_FILES
        .into_iter()
        .chain(imported.filter(|name| !AUDIO_FILES.contains(name)))
    {
        if let Some(meta) = present(dir.symlink_metadata(name))?
            && let Some(size) = audio_file_bytes(meta)?
        {
            bytes = Some(bytes.unwrap_or(0) + size);
            if matches!(name, "mic.wav" | "system.wav") || imported == Some(name) {
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
                if let Some(meta) = present(chunks.symlink_metadata(name))?
                    && let Some(size) = audio_file_bytes(meta)?
                {
                    bytes = Some(bytes.unwrap_or(0) + size);
                    tracks.extend(track);
                }
            }
        }
    }
    Ok((bytes, tracks.len()))
}

fn chunk_track(name: &str) -> Option<&'static str> {
    let (track, seq) = name.strip_suffix(".wav")?.split_once('-')?;
    let number: u32 = seq.parse().ok()?;
    if seq != format!("{number:06}") {
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
        imported: Option<&str>,
    ) -> io::Result<Option<u64>> {
        let (parent, name) = match self.audio_folder(directory, id) {
            Err(e) if !required && e.kind() == io::ErrorKind::NotFound => return Ok(None),
            result => result?,
        };
        let Some(dir) = present(parent.open_dir_nofollow(name))? else {
            return Ok(None);
        };
        audio_sizes(&dir, imported).map(|(bytes, _)| bytes)
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

    fn delete_audio(
        &self,
        directory: &Path,
        id: MeetingId,
        imported: Option<&str>,
    ) -> io::Result<()> {
        let (parent, name) = self.audio_folder(directory, id)?;
        let Some(dir) = present(parent.open_dir_nofollow(&name))? else {
            return Ok(());
        };
        // Only touch regular files with names owned by the recorder or importer.
        // Paths below the trusted storage root are handled without following links.
        for file in AUDIO_FILES
            .into_iter()
            .chain(imported.filter(|name| !AUDIO_FILES.contains(name)))
        {
            remove_audio_file(&dir, file)?;
        }
        if let Some(chunks) = present(dir.open_dir_nofollow("chunks"))? {
            for entry in chunks.entries()? {
                let entry = entry?;
                let name = entry.file_name();
                if chunk_track(&name.to_string_lossy()).is_some() || stitch_marker(&name) {
                    remove_audio_file(&chunks, name)?;
                }
            }
            drop(chunks);
            remove_empty(&dir, "chunks")?;
        }
        drop(dir);
        remove_empty(&parent, &name)
    }

    fn markdown_bytes(&self, path: &Path, id: MeetingId) -> io::Result<Option<u64>> {
        let (dir, name) = export_location(path)?;
        export_metadata(&dir, name, id)
    }

    fn delete_markdown(&self, path: &Path, id: MeetingId) -> io::Result<()> {
        change_export(path, id, |previous| {
            Ok(previous.map(|_| ExportChange::Delete))
        })?;
        Ok(())
    }
}

const STAGE_PREFIX: &str = ".minutae-pending-";
const STAGED_FILE: &str = "original";
/// Marks a staged original that was replaced and only still needs removing.
const SUPERSEDED: &str = "superseded";

fn stage_name(name: &OsStr) -> OsString {
    let mut pending = OsString::from(STAGE_PREFIX);
    pending.push(name);
    pending
}

fn summary_names(dir: &Dir) -> io::Result<BTreeSet<OsString>> {
    dir.entries()?
        .map(|entry| {
            let name = entry?.file_name();
            Ok(
                match name
                    .to_str()
                    .and_then(|name| name.strip_prefix(STAGE_PREFIX))
                {
                    Some(original) => original.into(),
                    None => name,
                },
            )
        })
        .collect()
}

fn export_location(path: &Path) -> io::Result<(Dir, &OsStr)> {
    let root = path.parent().ok_or_else(unsafe_path)?;
    let name = path.file_name().ok_or_else(unsafe_path)?;
    Ok((open_ambient(root).map_err(|e| unavailable(root, e))?, name))
}

fn export_metadata(dir: &Dir, name: &OsStr, id: MeetingId) -> io::Result<Option<u64>> {
    open_export(dir, name, id)?
        .map(|file| file.metadata().map(|m| m.len()))
        .transpose()
}

fn open_export(dir: &Dir, name: &OsStr, id: MeetingId) -> io::Result<Option<File>> {
    if let Some(file) = verified_export(dir, name, id)? {
        return Ok(Some(file));
    }
    let Some(stage) = present(dir.open_dir_nofollow(stage_name(name)))? else {
        return Ok(None);
    };
    if present(stage.symlink_metadata(SUPERSEDED))?.is_some() {
        return Ok(None);
    }
    verified_export(&stage, OsStr::new(STAGED_FILE), id)
}

fn verified_export(dir: &Dir, name: &OsStr, id: MeetingId) -> io::Result<Option<File>> {
    let Some(metadata) = present(dir.symlink_metadata(name))? else {
        return Ok(None);
    };
    let not_a_file = || {
        unowned(
            io::ErrorKind::PermissionDenied,
            "This path is not a regular meeting-owned file or folder.",
        )
    };
    if !metadata.is_file() {
        return Err(not_a_file());
    }
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        use cap_std::fs::OpenOptionsExt;
        options.custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32);
    }
    let Some(mut file) = present(dir.open_with(name, &options))? else {
        return Ok(None);
    };
    if !file.metadata()?.is_file() {
        return Err(not_a_file());
    }
    let identified = match has_summary_id(&file, id) {
        // A header that isn't text proves nothing.
        Err(error) if error.kind() == io::ErrorKind::InvalidData => false,
        result => result?,
    };
    if !identified {
        return Err(unowned(
            io::ErrorKind::InvalidData,
            "The Markdown file does not identify this meeting.",
        ));
    }
    use std::io::Seek;
    file.rewind()?;
    Ok(Some(file))
}

pub(super) enum ExportChange {
    Delete,
    Write {
        bytes: Vec<u8>,
        keep_edited_copy: bool,
    },
}

/// The filesystem steps whose support or success varies between disks.
trait FileOps {
    /// Rename in one step, failing with `AlreadyExists` instead of replacing
    /// anything. `Unsupported` means this disk has no such operation.
    fn rename_noreplace(&self, from: &Dir, old: &OsStr, to: &Dir, new: &OsStr) -> io::Result<()> {
        rename_noreplace(from, old, to, new)
    }

    fn hard_link(&self, from: &Dir, old: &OsStr, to: &Dir, new: &OsStr) -> io::Result<()> {
        from.hard_link(old, to, new)
    }

    fn remove_staged(&self, stage: &Dir) -> io::Result<()> {
        stage.remove_file(STAGED_FILE)
    }
}

struct NativeOps;

impl FileOps for NativeOps {}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn rename_noreplace(from: &Dir, old: &OsStr, to: &Dir, new: &OsStr) -> io::Result<()> {
    use rustix::fs::{RenameFlags, renameat_with};
    use rustix::io::Errno;
    renameat_with(from, old, to, new, RenameFlags::NOREPLACE).map_err(|error| {
        // exFAT, FAT and many network disks refuse the flag in one of these ways.
        if [Errno::NOSYS, Errno::INVAL, Errno::NOTSUP, Errno::OPNOTSUPP].contains(&error) {
            io::ErrorKind::Unsupported.into()
        } else {
            error.into()
        }
    })
}

#[cfg(windows)]
fn rename_noreplace(from: &Dir, old: &OsStr, to: &Dir, new: &OsStr) -> io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
    let old = wide_path(from, old)?;
    let new = wide_path(to, new)?;
    // Without flags this never replaces the destination, never copies between
    // volumes, and moves a link itself rather than what it points to. It works
    // for directories and on FAT, exFAT and network disks.
    // SAFETY: both arguments are NUL-terminated UTF-16 that outlives the call.
    if unsafe { MoveFileExW(old.as_ptr(), new.as_ptr(), 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// The path of `name` inside an open folder, resolved from the folder's handle
/// as `Dir::rename` does. The `\\?\` prefix is kept so long paths work.
#[cfg(windows)]
fn wide_path(dir: &Dir, name: &OsStr) -> io::Result<Vec<u16>> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_NAME_NORMALIZED, GetFinalPathNameByHandleW, VOLUME_NAME_DOS,
    };
    if Path::new(name).file_name() != Some(name) || name.encode_wide().any(|unit| unit == 0) {
        return Err(unsafe_path());
    }
    let mut path = vec![0_u16; 260];
    loop {
        // SAFETY: `dir` keeps the handle open and `path` is writable for the
        // length passed.
        let length = unsafe {
            GetFinalPathNameByHandleW(
                dir.as_raw_handle(),
                path.as_mut_ptr(),
                path.len() as u32,
                FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
            )
        } as usize;
        if length == 0 {
            return Err(io::Error::last_os_error());
        }
        if length < path.len() {
            path.truncate(length);
            break;
        }
        path.resize(length + 1, 0);
    }
    let separator = u16::from(b'\\');
    if path.last() != Some(&separator) {
        path.push(separator);
    }
    path.extend(name.encode_wide());
    path.push(0);
    Ok(path)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn rename_noreplace(_: &Dir, _: &OsStr, _: &Dir, _: &OsStr) -> io::Result<()> {
    Err(io::ErrorKind::Unsupported.into())
}

/// Move without ever replacing an existing file.
fn move_noclobber(
    ops: &dyn FileOps,
    from: &Dir,
    old: &OsStr,
    to: &Dir,
    new: &OsStr,
) -> io::Result<()> {
    match ops.rename_noreplace(from, old, to, new) {
        Err(error) if error.kind() == io::ErrorKind::Unsupported => {}
        result => return result,
    }
    let metadata = from.symlink_metadata(old)?;
    if metadata.is_dir() {
        // Claim the name first. A rename may then replace only an empty
        // folder, never a file.
        to.create_dir(new)?;
        let moved = from.rename(old, to, new);
        if moved.is_err() {
            let _ = to.remove_dir(new);
        }
        return moved;
    }
    match ops.hard_link(from, old, to, new) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Err(error),
        Err(_) if metadata.is_file() => copy_new(from, old, to, new)?,
        #[cfg(unix)]
        Err(_) if metadata.is_symlink() => {
            to.symlink_contents(from.read_link_contents(old)?, new)?;
        }
        Err(error) => return Err(error),
    }
    // The destination is complete, so the move has happened. A source that
    // can't be removed yet is left behind.
    let _ = from.remove_file(old);
    Ok(())
}

/// For disks without links: exclusive creation is what prevents replacement.
fn copy_new(from: &Dir, old: &OsStr, to: &Dir, new: &OsStr) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    let mut source = from.open_with(old, &options)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use cap_std::fs::{OpenOptionsExt, PermissionsExt};
        options.mode(source.metadata()?.permissions().mode() & 0o777);
    }
    let mut copy = to.open_with(new, &options)?;
    let result = io::copy(&mut source, &mut copy).and_then(|_| copy.sync_all());
    #[cfg(unix)]
    if result.is_err() {
        // Take back the partial copy, if the name still refers to it.
        use cap_std::fs::MetadataExt;
        let identity = |m: cap_std::fs::Metadata| (m.dev(), m.ino());
        if let (Ok(ours), Ok(there)) = (copy.metadata(), to.symlink_metadata(new))
            && identity(ours) == identity(there)
        {
            let _ = to.remove_file(new);
        }
    }
    result
}

fn write_new(ops: &dyn FileOps, dir: &Dir, name: &OsStr, bytes: &[u8]) -> io::Result<()> {
    // Move a complete file into place without replacing anything that arrived
    // at the destination while the export was being prepared.
    let tmp = format!(".minutae-write-{}.tmp", uuid::Uuid::new_v4());
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = dir.open_with(&tmp, &options)?;
    let result = file.write_all(bytes).and_then(|()| file.sync_all());
    drop(file);
    let result = result.and_then(|()| move_noclobber(ops, dir, OsStr::new(&tmp), dir, name));
    let _ = dir.remove_file(&tmp);
    result
}

fn keep_edited(ops: &dyn FileOps, dir: &Dir, name: &OsStr, bytes: &[u8]) -> io::Result<()> {
    let stem = Path::new(name).file_stem().ok_or_else(unsafe_path)?;
    let mut copy = stem.to_os_string();
    copy.push(format!(".edited-{}.md", uuid::Uuid::new_v4().simple()));
    write_new(ops, dir, &copy, bytes)
}

fn restore_staged(ops: &dyn FileOps, stage: &Dir, dir: &Dir, name: &OsStr) -> io::Result<()> {
    if dir.symlink_metadata(name).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "The Markdown path was replaced while Minutae was working. The original is in the adjacent .minutae-pending folder.",
        ));
    }
    move_noclobber(ops, stage, OsStr::new(STAGED_FILE), dir, name)
}

/// Remove a staged original whose content is no longer needed. If the disk
/// won't let go of it, remember that, so it isn't mistaken for the only copy.
fn discard_staged(ops: &dyn FileOps, stage: &Dir) -> io::Result<()> {
    ops.remove_staged(stage).inspect_err(|_| {
        let _ = stage.write(SUPERSEDED, b"");
    })
}

/// Finish whatever an interrupted or incomplete change left in its staging
/// folder.
fn recover_stage(ops: &dyn FileOps, dir: &Dir, name: &OsStr, id: MeetingId) -> io::Result<()> {
    let pending = stage_name(name);
    let Some(stage) = present(dir.open_dir_nofollow(&pending))? else {
        return Ok(());
    };
    if present(stage.symlink_metadata(STAGED_FILE))?.is_some() {
        if present(stage.symlink_metadata(SUPERSEDED))?.is_some() {
            discard_staged(ops, &stage)?;
        } else if present(dir.symlink_metadata(name))?.is_none() {
            restore_staged(ops, &stage, dir, name)?;
        } else {
            // A crash after publication can leave both versions. Preserve
            // the staged version as an unmanaged copy before continuing,
            // unless the public file already holds the same bytes.
            let original = read_verified(verified_export(&stage, OsStr::new(STAGED_FILE), id)?)?
                .ok_or_else(unsafe_path)?;
            let public = verified_export(dir, name, id).and_then(read_verified);
            if !matches!(public, Ok(Some(ref bytes)) if *bytes == original) {
                keep_edited(ops, dir, name, &original)?;
            }
            discard_staged(ops, &stage)?;
        }
    }
    if present(stage.symlink_metadata(STAGED_FILE))?.is_none() {
        let _ = stage.remove_file(SUPERSEDED);
    }
    drop(stage);
    // An empty folder that stays behind is reused by the next change.
    let _ = remove_empty(dir, &pending);
    Ok(())
}

/// Stage by rename inside the trusted root so validation and unlink concern
/// the same file, even if another process has replaced the public name.
pub(super) fn change_export(
    path: &Path,
    id: MeetingId,
    prepare: impl FnOnce(Option<&[u8]>) -> io::Result<Option<ExportChange>>,
) -> io::Result<()> {
    change_export_with(&NativeOps, path, id, prepare)
}

fn change_export_with(
    ops: &dyn FileOps,
    path: &Path,
    id: MeetingId,
    prepare: impl FnOnce(Option<&[u8]>) -> io::Result<Option<ExportChange>>,
) -> io::Result<()> {
    let (dir, name) = export_location(path)?;
    let pending = stage_name(name);
    recover_stage(ops, &dir, name, id)?;
    let previous = read_verified(verified_export(&dir, name, id)?)?;
    let Some(change) = prepare(previous.as_deref())? else {
        return Ok(());
    };
    if previous.is_none() {
        return match change {
            ExportChange::Write { bytes, .. } => write_new(ops, &dir, name, &bytes),
            ExportChange::Delete => Ok(()),
        };
    }
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut builder = DirBuilder::new();
    #[cfg(unix)]
    {
        use cap_std::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    match dir.create_dir_with(&pending, &builder) {
        Err(error) if error.kind() != io::ErrorKind::AlreadyExists => return Err(error),
        _ => {}
    }
    let stage = dir.open_dir_nofollow(&pending)?;
    let mut moved = false;
    let mut published = false;
    let result = (|| {
        dir.rename(name, &stage, STAGED_FILE)?;
        moved = true;
        let actual = read_verified(verified_export(&stage, OsStr::new(STAGED_FILE), id)?)?;
        if actual != previous {
            return Err(io::Error::other(
                "The Markdown file changed while Minutae was working. Try again.",
            ));
        }
        match change {
            ExportChange::Write {
                bytes,
                keep_edited_copy,
            } => {
                if keep_edited_copy {
                    keep_edited(ops, &dir, name, actual.as_deref().ok_or_else(unsafe_path)?)?;
                }
                write_new(ops, &dir, name, &bytes)?;
                published = true;
                // The new file is in place: failing to tidy up must not undo
                // the change. The next one tries again.
                let _ = discard_staged(ops, &stage);
            }
            ExportChange::Delete => ops.remove_staged(&stage)?,
        }
        Ok(())
    })();
    if result.is_err() && moved && !published {
        restore_staged(ops, &stage, &dir, name)?;
    }
    drop(stage);
    let _ = remove_empty(&dir, &pending);
    result
}

fn read_verified(file: Option<File>) -> io::Result<Option<Vec<u8>>> {
    file.map(|mut file| {
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok(bytes)
    })
    .transpose()
}

pub fn read_export(path: &Path, id: MeetingId) -> io::Result<Option<Vec<u8>>> {
    let (dir, name) = export_location(path)?;
    read_verified(open_export(&dir, name, id)?)
}

fn remove_empty(dir: &Dir, name: impl AsRef<Path>) -> io::Result<()> {
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
    const HEADER_LIMIT: u64 = 4096;
    let mut header = BufReader::new(file.take(HEADER_LIMIT + 1));
    let mut line = String::new();
    let mut bytes = header.read_line(&mut line)? as u64;
    if line.trim_end_matches(['\r', '\n']) != "---" {
        return Ok(false);
    }
    let mut matching_id = false;
    loop {
        line.clear();
        let count = header.read_line(&mut line)? as u64;
        bytes += count;
        if count == 0 || bytes > HEADER_LIMIT {
            return Ok(false);
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line == "---" {
            return Ok(matching_id);
        }
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        // Accept the plain, top-level mapping Minutae writes. Complex YAML
        // keys, merge keys and continuations cannot prove ownership.
        let Some((key, value)) = line.split_once(':') else {
            return Ok(false);
        };
        if key.is_empty()
            || !key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        {
            return Ok(false);
        }
        if key == "id" {
            if matching_id || uuid::Uuid::parse_str(value.trim()).ok() != Some(id) {
                return Ok(false);
            }
            matching_id = true;
        }
    }
}

pub fn summary_belongs_to(path: &Path, id: MeetingId) -> io::Result<bool> {
    let (dir, name) = export_location(path)?;
    Ok(export_metadata(&dir, name, id)?.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn export_replacement_restores_directory_swapped_in_after_verification() {
        let tmp = tempfile::tempdir().unwrap();
        let id = uuid::Uuid::new_v4();
        let path = tmp.path().join("summary.md");
        let original = format!("---\nid: {id}\n---\nOld summary");
        std::fs::write(&path, original).unwrap();

        let failure = change_export(&path, id, |_| {
            std::fs::remove_file(&path)?;
            std::fs::create_dir(&path)?;
            std::fs::write(path.join("personal.txt"), "keep this")?;
            Ok(Some(ExportChange::Write {
                bytes: b"new".to_vec(),
                keep_edited_copy: false,
            }))
        });
        assert!(failure.is_err());
        assert_eq!(
            std::fs::read_to_string(path.join("personal.txt")).unwrap(),
            "keep this"
        );
        assert!(
            !tmp.path()
                .join(stage_name(OsStr::new("summary.md")))
                .exists()
        );
    }

    #[test]
    fn no_replace_move_preserves_existing_destination() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = open_ambient(tmp.path()).unwrap();
        dir.write("source", b"source").unwrap();
        dir.write("existing", b"existing").unwrap();
        for ops in disks() {
            assert!(
                move_noclobber(
                    ops,
                    &dir,
                    OsStr::new("source"),
                    &dir,
                    OsStr::new("existing")
                )
                .is_err()
            );
            assert_eq!(dir.read("source").unwrap(), b"source");
            assert_eq!(dir.read("existing").unwrap(), b"existing");
        }
    }

    /// A disk such as exFAT: no no-replace rename, optionally no hard links.
    struct PlainDisk {
        links: bool,
    }

    impl FileOps for PlainDisk {
        fn rename_noreplace(&self, _: &Dir, _: &OsStr, _: &Dir, _: &OsStr) -> io::Result<()> {
            Err(io::ErrorKind::Unsupported.into())
        }

        fn hard_link(&self, from: &Dir, old: &OsStr, to: &Dir, new: &OsStr) -> io::Result<()> {
            if self.links {
                from.hard_link(old, to, new)
            } else {
                Err(io::ErrorKind::Unsupported.into())
            }
        }
    }

    fn disks() -> [&'static dyn FileOps; 3] {
        [
            &NativeOps,
            &PlainDisk { links: true },
            &PlainDisk { links: false },
        ]
    }

    fn write(bytes: &[u8], keep_edited_copy: bool) -> io::Result<Option<ExportChange>> {
        Ok(Some(ExportChange::Write {
            bytes: bytes.to_vec(),
            keep_edited_copy,
        }))
    }

    fn names(root: &Path) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(root)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            // macOS keeps metadata in sidecar files on FAT and exFAT.
            .filter(|name| !name.starts_with("._"))
            .collect();
        names.sort();
        names
    }

    #[test]
    fn exports_can_be_written_replaced_and_deleted_on_every_kind_of_disk() {
        let id = uuid::Uuid::new_v4();
        let version = |text: &str| format!("---\nid: {id}\n---\n{text}").into_bytes();
        for ops in disks() {
            let tmp = tempfile::tempdir().unwrap();
            let path = tmp.path().join("summary.md");
            change_export_with(ops, &path, id, |_| write(&version("One"), false)).unwrap();
            change_export_with(ops, &path, id, |_| write(&version("Two"), true)).unwrap();
            assert_eq!(fs::read(&path).unwrap(), version("Two"));
            let kept = names(tmp.path());
            assert_eq!(kept.len(), 2, "{kept:?}");
            let copy = kept.iter().find(|n| n.contains(".edited-")).unwrap();
            assert_eq!(fs::read(tmp.path().join(copy)).unwrap(), version("One"));
            change_export_with(ops, &path, id, |_| Ok(Some(ExportChange::Delete))).unwrap();
            assert_eq!(names(tmp.path()), std::slice::from_ref(copy));
        }
    }

    #[test]
    fn a_file_arriving_during_a_change_is_never_replaced() {
        let id = uuid::Uuid::new_v4();
        let original = format!("---\nid: {id}\n---\nOld");
        for ops in disks() {
            let tmp = tempfile::tempdir().unwrap();
            let path = tmp.path().join("summary.md");
            let failure = change_export_with(ops, &path, id, |_| {
                fs::write(&path, b"Someone else's")?;
                write(b"new", false)
            });
            assert!(failure.is_err());
            assert_eq!(fs::read(&path).unwrap(), b"Someone else's");
            assert_eq!(names(tmp.path()), ["summary.md"]);

            // The same when there is an original to put back.
            fs::write(&path, &original).unwrap();
            let stage = tmp.path().join(stage_name(OsStr::new("summary.md")));
            fs::create_dir(&stage).unwrap();
            fs::write(stage.join(STAGED_FILE), &original).unwrap();
            let dir = open_ambient(tmp.path()).unwrap();
            let staged = dir.open_dir_nofollow(stage.file_name().unwrap()).unwrap();
            fs::write(&path, b"Someone else's").unwrap();
            assert!(restore_staged(ops, &staged, &dir, OsStr::new("summary.md")).is_err());
            assert_eq!(fs::read(&path).unwrap(), b"Someone else's");
            assert_eq!(
                fs::read(stage.join(STAGED_FILE)).unwrap(),
                original.as_bytes()
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn directories_and_links_swapped_in_are_restorable_without_a_noreplace_rename() {
        let id = uuid::Uuid::new_v4();
        let original = format!("---\nid: {id}\n---\nOld");
        for ops in disks() {
            for link in [false, true] {
                let tmp = tempfile::tempdir().unwrap();
                let path = tmp.path().join("summary.md");
                fs::write(&path, &original).unwrap();
                let failure = change_export_with(ops, &path, id, |_| {
                    fs::remove_file(&path)?;
                    if link {
                        std::os::unix::fs::symlink("elsewhere", &path)?;
                    } else {
                        fs::create_dir(&path)?;
                        fs::write(path.join("personal.txt"), "keep this")?;
                    }
                    write(b"new", false)
                });
                assert!(failure.is_err());
                if link {
                    assert_eq!(fs::read_link(&path).unwrap(), Path::new("elsewhere"));
                } else {
                    assert_eq!(
                        fs::read_to_string(path.join("personal.txt")).unwrap(),
                        "keep this"
                    );
                }
                assert_eq!(names(tmp.path()), ["summary.md"]);
            }
        }
    }

    /// A scanner holding the staged original open, as antivirus software does.
    struct HeldOpen(std::cell::Cell<bool>);

    impl FileOps for HeldOpen {
        fn remove_staged(&self, stage: &Dir) -> io::Result<()> {
            if self.0.get() {
                Err(io::ErrorKind::PermissionDenied.into())
            } else {
                stage.remove_file(STAGED_FILE)
            }
        }
    }

    #[test]
    fn a_staged_file_that_cannot_be_removed_does_not_fail_a_published_change() {
        let tmp = tempfile::tempdir().unwrap();
        let id = uuid::Uuid::new_v4();
        let path = tmp.path().join("summary.md");
        let version = |text: &str| format!("---\nid: {id}\n---\n{text}").into_bytes();
        let pending = stage_name(OsStr::new("summary.md")).into_string().unwrap();
        fs::write(&path, version("One")).unwrap();
        let held = HeldOpen(true.into());

        change_export_with(&held, &path, id, |_| write(&version("Two"), false)).unwrap();
        assert_eq!(fs::read(&path).unwrap(), version("Two"));
        assert!(tmp.path().join(&pending).join(STAGED_FILE).exists());

        // Still held: the next change says so and leaves everything in place.
        assert!(change_export_with(&held, &path, id, |_| write(b"no", false)).is_err());
        assert_eq!(fs::read(&path).unwrap(), version("Two"));

        held.0.set(false);
        change_export_with(&held, &path, id, |_| write(&version("Three"), false)).unwrap();
        assert_eq!(fs::read(&path).unwrap(), version("Three"));
        // The replaced version is cleaned up, not kept as a copy.
        assert_eq!(names(tmp.path()), ["summary.md"]);
    }

    #[test]
    fn a_replaced_original_awaiting_removal_is_not_read_as_the_export() {
        let tmp = tempfile::tempdir().unwrap();
        let id = uuid::Uuid::new_v4();
        let path = tmp.path().join("summary.md");
        fs::write(&path, format!("---\nid: {id}\n---\nOne")).unwrap();
        let held = HeldOpen(true.into());
        let bytes = format!("---\nid: {id}\n---\nTwo").into_bytes();
        change_export_with(&held, &path, id, |_| write(&bytes, false)).unwrap();
        fs::remove_file(&path).unwrap();
        assert_eq!(read_export(&path, id).unwrap(), None);
        LocalMeetingFiles.delete_markdown(&path, id).unwrap();
        assert!(names(tmp.path()).is_empty());
    }

    #[test]
    fn deletion_recovers_staged_markdown_and_restores_a_foreign_replacement() {
        let temp = tempfile::tempdir().unwrap();
        let id = uuid::Uuid::new_v4();
        let path = temp.path().join("summary.md");
        let stage = temp.path().join(stage_name(path.file_name().unwrap()));
        fs::create_dir(&stage).unwrap();
        fs::write(
            stage.join(STAGED_FILE),
            format!("---\nid: {id}\n---\nOwned"),
        )
        .unwrap();
        assert!(
            LocalMeetingFiles
                .markdown_bytes(&path, id)
                .unwrap()
                .is_some()
        );
        LocalMeetingFiles.delete_markdown(&path, id).unwrap();
        assert!(!path.exists());
        assert!(!stage.exists());

        fs::create_dir(&stage).unwrap();
        fs::write(stage.join(STAGED_FILE), b"New user's file").unwrap();
        assert!(LocalMeetingFiles.delete_markdown(&path, id).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"New user's file");
    }
}
