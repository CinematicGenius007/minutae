//! Parse the summary Markdown both older apps stored in `summaries.markdown_content`
//! (`legacy/src/lib/markdown.ts` `buildSummaryBody`, ported byte for byte by the Swift app):
//!
//! ```text
//! ## Summary
//! - bullet
//! ## Key Decisions
//! - decision
//! ## Action Items
//! - [ ] item
//! ```
//!
//! Anything else (hand-edited text, a different provider's format) still imports: the text
//! becomes the overview and the takeaway is its first line.

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ParsedSummary {
    pub takeaway: String,
    pub overview: String,
    pub decisions: Vec<String>,
    /// `(text, done)` from `- [ ]` / `- [x]` lines.
    pub action_items: Vec<(String, bool)>,
}

const TAKEAWAY_MAX: usize = 240;

fn strip_frontmatter(md: &str) -> &str {
    let t = md.trim_start();
    if let Some(rest) = t
        .strip_prefix("---\n")
        .or_else(|| t.strip_prefix("---\r\n"))
        && let Some(end) = rest.find("\n---")
    {
        let after = &rest[end + 4..];
        return after.trim_start_matches(['\r', '\n']);
    }
    md
}

fn bullet(line: &str) -> Option<&str> {
    let l = line.trim_start();
    l.strip_prefix("- ")
        .or_else(|| l.strip_prefix("* "))
        .map(str::trim)
}

fn checkbox(text: &str) -> (String, bool) {
    for (prefix, done) in [("[ ] ", false), ("[x] ", true), ("[X] ", true)] {
        if let Some(rest) = text.strip_prefix(prefix) {
            return (rest.trim().to_string(), done);
        }
    }
    (text.to_string(), false)
}

fn shorten(s: &str) -> String {
    let s = s.trim();
    if s.chars().count() <= TAKEAWAY_MAX {
        return s.to_string();
    }
    let cut: String = s.chars().take(TAKEAWAY_MAX - 1).collect();
    format!("{}…", cut.trim_end())
}

pub fn parse(markdown: &str) -> ParsedSummary {
    #[derive(PartialEq)]
    enum Sec {
        None,
        Summary,
        Decisions,
        Actions,
        Other,
    }
    let body = strip_frontmatter(markdown);
    let mut sec = Sec::None;
    let mut points = Vec::new();
    let mut prose = Vec::new();
    let mut out = ParsedSummary::default();
    for line in body.lines() {
        let t = line.trim();
        if let Some(h) = t.strip_prefix('#') {
            let h = h.trim_start_matches('#').trim().to_lowercase();
            sec = match h.as_str() {
                "summary" | "overview" => Sec::Summary,
                "key decisions" | "decisions" => Sec::Decisions,
                "action items" | "actions" | "next steps" => Sec::Actions,
                _ => Sec::Other,
            };
            continue;
        }
        if t.is_empty() {
            continue;
        }
        match (&sec, bullet(t)) {
            (Sec::Summary, Some(b)) => points.push(b.to_string()),
            (Sec::Decisions, Some(b)) => out.decisions.push(b.to_string()),
            (Sec::Actions, Some(b)) => {
                let item = checkbox(b);
                if !item.0.is_empty() {
                    out.action_items.push(item);
                }
            }
            (Sec::Summary | Sec::None | Sec::Other, None) => prose.push(t.to_string()),
            (Sec::None | Sec::Other, Some(b)) => prose.push(b.to_string()),
            _ => {}
        }
    }
    if !points.is_empty() {
        out.takeaway = shorten(&points[0]);
        out.overview = points.join(" ");
    } else if !prose.is_empty() {
        out.takeaway = shorten(&prose[0]);
        out.overview = prose.join(" ");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_legacy_body() {
        let md = "## Summary\n- Launch moves to May.\n- Perf is over budget.\n\n## Key Decisions\n- Ship in May\n\n## Action Items\n- [ ] Alice: share mockups\n- [x] Book room\n";
        let p = parse(md);
        assert_eq!(p.takeaway, "Launch moves to May.");
        assert_eq!(p.overview, "Launch moves to May. Perf is over budget.");
        assert_eq!(p.decisions, vec!["Ship in May"]);
        assert_eq!(
            p.action_items,
            vec![
                ("Alice: share mockups".to_string(), false),
                ("Book room".to_string(), true)
            ]
        );
    }

    #[test]
    fn strips_frontmatter_and_degrades_to_prose() {
        let md = "---\nid: x\ntitle: \"T\"\n---\n\nWe talked about hiring.\nNothing decided.\n";
        let p = parse(md);
        assert_eq!(p.takeaway, "We talked about hiring.");
        assert_eq!(p.overview, "We talked about hiring. Nothing decided.");
        assert!(p.decisions.is_empty());
        assert_eq!(parse("").overview, "");
    }

    #[test]
    fn long_takeaway_is_shortened() {
        let p = parse(&format!("## Summary\n- {}\n", "word ".repeat(100)));
        assert!(p.takeaway.chars().count() <= TAKEAWAY_MAX);
        assert!(p.takeaway.ends_with('…'));
    }
}
