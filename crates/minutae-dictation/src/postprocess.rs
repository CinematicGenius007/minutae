//! Turns raw speech-to-text output into the text that gets typed.
//!
//! Ported from the Swift `LocalCleanup` / `DictionaryReplacer`, plus spoken
//! punctuation, repeat and false-start cleanup, and fuzzy vocabulary matching.
//! Every function here is pure. The pipeline works on whitespace-separated
//! tokens; a token that starts with `\n` is a line break produced by a spoken
//! "new line" / "new paragraph" command.
//!
//! Order: normalize → fillers → false starts → repeats → spoken punctuation →
//! vocabulary → finish (pronoun "I", sentence case, end punctuation).
//! With `tidy = false` only normalize, spoken punctuation, vocabulary and
//! spacing run, so the text stays as close to the transcript as possible.

/// Hesitation sounds that are never meaningful in dictated text.
const FILLERS: &[&str] = &[
    "um", "umm", "ummm", "ummmm", "uh", "uhh", "uhhh", "uhhhh", "uhm", "uhmm", "erm",
];

/// Single words that people repeat on purpose ("very very", "no no").
const REPEAT_OK: &[&str] = &[
    "had", "that", "very", "really", "so", "no", "yes", "yeah", "bye", "ha", "haha", "knock",
];

/// A spoken command is literal text after one of these ("a comma", "the colon").
const DETERMINERS: &[&str] = &[
    "a", "an", "the", "this", "that", "each", "every", "one", "per", "of", "my", "your", "his",
    "her", "its", "our", "their", "no", "any", "another",
];

/// Words ending in "." that do not end a sentence.
const ABBREVIATIONS: &[&str] = &[
    "e.g", "i.e", "etc", "vs", "mr", "mrs", "ms", "dr", "st", "approx",
];

const FIRST_PERSON: &[(&str, &str)] = &[
    ("i", "I"),
    ("i'm", "I'm"),
    ("i'll", "I'll"),
    ("i've", "I've"),
    ("i'd", "I'd"),
];

#[derive(Clone, Copy)]
enum Command {
    /// Attach this mark to the previous word.
    Mark(char),
    /// Like `Mark`, but only when it is the last thing said ("period" is a common word).
    MarkAtEnd(char),
    /// Insert a line break.
    Break(&'static str),
}

const COMMANDS: &[(&[&str], Command)] = &[
    (&["new", "paragraph"], Command::Break("\n\n")),
    (&["new", "line"], Command::Break("\n")),
    (&["newline"], Command::Break("\n")),
    (&["full", "stop"], Command::Mark('.')),
    (&["question", "mark"], Command::Mark('?')),
    (&["exclamation", "mark"], Command::Mark('!')),
    (&["exclamation", "point"], Command::Mark('!')),
    (&["semicolon"], Command::Mark(';')),
    (&["colon"], Command::Mark(':')),
    (&["comma"], Command::Mark(',')),
    (&["period"], Command::MarkAtEnd('.')),
];

/// Full pipeline. `tidy` mirrors `AppSettings::tidy_dictation`.
pub fn process(raw: &str, tidy: bool, vocabulary: &[String]) -> String {
    let mut tokens = normalize(raw);
    if tidy {
        tokens = remove_fillers(tokens);
        tokens = remove_false_starts(tokens);
        tokens = collapse_repeats(tokens);
    }
    tokens = apply_spoken_punctuation(tokens);
    tokens = apply_vocabulary(tokens, vocabulary);
    if tidy {
        tokens = fix_first_person(tokens);
        tokens = sentence_case(tokens);
        tokens = end_punctuation(tokens);
    }
    let text = join(&trim_breaks(tokens));
    if text.chars().any(char::is_alphanumeric) {
        text
    } else {
        String::new()
    }
}

/// Words in the final text, as shown in the HUD ("Inserted · 23 words").
pub fn word_count(text: &str) -> u32 {
    text.split_whitespace()
        .filter(|t| t.chars().any(char::is_alphanumeric))
        .count() as u32
}

// ---------------------------------------------------------------------------
// Tokens

fn is_break(token: &str) -> bool {
    token.starts_with('\n')
}

/// Splits a token into leading punctuation, the word, and trailing punctuation.
/// The word runs from the first to the last alphanumeric character, so inner
/// apostrophes and hyphens ("don't", "i.e") stay in it.
fn affixes(token: &str) -> (&str, &str, &str) {
    let Some(start) = token
        .char_indices()
        .find(|(_, c)| c.is_alphanumeric())
        .map(|(i, _)| i)
    else {
        return (token, "", "");
    };
    let end = token
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_alphanumeric())
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(token.len());
    (&token[..start], &token[start..end], &token[end..])
}

fn key(token: &str) -> String {
    affixes(token).1.to_lowercase()
}

/// Lowercase alphanumerics only: "Swift-UI" and "swift ui" both become "swiftui".
fn squash(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn is_tight_punctuation(token: &str) -> bool {
    !token.is_empty() && token.chars().all(|c| ".,;:!?…".contains(c))
}

fn ends_sentence(token: &str) -> bool {
    affixes(token)
        .2
        .chars()
        .any(|c| matches!(c, '.' | '!' | '?' | '…'))
}

/// Straight apostrophes, no `[BLANK_AUDIO]`-style annotations, split on whitespace.
fn normalize(raw: &str) -> Vec<String> {
    let mut text = raw.replace(['\u{2019}', '\u{2018}'], "'");
    while let (Some(open), Some(close)) = (text.find('['), text.find(']')) {
        if close < open {
            break;
        }
        text.replace_range(open..=close, " ");
    }
    text.split_whitespace().map(str::to_owned).collect()
}

fn join(tokens: &[String]) -> String {
    let mut out = String::new();
    for token in tokens {
        if is_break(token) {
            while out.ends_with(' ') {
                out.pop();
            }
            out.push_str(token);
        } else if out.is_empty() || out.ends_with('\n') || is_tight_punctuation(token) {
            out.push_str(token);
        } else {
            out.push(' ');
            out.push_str(token);
        }
    }
    out
}

fn trim_breaks(mut tokens: Vec<String>) -> Vec<String> {
    while tokens.last().is_some_and(|t| is_break(t)) {
        tokens.pop();
    }
    let lead = tokens.iter().take_while(|t| is_break(t)).count();
    tokens.drain(..lead);
    tokens
}

fn strip_trailing_marks(token: &mut String) {
    while token.ends_with(|c: char| ".,;:!?".contains(c)) {
        token.pop();
    }
}

// ---------------------------------------------------------------------------
// Cleanup passes

/// Drops hesitation sounds, and "like" when it is set off by commas
/// ("it was, like, huge" / "Like, I think"). "I like it" is untouched.
fn remove_fillers(tokens: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(tokens.len());
    for token in tokens {
        let (_, core, trail) = affixes(&token);
        let word = core.to_lowercase();
        if FILLERS.contains(&word.as_str()) {
            // "so that's it um." keeps its full stop.
            if let Some(mark) = trail.chars().find(|c| matches!(c, '.' | '!' | '?'))
                && let Some(prev) = out.last_mut()
                && !is_break(prev)
            {
                strip_trailing_marks(prev);
                prev.push(mark);
            }
            continue;
        }
        if word == "like" && trail == "," {
            let at_start = out.last().is_none_or(|p| is_break(p) || ends_sentence(p));
            let after_comma = out.last().is_some_and(|p| p.ends_with(','));
            if at_start || after_comma {
                if after_comma && let Some(prev) = out.last_mut() {
                    prev.pop();
                }
                continue;
            }
        }
        out.push(token);
    }
    out
}

/// Drops a cut-off word: "go to the sto- store" → "go to the store".
fn remove_false_starts(tokens: Vec<String>) -> Vec<String> {
    let count = tokens.len();
    tokens
        .into_iter()
        .enumerate()
        .filter(|(i, t)| {
            let (_, core, trail) = affixes(t);
            let cut = !core.is_empty() && (trail.ends_with('-') || trail.ends_with('\u{2014}'));
            !(cut && i + 1 < count)
        })
        .map(|(_, t)| t)
        .collect()
}

/// Collapses an immediate repeat of one to three words: "the the" → "the",
/// "I want I want to" → "I want to". A repeat after punctuation ("No, no")
/// is deliberate and stays.
fn collapse_repeats(mut tokens: Vec<String>) -> Vec<String> {
    let mut i = 0;
    while i < tokens.len() {
        let mut collapsed = false;
        for n in (1..=3).rev() {
            if i + 2 * n > tokens.len() {
                continue;
            }
            let (first, second) = (&tokens[i..i + n], &tokens[i + n..i + 2 * n]);
            if first.iter().chain(second).any(|t| is_break(t)) {
                continue;
            }
            if first.iter().any(|t| !affixes(t).2.is_empty()) {
                continue;
            }
            let same = first.iter().zip(second).all(|(a, b)| {
                let k = key(a);
                !k.is_empty() && k == key(b)
            });
            if !same || (n == 1 && REPEAT_OK.contains(&key(&first[0]).as_str())) {
                continue;
            }
            tokens.drain(i..i + n);
            collapsed = true;
            break;
        }
        if !collapsed {
            i += 1;
        }
    }
    tokens
}

fn match_command(tokens: &[String], at: usize) -> Option<(usize, Command)> {
    COMMANDS.iter().find_map(|(words, command)| {
        let end = at + words.len();
        if end > tokens.len() {
            return None;
        }
        let window = &tokens[at..end];
        let hit = window
            .iter()
            .zip(words.iter())
            .all(|(t, w)| !is_break(t) && key(t) == *w);
        // "new, line" is two separate words, not a command.
        let joined = window[..window.len() - 1]
            .iter()
            .all(|t| affixes(t).2.is_empty());
        (hit && joined).then_some((words.len(), *command))
    })
}

/// "hello comma world full stop" → "hello, world."; "new line" → line break.
fn apply_spoken_punctuation(tokens: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        let after_determiner = out.last().is_some_and(|p| {
            !is_break(p) && affixes(p).2.is_empty() && DETERMINERS.contains(&key(p).as_str())
        });
        let found = if after_determiner {
            None
        } else {
            match_command(&tokens, i)
        };
        let Some((len, command)) = found else {
            out.push(tokens[i].clone());
            i += 1;
            continue;
        };
        let next = i + len;
        let at_end = next == tokens.len()
            || is_break(&tokens[next])
            || match_command(&tokens, next).is_some();
        match command {
            Command::Break(text) => out.push(text.to_owned()),
            Command::MarkAtEnd(_) if !at_end => {
                out.push(tokens[i].clone());
                i += 1;
                continue;
            }
            Command::Mark(mark) | Command::MarkAtEnd(mark) => {
                if let Some(prev) = out.last_mut().filter(|p| !is_break(p)) {
                    strip_trailing_marks(prev);
                    prev.push(mark);
                }
            }
        }
        i = next;
    }
    out
}

/// Character-level edit distance.
fn levenshtein(a: &[char], b: &[char]) -> usize {
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (diagonal + usize::from(ca != cb))
                .min(row[j] + 1)
                .min(above + 1);
            diagonal = above;
        }
    }
    row[b.len()]
}

/// Similarity in 0..=1 for a near-miss spelling, or `None` when the two
/// should never be treated as the same word.
///
/// Conservative on purpose: both at least 5 characters, same first letter,
/// at most 2 edits, at least 80 % similar, and the heard word is not shorter
/// than the term (so "minute" never becomes "Minutae", but "minutiae" does).
fn fuzzy_score(heard: &str, term: &str) -> Option<f32> {
    let (h, t): (Vec<char>, Vec<char>) = (heard.chars().collect(), term.chars().collect());
    if h.len() < 5 || t.len() < 5 || h[0] != t[0] || h.len() < t.len() || h.len() > t.len() + 2 {
        return None;
    }
    let distance = levenshtein(&h, &t);
    let score = 1.0 - distance as f32 / h.len().max(t.len()) as f32;
    (distance <= 2 && score >= 0.8).then_some(score)
}

/// Rewrites words that match a vocabulary term, case-insensitively and across
/// spacing ("swift ui" → "SwiftUI"), or as a close misspelling ("minutiae" →
/// "Minutae"). Exact matches beat fuzzy ones; longer spans beat shorter ones.
fn apply_vocabulary(tokens: Vec<String>, vocabulary: &[String]) -> Vec<String> {
    let terms: Vec<(&str, String, usize)> = vocabulary
        .iter()
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .map(|t| (t, squash(t), t.split_whitespace().count()))
        .filter(|(_, squashed, _)| !squashed.is_empty())
        .collect();
    if terms.is_empty() {
        return tokens;
    }
    let max_window = terms
        .iter()
        .map(|(_, _, words)| words + 1)
        .max()
        .unwrap_or(1)
        .min(5);

    let mut out = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        let mut exact: Option<(usize, &str)> = None;
        let mut fuzzy: Option<(f32, usize, &str)> = None;
        for width in (1..=max_window).rev() {
            if i + width > tokens.len() {
                continue;
            }
            let window = &tokens[i..i + width];
            if window
                .iter()
                .any(|t| is_break(t) || affixes(t).1.is_empty())
            {
                continue;
            }
            // Never join words across a comma or full stop.
            if window[..width - 1].iter().any(|t| !affixes(t).2.is_empty()) {
                continue;
            }
            let heard: String = window
                .iter()
                .map(|t| squash(possessive_stem(affixes(t).1)))
                .collect();
            for (term, squashed, words) in &terms {
                if heard == *squashed {
                    exact = exact.or(Some((width, *term)));
                } else if width <= words + 1
                    && let Some(score) = fuzzy_score(&heard, squashed)
                    && fuzzy.is_none_or(|(best, _, _)| score > best)
                {
                    fuzzy = Some((score, width, *term));
                }
            }
            if exact.is_some() {
                break;
            }
        }
        let Some((width, term)) = exact.or(fuzzy.map(|(_, w, t)| (w, t))) else {
            out.push(tokens[i].clone());
            i += 1;
            continue;
        };
        let (lead, _, _) = affixes(&tokens[i]);
        let (_, last_core, trail) = affixes(&tokens[i + width - 1]);
        let suffix = &last_core[possessive_stem(last_core).len()..];
        out.push(format!("{lead}{term}{suffix}{trail}"));
        i += width;
    }
    out
}

/// "minutae's" → "minutae". The suffix is put back after the replacement.
fn possessive_stem(core: &str) -> &str {
    core.strip_suffix("'s")
        .or_else(|| core.strip_suffix("'S"))
        .unwrap_or(core)
}

// ---------------------------------------------------------------------------
// Finish

/// "i" → "I" (and i'm, i'll, i've, i'd). "i.e." is left alone.
fn fix_first_person(tokens: Vec<String>) -> Vec<String> {
    tokens
        .into_iter()
        .map(|token| {
            let (lead, core, trail) = affixes(&token);
            match FIRST_PERSON
                .iter()
                .find(|(from, _)| *from == core.to_lowercase())
            {
                Some((_, to)) => format!("{lead}{to}{trail}"),
                None => token,
            }
        })
        .collect()
}

/// Capitalises the first word of each sentence when that word is entirely
/// lowercase, so "iPhone" and "macOS" keep their spelling.
fn sentence_case(tokens: Vec<String>) -> Vec<String> {
    let mut start = true;
    tokens
        .into_iter()
        .map(|token| {
            if is_break(&token) {
                start = true;
                return token;
            }
            let (lead, core, trail) = affixes(&token);
            if core.is_empty() {
                return token;
            }
            let mut result = token.clone();
            if start && !core.chars().any(char::is_uppercase) {
                let mut chars = core.chars();
                if let Some(first) = chars.next().filter(|c| c.is_alphabetic()) {
                    result = format!("{lead}{}{}{trail}", first.to_uppercase(), chars.as_str());
                }
            }
            let abbreviation =
                trail.starts_with('.') && ABBREVIATIONS.contains(&core.to_lowercase().as_str());
            start = ends_sentence(&token) && !abbreviation;
            result
        })
        .collect()
}

/// Ends the text with a full stop unless it already ends with a mark.
/// A dangling comma or semicolon becomes a full stop.
fn end_punctuation(mut tokens: Vec<String>) -> Vec<String> {
    let Some(last) = tokens.iter_mut().rev().find(|t| !is_break(t)) else {
        return tokens;
    };
    let (lead, core, trail) = affixes(last);
    if core.is_empty() {
        return tokens;
    }
    let closers: String = trail
        .chars()
        .rev()
        .take_while(|c| "\"')]}\u{201D}\u{2019}".contains(*c))
        .collect();
    let closers: String = closers.chars().rev().collect();
    let marks = &trail[..trail.len() - closers.len()];
    let mark = match marks.chars().last() {
        Some('.' | '!' | '?' | '…' | ':') => return tokens,
        Some(',' | ';') => format!("{}.", &marks[..marks.len() - 1]),
        _ => format!("{marks}."),
    };
    *last = format!("{lead}{core}{mark}{closers}");
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tidy(raw: &str) -> String {
        process(raw, true, &[])
    }

    fn vocab(raw: &str, terms: &[&str]) -> String {
        let terms: Vec<String> = terms.iter().map(|s| s.to_string()).collect();
        process(raw, true, &terms)
    }

    #[test]
    fn fillers() {
        let cases = [
            (
                "um so I think we should ship it",
                "So I think we should ship it.",
            ),
            (
                "Um, so I think, uh, we should ship it.",
                "So I think, we should ship it.",
            ),
            ("I was um going home", "I was going home."),
            ("that's it um.", "That's it."),
            ("it was, like, huge", "It was huge."),
            ("Like, I think so", "I think so."),
            ("I like pizza", "I like pizza."),
            ("things I like, such as tea", "Things I like, such as tea."),
            ("uhh umm", ""),
            ("drum and summary", "Drum and summary."),
        ];
        for (input, expected) in cases {
            assert_eq!(tidy(input), expected, "input: {input:?}");
        }
    }

    #[test]
    fn repeats_and_false_starts() {
        let cases = [
            ("the the cat sat", "The cat sat."),
            ("the the the cat", "The cat."),
            ("I want I want to go", "I want to go."),
            ("go to the sto- store", "Go to the store."),
            ("I want-- I want to go", "I want to go."),
            ("we need to to to fix this", "We need to fix this."),
            ("very very good", "Very very good."),
            ("I knew that that was wrong", "I knew that that was wrong."),
            ("No, no, that's fine", "No, no, that's fine."),
            ("ends with a dash-", "Ends with a dash-."),
        ];
        for (input, expected) in cases {
            assert_eq!(tidy(input), expected, "input: {input:?}");
        }
    }

    #[test]
    fn spoken_punctuation() {
        let cases = [
            ("hello comma world full stop", "Hello, world."),
            ("is it ready question mark", "Is it ready?"),
            ("wow exclamation point", "Wow!"),
            (
                "first line new line second line",
                "First line\nSecond line.",
            ),
            ("dear team new paragraph thanks", "Dear team\n\nThanks."),
            ("add a comma here", "Add a comma here."),
            ("the trial period ended", "The trial period ended."),
            ("that is all period", "That is all."),
            ("Hello, comma, world.", "Hello, world."),
            ("note colon buy milk", "Note: buy milk."),
            ("comma at start", "At start."),
            ("we said new, line twice", "We said new, line twice."),
        ];
        for (input, expected) in cases {
            assert_eq!(tidy(input), expected, "input: {input:?}");
        }
    }

    #[test]
    fn capitalisation_and_pronoun() {
        let cases = [
            ("i think i'm right", "I think I'm right."),
            ("use i.e. this one", "Use i.e. this one."),
            (
                "first. second? third! fourth",
                "First. Second? Third! Fourth.",
            ),
            ("iPhone is great", "iPhone is great."),
            ("see e.g. this case", "See e.g. this case."),
            ("macOS ships today", "macOS ships today."),
            ("  spaced   out  ", "Spaced out."),
            ("it ends with a comma,", "It ends with a comma."),
            ("he said \"hello\"", "He said \"hello.\""),
            ("[BLANK_AUDIO]", ""),
            ("ok [MUSIC] go", "Ok go."),
            ("it\u{2019}s fine", "It's fine."),
            ("42 is the answer", "42 is the answer."),
            ("", ""),
        ];
        for (input, expected) in cases {
            assert_eq!(tidy(input), expected, "input: {input:?}");
        }
    }

    #[test]
    fn vocabulary_correction() {
        let terms = [
            "Minutae",
            "SwiftUI",
            "Node.js",
            "Kubernetes",
            "GitHub Actions",
        ];
        let cases = [
            ("I use minutae daily", "I use Minutae daily."),
            ("built with swift ui.", "Built with SwiftUI."),
            (
                "built with Swift UI and node js",
                "Built with SwiftUI and Node.js.",
            ),
            ("open minutiae now", "Open Minutae now."),
            ("wait a minute please", "Wait a minute please."),
            ("deploy to kubernetis", "Deploy to Kubernetes."),
            ("runs in github actions", "Runs in GitHub Actions."),
            ("minutae's summary", "Minutae's summary."),
            ("swift, ui", "Swift, ui."),
        ];
        for (input, expected) in cases {
            assert_eq!(vocab(input, &terms), expected, "input: {input:?}");
        }
    }

    #[test]
    fn untidy_keeps_transcript_shape() {
        let terms = vec!["SwiftUI".to_string()];
        assert_eq!(
            process("um the the swift ui comma ok", false, &terms),
            "um the the SwiftUI, ok"
        );
        assert_eq!(process("  hello   world ", false, &[]), "hello world");
    }

    #[test]
    fn counts_words() {
        assert_eq!(word_count("Hello, world. It's me — again!"), 5);
        assert_eq!(word_count(""), 0);
        assert_eq!(word_count("one\ntwo"), 2);
    }

    #[test]
    fn edit_distance() {
        let d = |a: &str, b: &str| {
            levenshtein(
                &a.chars().collect::<Vec<_>>(),
                &b.chars().collect::<Vec<_>>(),
            )
        };
        assert_eq!(d("kitten", "sitting"), 3);
        assert_eq!(d("", "abc"), 3);
        assert_eq!(d("same", "same"), 0);
        assert!(fuzzy_score("minute", "minutae").is_none());
        assert!(fuzzy_score("minutiae", "minutae").is_some());
    }
}
