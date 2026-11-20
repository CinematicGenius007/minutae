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
}
