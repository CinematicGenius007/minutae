//! Small pure helpers: day-group labels, markdown parsing, file names, dir sizes, levels.

use std::path::{Path, PathBuf};

use chrono::{Datelike, NaiveDate};

/// "Today", "Yesterday", "Earlier this week", "Last week", else "November 2026".
/// Weeks start on Monday.
pub fn day_label(date: NaiveDate, today: NaiveDate) -> String {
    let days = (today - date).num_days();
    if days <= 0 {
        return "Today".into();
    }
    if days == 1 {
        return "Yesterday".into();
    }
    let week_start = today - chrono::Duration::days(today.weekday().num_days_from_monday() as i64);
    if date >= week_start {
        "Earlier this week".into()
    } else if date >= week_start - chrono::Duration::days(7) {
        "Last week".into()
    } else {
        date.format("%B %Y").to_string()
    }
}

/// Bullets under `## <heading>` in a summary markdown file.
pub fn markdown_bullets(markdown: &str, heading: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in markdown.lines() {
        let t = line.trim();
        if let Some(h) = t.strip_prefix("## ") {
            inside = h.trim().eq_ignore_ascii_case(heading);
            continue;
        }
        if inside && let Some(b) = t.strip_prefix("- ") {
            out.push(b.trim().to_string());
        }
    }
    out
}

/// Lowercase ASCII slug, at most 48 chars.
pub fn slug(title: &str) -> String {
    let mut s = String::new();
    let mut dash = false;
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash && !s.is_empty() {
            s.push('-');
            dash = true;
        }
        if s.len() >= 48 {
            break;
        }
    }
    let s = s.trim_matches('-').to_string();
    if s.is_empty() { "meeting".into() } else { s }
}

/// Recursive size of a file or directory (missing = 0).
pub fn dir_size(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.is_file() {
        return meta.len();
    }
    if !meta.is_dir() {
        return 0;
    }
    std::fs::read_dir(path)
        .map(|rd| rd.flatten().map(|e| dir_size(&e.path())).sum())
        .unwrap_or(0)
}

/// Every file under `root` (recursive), skipping symlinks.
pub fn walk_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                stack.push(e.path());
            } else if ft.is_file() {
                out.push(e.path());
            }
        }
    }
    out.sort();
    out
}

/// File types `open_path` may hand to the OS (never executables or scripts).
pub const OPENABLE_EXTENSIONS: [&str; 3] = ["md", "wav", "txt"];

/// Validate a path the UI asks to open or reveal: absolute, no `..`, existing, and
/// (after resolving symlinks) inside one of `roots` or equal to one of `exact`.
/// With `open`, only folders and [`OPENABLE_EXTENSIONS`] files are accepted.
pub fn checked_user_path(
    path: &str,
    roots: &[PathBuf],
    exact: &[PathBuf],
    open: bool,
) -> Result<PathBuf, String> {
    let denied = || "Minutae can only open its own files.".to_string();
    let p = Path::new(path);
    if !p.is_absolute()
        || p.components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(denied());
    }
    let real = p
        .canonicalize()
        .map_err(|_| "That file no longer exists.".to_string())?;
    let canon = |r: &PathBuf| r.canonicalize().ok();
    let inside = roots
        .iter()
        .filter_map(canon)
        .any(|r| real.starts_with(&r) && r.parent().is_some())
        || exact.iter().filter_map(canon).any(|e| e == real);
    if !inside {
        return Err(denied());
    }
    if open && !real.is_dir() {
        let ext = real
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase);
        if !ext.is_some_and(|e| OPENABLE_EXTENSIONS.contains(&e.as_str())) {
            return Err("Minutae only opens notes, transcripts and recordings.".into());
        }
    }
    Ok(real)
}

/// A folder the user may choose for recordings/summaries: absolute, no `..`, and not
/// the top of a disk (that would expose everything through the asset protocol).
pub fn acceptable_folder(path: &Path) -> bool {
    path.is_absolute()
        && path.parent().is_some()
        && !path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
}

/// A global shortcut must have a modifier other than Shift (Ctrl/Alt/Cmd), unless it
/// is a function key, and must differ from Minutae's other shortcut.
pub fn validate_accelerator(
    accelerator: &str,
    other: &str,
) -> Result<tauri_plugin_global_shortcut::Shortcut, String> {
    use std::str::FromStr;
    use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};
    let s = Shortcut::from_str(accelerator.trim())
        .map_err(|_| "That isn't a shortcut Minutae can use.".to_string())?;
    let function_key = matches!(
        s.key,
        Code::F1
            | Code::F2
            | Code::F3
            | Code::F4
            | Code::F5
            | Code::F6
            | Code::F7
            | Code::F8
            | Code::F9
            | Code::F10
            | Code::F11
            | Code::F12
            | Code::F13
            | Code::F14
            | Code::F15
            | Code::F16
            | Code::F17
            | Code::F18
            | Code::F19
            | Code::F20
            | Code::F21
            | Code::F22
            | Code::F23
            | Code::F24
    );
    let real_modifier = s
        .mods
        .intersects(Modifiers::CONTROL | Modifiers::ALT | Modifiers::SUPER);
    if !function_key && !real_modifier {
        return Err("Choose a shortcut with a modifier key".into());
    }
    if let Ok(o) = Shortcut::from_str(other.trim())
        && o.mods == s.mods
        && o.key == s.key
    {
        return Err("Minutae already uses that shortcut for something else".into());
    }
    Ok(s)
}

/// RMS (0..1) → meter level (0..1) on a -60..0 dBFS scale.
pub fn meter_level(rms: f32) -> f32 {
    if rms <= 0.0 {
        return 0.0;
    }
    let db = 20.0 * rms.max(1e-6).log10();
    ((db + 60.0) / 60.0).clamp(0.0, 1.0)
}

/// "Hold ⌥ Space to dictate" style label for a shortcut string.
pub fn shortcut_label(shortcut: &str) -> String {
    let mac = cfg!(target_os = "macos");
    shortcut
        .split('+')
        .map(|p| match (p.trim(), mac) {
            ("Alt" | "Option", true) => "⌥".to_string(),
            ("CommandOrControl" | "CmdOrCtrl", true) | ("Command" | "Cmd" | "Super", true) => {
                "⌘".to_string()
            }
            ("Control" | "Ctrl", true) => "⌃".to_string(),
            ("Shift", true) => "⇧".to_string(),
            ("CommandOrControl" | "CmdOrCtrl", false) => "Ctrl".to_string(),
            (other, _) => other.to_string(),
        })
        .collect::<Vec<_>>()
        .join(if mac { "" } else { "+" })
}

/// `12:04` or `1:02:03`.
pub fn clock(ms: u64) -> String {
    let s = ms / 1000;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// "AIza••••3kQ": first 4 and last 3 characters.
pub fn mask_key(key: &str) -> String {
    let chars: Vec<char> = key.trim().chars().collect();
    if chars.len() <= 8 {
        return "••••".into();
    }
    let head: String = chars[..4].iter().collect();
    let tail: String = chars[chars.len() - 3..].iter().collect();
    format!("{head}••••{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn labels() {
        // 2026-11-20 is a Friday.
        let today = d(2026, 11, 20);
        assert_eq!(day_label(today, today), "Today");
        assert_eq!(day_label(d(2026, 11, 19), today), "Yesterday");
        assert_eq!(day_label(d(2026, 11, 16), today), "Earlier this week");
        assert_eq!(day_label(d(2026, 11, 15), today), "Last week");
        assert_eq!(day_label(d(2026, 11, 9), today), "Last week");
        assert_eq!(day_label(d(2026, 11, 8), today), "November 2026");
        assert_eq!(day_label(d(2026, 10, 1), today), "October 2026");
    }

    #[test]
    fn bullets() {
        let md = "---\ntitle: x\n---\n\n## Summary\n- One\n- Two\n\n## Key Decisions\n- D\n";
        assert_eq!(markdown_bullets(md, "Summary"), vec!["One", "Two"]);
        assert_eq!(markdown_bullets(md, "Key Decisions"), vec!["D"]);
    }

    #[test]
    fn misc() {
        assert_eq!(slug("Q2 Design Sync!"), "q2-design-sync");
        assert_eq!(slug("—"), "meeting");
        assert_eq!(clock(65_000), "1:05");
        assert_eq!(clock(3_723_000), "1:02:03");
        assert_eq!(mask_key("AIzaSyD-very-long-key-3kQ"), "AIza••••3kQ");
        assert_eq!(mask_key("short"), "••••");
        assert_eq!(meter_level(0.0), 0.0);
        assert!((meter_level(1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn user_paths_are_confined() {
        let data = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let md = data.path().join("a.md");
        std::fs::write(&md, "# x").unwrap();
        let exe = data.path().join("run.command");
        std::fs::write(&exe, "#!/bin/sh").unwrap();
        let outside = other.path().join("b.md");
        std::fs::write(&outside, "# y").unwrap();
        let zip = other.path().join("export.zip");
        std::fs::write(&zip, "z").unwrap();
        let roots = vec![data.path().to_path_buf()];
        let s = |p: &Path| p.to_string_lossy().into_owned();

        assert!(checked_user_path(&s(&md), &roots, &[], true).is_ok());
        assert!(checked_user_path(&s(data.path()), &roots, &[], true).is_ok());
        assert!(checked_user_path(&s(&exe), &roots, &[], true).is_err());
        assert!(checked_user_path(&s(&exe), &roots, &[], false).is_ok());
        assert!(checked_user_path(&s(&outside), &roots, &[], false).is_err());
        let sneaky = data
            .path()
            .join("..")
            .join(other.path().file_name().unwrap())
            .join("b.md");
        assert!(checked_user_path(&s(&sneaky), &roots, &[], false).is_err());
        assert!(checked_user_path("relative/a.md", &roots, &[], false).is_err());
        assert!(checked_user_path(&s(&zip), &roots, &[], false).is_err());
        assert!(checked_user_path(&s(&zip), &roots, std::slice::from_ref(&zip), false).is_ok());
        // A root of "/" never counts.
        assert!(checked_user_path(&s(&outside), &[PathBuf::from("/")], &[], false).is_err());
        #[cfg(unix)]
        {
            let link = data.path().join("link.md");
            std::os::unix::fs::symlink(&outside, &link).unwrap();
            assert!(checked_user_path(&s(&link), &roots, &[], true).is_err());
        }
    }

    #[test]
    fn folder_choice() {
        // Absolute paths need a drive on Windows.
        #[cfg(windows)]
        let (good, root, sneaky) = (r"C:\Users\me\Notes", r"C:\", r"C:\Users\me\..\..\Windows");
        #[cfg(not(windows))]
        let (good, root, sneaky) = ("/Users/me/Notes", "/", "/Users/me/../../etc");
        assert!(acceptable_folder(Path::new(good)));
        assert!(!acceptable_folder(Path::new(root)));
        assert!(!acceptable_folder(Path::new("notes")));
        assert!(!acceptable_folder(Path::new(sneaky)));
    }

    #[test]
    fn accelerators() {
        let rec = "CommandOrControl+Alt+R";
        assert!(validate_accelerator("Alt+Space", rec).is_ok());
        assert!(validate_accelerator("Control+Shift+D", rec).is_ok());
        assert!(validate_accelerator("F5", rec).is_ok());
        assert!(validate_accelerator("Shift+F13", rec).is_ok());
        assert_eq!(
            validate_accelerator("Shift+D", rec).unwrap_err(),
            "Choose a shortcut with a modifier key"
        );
        assert!(validate_accelerator("Space", rec).is_err());
        assert!(validate_accelerator("Alt+R+Q+nonsense", rec).is_err());
        assert!(validate_accelerator(rec, rec).is_err());
        assert!(validate_accelerator("CmdOrCtrl+Option+R", rec).is_err());
    }
}
