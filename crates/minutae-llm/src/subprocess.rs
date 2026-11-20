//! Subprocess runner and binary resolution for the CLI transports.
//!
//! Rules (ARCHITECTURE §5.3): run the real binary path, never a shell or shell function;
//! prompt on stdin (or a temp file); empty `0700` temp working directory removed afterwards;
//! filtered environment; hard timeout; cancellation kills the child; one call in flight per runner.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use minutae_core::FailureKind;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::types::{LlmError, clip};

/// Environment variables passed through to CLIs. Everything else is dropped.
#[cfg(not(windows))]
const ENV_KEYS: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "TMPDIR",
    "LANG",
    "LC_ALL",
    "CODEX_HOME",
];
#[cfg(windows)]
const ENV_KEYS: &[&str] = &[
    "PATH",
    "PATHEXT",
    "SYSTEMROOT",
    "SYSTEMDRIVE",
    "WINDIR",
    "COMSPEC",
    "TEMP",
    "TMP",
    "USERPROFILE",
    "USERNAME",
    "HOMEDRIVE",
    "HOMEPATH",
    "APPDATA",
    "LOCALAPPDATA",
    "PROGRAMDATA",
    "PROGRAMFILES",
    "CODEX_HOME",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubprocessOutput {
    /// None when the child was killed by a signal.
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl SubprocessOutput {
    pub fn success(&self) -> bool {
        self.exit_code == Some(0)
    }
}

/// One runner per transport. Calls are serialised (the second waits for the first).
#[derive(Debug)]
pub struct SubprocessRunner {
    output_limit: usize,
    busy: Mutex<()>,
}

impl Default for SubprocessRunner {
    fn default() -> Self {
        Self::new(1_048_576)
    }
}

impl SubprocessRunner {
    pub fn new(output_limit: usize) -> Self {
        Self {
            output_limit,
            busy: Mutex::new(()),
        }
    }

    /// Runs `program args…` in `cwd` with `stdin` written then closed.
    pub async fn run(
        &self,
        program: &Path,
        args: &[OsString],
        stdin: &str,
        cwd: &Path,
        timeout: Duration,
        cancel: &CancellationToken,
    ) -> Result<SubprocessOutput, LlmError> {
        if cancel.is_cancelled() {
            return Err(LlmError::cancelled());
        }
        let _guard = tokio::select! {
            g = self.busy.lock() => g,
            _ = cancel.cancelled() => return Err(LlmError::cancelled()),
        };

        let mut cmd = Command::new(program);
        cmd.args(args)
            .current_dir(cwd)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for key in ENV_KEYS {
            if let Some(v) = std::env::var_os(key) {
                if *key == "PATH" {
                    cmd.env(key, path_with_parent(&v, program));
                } else {
                    cmd.env(key, v);
                }
            }
        }
        if std::env::var_os("PATH").is_none() {
            cmd.env("PATH", path_with_parent(&OsString::new(), program));
        }
        #[cfg(windows)]
        {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        #[cfg(unix)]
        cmd.process_group(0);

        let started = tokio::time::Instant::now();
        let deadline = started + timeout;
        // Linux returns ETXTBSY ("Text file busy") when the binary was just written and
        // another thread's fork still holds it open for a moment (seen after CLI updates
        // and with test scripts). It is transient: retry briefly instead of failing.
        let mut attempt = 0u64;
        let mut child = loop {
            match cmd.spawn() {
                Ok(c) => break c,
                Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy && attempt < 5 => {
                    attempt += 1;
                    tokio::time::sleep(Duration::from_millis(25 * attempt)).await;
                }
                Err(e) => return Err(spawn_error(&e, program)),
            }
        };
        // The whole tree (npm launchers spawn a native grandchild) dies with this guard.
        let tree = ProcessTree::attach(&child);
        let mut stdin_pipe = child.stdin.take();
        let input = stdin.as_bytes().to_vec();
        let writer = tokio::spawn(async move {
            if let Some(mut pipe) = stdin_pipe.take() {
                let _ = pipe.write_all(&input).await;
                let _ = pipe.shutdown().await;
            }
        });
        let out_buf = SharedBuf::default();
        let err_buf = SharedBuf::default();
        let mut out = tokio::spawn(read_capped(
            child.stdout.take(),
            self.output_limit,
            out_buf.clone(),
        ));
        let mut err = tokio::spawn(read_capped(
            child.stderr.take(),
            self.output_limit,
            err_buf.clone(),
        ));

        let outcome = tokio::select! {
            status = child.wait() => Ok(status),
            _ = tokio::time::sleep_until(deadline) => Err(LlmError::timeout()),
            _ = cancel.cancelled() => Err(LlmError::cancelled()),
        };
        let status = match outcome {
            Ok(Ok(status)) => status,
            Ok(Err(e)) => {
                tree.kill();
                writer.abort();
                out.abort();
                err.abort();
                return Err(LlmError::unavailable(format!(
                    "could not wait for subprocess: {e}"
                )));
            }
            Err(stop) => {
                tree.kill();
                let _ = child.start_kill();
                let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
                writer.abort();
                out.abort();
                err.abort();
                return Err(stop);
            }
        };

        // The direct child exited, but a grandchild may still hold stdout/stderr open.
        // Drain only until the original deadline, then kill the tree and keep what was read.
        let drained = tokio::select! {
            _ = async { let _ = (&mut out).await; let _ = (&mut err).await; } => Ok(()),
            _ = tokio::time::sleep_until(deadline) => Err(None),
            _ = cancel.cancelled() => Err(Some(LlmError::cancelled())),
        };
        if let Err(stop) = drained {
            tree.kill();
            let grace = Duration::from_millis(500);
            if tokio::time::timeout(grace, &mut out).await.is_err() {
                out.abort();
            }
            if tokio::time::timeout(grace, &mut err).await.is_err() {
                err.abort();
            }
            if let Some(stop) = stop {
                writer.abort();
                return Err(stop);
            }
        }
        writer.abort();
        // Leftover background processes from the CLI do not outlive the call.
        tree.kill();
        Ok(SubprocessOutput {
            exit_code: status.code(),
            stdout: out_buf.take(),
            stderr: err_buf.take(),
        })
    }
}

/// Bytes read so far from a pipe; survives aborting the reader task.
#[derive(Debug, Clone, Default)]
struct SharedBuf(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl SharedBuf {
    fn take(&self) -> String {
        let bytes = std::mem::take(&mut *self.0.lock().unwrap_or_else(|e| e.into_inner()));
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

/// Handle on the child's whole process tree: a process group on Unix, a Job Object on Windows.
/// Killing it (or dropping it) kills grandchildren too.
struct ProcessTree {
    #[cfg(unix)]
    pgid: Option<i32>,
    #[cfg(windows)]
    job: Option<win_job::Job>,
}

impl ProcessTree {
    fn attach(child: &tokio::process::Child) -> Self {
        #[cfg(unix)]
        {
            // `process_group(0)` made the child the leader of a group whose id is its pid.
            Self {
                pgid: child.id().and_then(|id| i32::try_from(id).ok()),
            }
        }
        #[cfg(windows)]
        {
            Self {
                job: child.raw_handle().and_then(win_job::Job::assign),
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = child;
            Self {}
        }
    }

    fn kill(&self) {
        #[cfg(unix)]
        if let Some(pgid) = self.pgid {
            // SAFETY: plain syscall; ESRCH (group already gone) is ignored.
            unsafe {
                libc::killpg(pgid, libc::SIGKILL);
            }
        }
        #[cfg(windows)]
        if let Some(job) = &self.job {
            job.terminate();
        }
    }
}

impl Drop for ProcessTree {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(windows)]
mod win_job {
    use std::ffi::c_void;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject,
    };

    /// A Job Object with KILL_ON_JOB_CLOSE. Stored as an integer so the guard stays `Send`.
    pub(super) struct Job(isize);

    impl Job {
        /// Creates a job and puts `process` in it. Children the process starts afterwards
        /// join the job automatically.
        pub(super) fn assign(process: std::os::windows::io::RawHandle) -> Option<Self> {
            // SAFETY: Win32 calls with valid pointers; the handle is closed on drop.
            unsafe {
                let job: HANDLE = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if job.is_null() {
                    return None;
                }
                let guard = Job(job as isize);
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let ok = SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                if ok == 0 || AssignProcessToJobObject(job, process as HANDLE) == 0 {
                    return None;
                }
                Some(guard)
            }
        }

        pub(super) fn terminate(&self) {
            // SAFETY: the handle is owned and open.
            unsafe {
                TerminateJobObject(self.0 as HANDLE, 1);
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY: closing our own handle; KILL_ON_JOB_CLOSE ends any survivors.
            unsafe {
                CloseHandle(self.0 as HANDLE);
            }
        }
    }
}

fn spawn_error(e: &std::io::Error, program: &Path) -> LlmError {
    let name = program
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    match e.kind() {
        std::io::ErrorKind::NotFound => LlmError::not_installed(format!("{name} was not found")),
        std::io::ErrorKind::PermissionDenied => LlmError::new(
            FailureKind::PermissionDenied,
            format!("{name} could not be executed"),
        ),
        _ => LlmError::unavailable(format!("could not start {name}: {e}")),
    }
}

/// Reads everything into `kept`, keeping at most `limit` bytes (the rest is drained so the
/// child never blocks).
async fn read_capped<R: AsyncRead + Unpin>(pipe: Option<R>, limit: usize, kept: SharedBuf) {
    let Some(mut pipe) = pipe else {
        return;
    };
    let mut buf = [0u8; 16_384];
    loop {
        match pipe.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let mut kept = kept.0.lock().unwrap_or_else(|e| e.into_inner());
                let room = limit.saturating_sub(kept.len());
                kept.extend_from_slice(&buf[..n.min(room)]);
            }
        }
    }
}

/// Puts the binary's own directory on PATH so `#!/usr/bin/env node` scripts (codex) find
/// their interpreter when the app was launched from the GUI with a minimal PATH.
fn path_with_parent(path: &OsString, program: &Path) -> OsString {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(parent) = program.parent() {
        dirs.push(parent.to_path_buf());
    }
    dirs.extend(std::env::split_paths(path));
    std::env::join_paths(dirs).unwrap_or_else(|_| path.clone())
}

/// A private scratch directory (`0700` on Unix), removed on drop.
pub(crate) fn scratch_dir(prefix: &str) -> Result<tempfile::TempDir, LlmError> {
    let io = |e: std::io::Error| {
        LlmError::new(
            FailureKind::Io,
            format!("could not create a temp directory: {e}"),
        )
    };
    let dir = tempfile::Builder::new()
        .prefix(prefix)
        .tempdir()
        .map_err(io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).map_err(io)?;
    }
    Ok(dir)
}

// ---------------------------------------------------------------------------
// Binary resolution
// ---------------------------------------------------------------------------

/// Resolves `name` to a real executable file without consulting a shell.
///
/// - An absolute path is checked directly.
/// - A bare name is searched on `path` (and on Windows with each `PATHEXT` extension).
/// - Relative paths containing a separator are rejected.
pub fn resolve_binary(name: &str, path: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    let p = Path::new(name);
    if p.is_absolute() {
        return candidates_for(p).into_iter().find_map(|c| executable(&c));
    }
    if name.contains('/') || name.contains('\\') || name.is_empty() {
        return None;
    }
    let path = path
        .map(|p| p.to_os_string())
        .or_else(|| std::env::var_os("PATH"))?;
    for dir in std::env::split_paths(&path) {
        if !dir.is_absolute() {
            continue;
        }
        if let Some(found) = candidates_for(&dir.join(name))
            .into_iter()
            .find_map(|c| executable(&c))
        {
            return Some(found);
        }
    }
    None
}

/// PATH first, then each known location (absolute paths).
pub fn resolve_with_fallbacks(name: &str, known: &[PathBuf]) -> Option<PathBuf> {
    resolve_binary(name, None).or_else(|| {
        known
            .iter()
            .find_map(|k| k.to_str().and_then(|s| resolve_binary(s, None)))
    })
}

fn candidates_for(base: &Path) -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        if base.extension().is_some() {
            return vec![base.to_path_buf()];
        }
        let exts = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
        let mut out: Vec<PathBuf> = exts
            .split(';')
            .filter(|e| !e.is_empty())
            .map(|e| {
                let mut s = base.as_os_str().to_os_string();
                s.push(e.to_ascii_lowercase());
                PathBuf::from(s)
            })
            .collect();
        out.push(base.to_path_buf());
        out
    }
    #[cfg(not(windows))]
    {
        vec![base.to_path_buf()]
    }
}

fn executable(path: &Path) -> Option<PathBuf> {
    let real = std::fs::canonicalize(path).ok()?;
    let meta = std::fs::metadata(&real).ok()?;
    if !meta.is_file() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o111 == 0 {
            return None;
        }
    }
    #[cfg(windows)]
    {
        let ext = real
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !matches!(ext.as_str(), "exe" | "cmd" | "bat" | "com") {
            return None;
        }
    }
    // Keep the path the caller named when canonicalisation produced a `\\?\` prefix on Windows.
    if cfg!(windows) {
        Some(path.to_path_buf())
    } else {
        Some(real)
    }
}

/// Common per-user and system install directories for a CLI named `name`.
pub fn known_locations(name: &str, extra_home_dirs: &[&str]) -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    let mut out: Vec<PathBuf> = extra_home_dirs
        .iter()
        .map(|d| home.join(d).join(name))
        .collect();
    #[cfg(windows)]
    {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            out.push(PathBuf::from(&appdata).join("npm").join(name));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            let local = PathBuf::from(local);
            out.push(local.join("Programs").join(name).join(name));
            out.push(
                local
                    .join("Microsoft")
                    .join("WinGet")
                    .join("Links")
                    .join(name),
            );
        }
        out.push(home.join(".local").join("bin").join(name));
        out.push(home.join("scoop").join("shims").join(name));
    }
    #[cfg(not(windows))]
    {
        out.push(home.join(".local/bin").join(name));
        out.push(PathBuf::from("/opt/homebrew/bin").join(name));
        out.push(PathBuf::from("/usr/local/bin").join(name));
        out.push(PathBuf::from("/usr/bin").join(name));
        out.push(home.join(".npm-global/bin").join(name));
        out.push(home.join(".bun/bin").join(name));
        out.push(home.join(".volta/bin").join(name));
    }
    out
}

// ---------------------------------------------------------------------------
// Error classification
// ---------------------------------------------------------------------------

/// Case-insensitive substring → kind. Earlier patterns win.
#[derive(Debug, Clone, Default)]
pub struct ErrorClassifier {
    patterns: Vec<(String, FailureKind)>,
}

impl ErrorClassifier {
    pub fn new(patterns: &[(&str, FailureKind)]) -> Self {
        Self {
            patterns: patterns
                .iter()
                .map(|(t, k)| (t.to_lowercase(), *k))
                .collect(),
        }
    }

    /// Appends patterns after the existing ones.
    pub fn registering(mut self, more: &[(&str, FailureKind)]) -> Self {
        self.patterns
            .extend(more.iter().map(|(t, k)| (t.to_lowercase(), *k)));
        self
    }

    /// Kind of the first matching pattern in `text`.
    pub fn match_text(&self, text: &str) -> Option<FailureKind> {
        let lower = text.to_lowercase();
        self.patterns
            .iter()
            .find(|(p, _)| !p.is_empty() && lower.contains(p.as_str()))
            .map(|(_, k)| *k)
    }

    /// None for a zero exit. Otherwise the first matching pattern, else `Unavailable`.
    pub fn classify(&self, out: &SubprocessOutput) -> Option<LlmError> {
        if out.success() {
            return None;
        }
        let detail = last_meaningful_line(&out.stderr)
            .or_else(|| last_meaningful_line(&out.stdout))
            .map(|l| clip(l, 240))
            .unwrap_or_else(|| match out.exit_code {
                Some(c) => format!("process exited with status {c}"),
                None => "process was killed".into(),
            });
        let kind = self
            .match_text(&out.stderr)
            .or_else(|| self.match_text(&out.stdout))
            .unwrap_or(FailureKind::Unavailable);
        Some(LlmError::new(kind, detail))
    }
}

/// Patterns every CLI shares. Registered after the transport's own.
pub(crate) const COMMON_PATTERNS: &[(&str, FailureKind)] = &[
    ("rate limit", FailureKind::RateLimited),
    ("rate_limit", FailureKind::RateLimited),
    ("too many requests", FailureKind::RateLimited),
    ("usage limit", FailureKind::RateLimited),
    ("quota exceeded", FailureKind::RateLimited),
    ("status 429", FailureKind::RateLimited),
    ("not logged in", FailureKind::NotLoggedIn),
    ("not signed in", FailureKind::NotLoggedIn),
    ("login required", FailureKind::NotLoggedIn),
    ("authentication required", FailureKind::NotLoggedIn),
    ("unauthorized", FailureKind::NotLoggedIn),
    ("token expired", FailureKind::NotLoggedIn),
    ("timed out", FailureKind::Timeout),
];

fn last_meaningful_line(s: &str) -> Option<&str> {
    s.lines().map(str::trim).rev().find(|l| !l.is_empty())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn classifier_orders_and_falls_back() {
        let c = ErrorClassifier::new(&[("sign in", FailureKind::NotLoggedIn)])
            .registering(COMMON_PATTERNS);
        let out = |code, err: &str| SubprocessOutput {
            exit_code: Some(code),
            stdout: String::new(),
            stderr: err.into(),
        };
        assert!(c.classify(&out(0, "rate limit")).is_none());
        assert_eq!(
            c.classify(&out(1, "Please SIGN IN again")).unwrap().kind,
            FailureKind::NotLoggedIn
        );
        assert_eq!(
            c.classify(&out(1, "Error: 429 Too Many Requests"))
                .unwrap()
                .kind,
            FailureKind::RateLimited
        );
        let e = c
            .classify(&out(2, "error: unexpected argument '--x' found\n"))
            .unwrap();
        assert_eq!(e.kind, FailureKind::Unavailable);
        assert_eq!(e.detail, "error: unexpected argument '--x' found");
    }

    #[test]
    fn rejects_relative_paths_with_separators() {
        assert!(resolve_binary("../codex", None).is_none());
        assert!(resolve_binary("", None).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn resolves_only_executables_on_given_path() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("tool");
        std::fs::write(&exe, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(resolve_binary("tool", Some(dir.path().as_os_str())).is_none());
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        let found = resolve_binary("tool", Some(dir.path().as_os_str())).unwrap();
        assert_eq!(found, std::fs::canonicalize(&exe).unwrap());
        // A directory with the same name is not a binary.
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        assert!(resolve_binary("sub", Some(dir.path().as_os_str())).is_none());
    }

    #[cfg(unix)]
    pub(crate) fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let p = dir.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn passes_stdin_and_captures_output() {
        let dir = tempfile::tempdir().unwrap();
        let bin = script(dir.path(), "echo-stdin", "cat; echo oops >&2; exit 3");
        let r = SubprocessRunner::default();
        let out = r
            .run(
                &bin,
                &[],
                "hello prompt",
                dir.path(),
                Duration::from_secs(5),
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(out.exit_code, Some(3));
        assert_eq!(out.stdout, "hello prompt");
        assert_eq!(out.stderr.trim(), "oops");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn env_is_filtered() {
        let dir = tempfile::tempdir().unwrap();
        let bin = script(dir.path(), "env-dump", "env");
        // SAFETY: test-only; no other test reads this variable.
        unsafe { std::env::set_var("MINUTAE_SECRET_TEST_VAR", "x") };
        let out = SubprocessRunner::default()
            .run(
                &bin,
                &[],
                "",
                dir.path(),
                Duration::from_secs(5),
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(!out.stdout.contains("MINUTAE_SECRET_TEST_VAR"));
        assert!(out.stdout.contains("PATH="));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn times_out_and_kills() {
        let dir = tempfile::tempdir().unwrap();
        let bin = script(dir.path(), "slow", "sleep 10");
        let started = std::time::Instant::now();
        let err = SubprocessRunner::default()
            .run(
                &bin,
                &[],
                "",
                dir.path(),
                Duration::from_millis(200),
                &CancellationToken::new(),
            )
            .await
            .unwrap_err();
        assert_eq!(err.kind, FailureKind::Timeout);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[cfg(unix)]
    async fn wait_gone(pid_file: &Path) -> bool {
        let pid: i32 = std::fs::read_to_string(pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        for _ in 0..40 {
            // SAFETY: signal 0 only probes for existence.
            if unsafe { libc::kill(pid, 0) } != 0 {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        false
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn timeout_kills_grandchildren() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        let bin = script(
            dir.path(),
            "launcher",
            &format!("sleep 30 & echo $! > '{}'; wait", pid_file.display()),
        );
        let started = std::time::Instant::now();
        let err = SubprocessRunner::default()
            .run(
                &bin,
                &[],
                "",
                dir.path(),
                Duration::from_millis(1000),
                &CancellationToken::new(),
            )
            .await
            .unwrap_err();
        assert_eq!(err.kind, FailureKind::Timeout);
        assert!(started.elapsed() < Duration::from_secs(4));
        assert!(
            wait_gone(&pid_file).await,
            "grandchild survived the timeout"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn grandchild_holding_pipe_cannot_outlast_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        // The direct child exits at once; its background child keeps stdout open.
        let bin = script(
            dir.path(),
            "leaky",
            &format!("sleep 30 & echo $! > '{}'; echo done", pid_file.display()),
        );
        let started = std::time::Instant::now();
        let out = SubprocessRunner::default()
            .run(
                &bin,
                &[],
                "",
                dir.path(),
                Duration::from_millis(500),
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(started.elapsed() < Duration::from_secs(3));
        assert_eq!(out.stdout.trim(), "done");
        assert!(out.success());
        assert!(wait_gone(&pid_file).await, "grandchild survived");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cancellation_kills() {
        let dir = tempfile::tempdir().unwrap();
        let bin = script(dir.path(), "slow", "sleep 10");
        let token = CancellationToken::new();
        let t2 = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            t2.cancel();
        });
        let err = SubprocessRunner::default()
            .run(&bin, &[], "", dir.path(), Duration::from_secs(30), &token)
            .await
            .unwrap_err();
        assert_eq!(err.kind, FailureKind::Cancelled);
    }

    #[tokio::test]
    async fn missing_binary_is_not_installed() {
        let dir = tempfile::tempdir().unwrap();
        let err = SubprocessRunner::default()
            .run(
                &dir.path().join("nope"),
                &[],
                "",
                dir.path(),
                Duration::from_secs(1),
                &CancellationToken::new(),
            )
            .await
            .unwrap_err();
        assert_eq!(err.kind, FailureKind::NotInstalled);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn output_is_capped() {
        let dir = tempfile::tempdir().unwrap();
        let bin = script(dir.path(), "big", "head -c 100000 /dev/zero | tr '\\0' a");
        let out = SubprocessRunner::new(1000)
            .run(
                &bin,
                &[],
                "",
                dir.path(),
                Duration::from_secs(5),
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(out.stdout.len(), 1000);
        assert!(out.success());
    }
}
