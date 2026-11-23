//! Work out how much of the token lies to the right of the cursor.
use crate::util::Args;

#[derive(Debug, PartialEq)]
pub(crate) struct Token {
    pub(crate) prefix: String,
    pub(crate) suffix: String,
    pub(crate) right_len: usize,
    pub(crate) quote: Option<(char, usize)>,
}

fn decoded(s: &str, fish: bool) -> (String, Option<(char, usize)>, usize) {
    let mut value = String::new();
    let mut quote = None;
    let mut chars = s.char_indices().peekable();
    while let Some((pos, c)) = chars.next() {
        match (quote, c) {
            (None, c) if c.is_whitespace() || "|;()<>&".contains(c) => return (value, quote, pos),
            (None, '\'' | '"') => quote = Some((c, pos)),
            (Some((q, _)), c) if c == q => quote = None,
            (state, '\\')
                if state.is_none()
                    || chars.peek().is_some_and(|(_, next)| match state.map(|(q, _)| q) {
                        Some('\'') => fish && "\\'".contains(*next),
                        Some('"') => {
                            if fish {
                                "\\\"$".contains(*next)
                            } else {
                                "\\\"$`".contains(*next)
                            }
                        }
                        _ => false,
                    }) =>
            {
                if let Some((_, escaped)) = chars.next() {
                    if escaped != '\n' {
                        value.push(escaped);
                    }
                }
            }
            (_, c) => value.push(c),
        }
    }
    (value, quote, s.len())
}

pub(crate) fn token(left: &str, right: &str, fish: bool) -> Token {
    let (prefix, quote, _) = decoded(left, fish);
    let (word, _, end) = decoded(&format!("{left}{right}"), fish);
    Token {
        suffix: word.strip_prefix(&prefix).unwrap_or_default().into(),
        right_len: end.saturating_sub(left.len()),
        prefix,
        quote,
    }
}

/// Read a standalone editor buffer as literal words before rewriting it.
pub(crate) fn literal_words(mut text: &str, fish: bool) -> Option<Vec<String>> {
    if text.chars().any(|c| c.is_control() && c != '\t') {
        return None;
    }
    let mut words = vec![];
    loop {
        text = text.trim_start();
        if text.is_empty() {
            return Some(words);
        }
        let (word, quote, consumed) = decoded(text, fish);
        if consumed == 0 || quote.is_some() || text.starts_with('#') {
            return None;
        }
        if text[..consumed].bytes().rev().take_while(|b| *b == b'\\').count() % 2 != 0 {
            return None;
        }
        words.push(word);
        text = &text[consumed..];
    }
}

/// Read the arguments to the right of the active token in this simple command.
pub(crate) fn following_words(mut text: &str, fish: bool) -> Option<Vec<String>> {
    let mut words = vec![];
    loop {
        text = text.trim_start_matches([' ', '\t']);
        if text.is_empty() || text.starts_with(['|', ';', '&', '\n', '#']) {
            return Some(words);
        }
        let (word, quote, consumed) = decoded(text, fish);
        if consumed == 0 || quote.is_some() {
            return None;
        }
        words.push(word);
        text = &text[consumed..];
    }
}

impl Token {
    /// A provider supplies the field at the cursor. Keep other mount fields
    /// that the user already entered to its right.
    pub(crate) fn with_mount_tail<'a>(&self, candidate: &'a str) -> std::borrow::Cow<'a, str> {
        let prefix = self.prefix.strip_prefix("--mount=")
            .or_else(|| self.prefix.strip_prefix("--mount-add="))
            .unwrap_or(&self.prefix);
        let field = prefix.rsplit(',').next().and_then(|part| part.split_once('=')).map(|(key, _)| key);
        if matches!(field, Some("source" | "src" | "type")) && candidate.starts_with(&self.prefix)
            && !candidate.ends_with(&self.suffix)
        {
            if let Some((end, tail)) = self.suffix.split_once(',') {
                if candidate.ends_with(end) && tail.contains('=') {
                    return format!("{candidate},{tail}").into();
                }
            }
        }
        candidate.into()
    }

    /// A matching full candidate can supply only the missing middle. Also
    /// retain an existing value when completing the name before its '='.
    pub(crate) fn matching_prefix<'a>(&self, candidate: &'a str) -> Option<&'a str> {
        let prefix = if let Some(value) = candidate.strip_suffix(&self.suffix) {
            value
        } else if let Some((end, _)) = self.suffix.split_once('=') {
            let option = candidate.strip_suffix('=').unwrap_or(candidate);
            option.strip_suffix(end)?
        } else {
            return None;
        };
        (prefix.len() >= self.prefix.len()).then_some(prefix)
    }

    fn fish_insertion(&self, candidate: &str) -> Option<String> {
        // The fragment editor cannot quote a process token whose percent sign
        // was already typed to the left of the cursor.
        if candidate == "%self" && self.quote.is_none() && self.prefix.starts_with('%') {
            return None;
        }
        let middle = self.matching_prefix(candidate)?.strip_prefix(&self.prefix)?;
        if middle.is_empty() || middle.chars().any(char::is_control) {
            return None;
        }
        let specials = match self.quote.map(|(q, _)| q) {
            Some('\'') => "\\'",
            Some('"') => "\\\"$",
            _ => "\\\"'$*?~;|&<>()[{}]#%",
        };
        let mut out = String::new();
        for c in middle.chars() {
            if specials.contains(c) || self.quote.is_none() && c.is_whitespace() {
                out.push('\\');
            }
            out.push(c);
        }
        Some(out)
    }
}

/// Accept rows from fish's own completion engine. Emit data only. The widget
/// inserts the selected fragment and leaves the original token suffix in situ.
pub(crate) fn fish_edit(args: &[String]) -> i32 {
    use std::io::Read;
    let a = Args::parse(args, &["--left", "--right"]);
    let mut stream = std::io::stdin().lock();
    let (left, right) = if a.has("--editor-input") {
        let Some(fields) = crate::util::editor_input(&mut stream) else { return 2 };
        fields
    } else {
        (a.value("--left").unwrap_or_default(), a.value("--right").unwrap_or_default())
    };
    let t = token(&left, &right, true);
    let mut input = String::new();
    let _ = stream.take(400_000).read_to_string(&mut input);
    let mut seen = std::collections::HashSet::new();
    for row in input.lines() {
        let (value, desc) = row.split_once('\t').unwrap_or((row, ""));
        if value.chars().any(char::is_control) {
            continue;
        }
        if let Some(edit) = t.fish_insertion(&t.with_mount_tail(value)) {
            if seen.insert(edit.clone()) {
                println!("{edit}\t{value}\t{}", crate::render::clean_desc(desc));
            }
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn following_arguments_stop_at_the_current_simple_command() {
        let arguments = following_words(" ./out --profile='work' | cat --profile=other", false).unwrap();
        assert_eq!(arguments, ["./out", "--profile=work"]);
        assert!(following_words(" --profile='incomplete", false).is_none());
        assert!(following_words(" >output --profile=other", false).is_none());
    }

    #[test]
    fn fish_edits_do_not_create_process_expansions() {
        assert_eq!(token("", "", true).fish_insertion("%self").as_deref(), Some("\\%self"));
        assert_eq!(token("%s", "", true).fish_insertion("%self"), None);
        assert_eq!(token("'%s", "'", true).fish_insertion("%self").as_deref(), Some("elf"));
    }

    #[test]
    fn suffix_tracks_quoted_and_escaped_delimiters() {
        let t = token("prefix", "suffix | other", false);
        assert_eq!(t.suffix, "suffix");
        assert_eq!(t.matching_prefix("prefix-middle-suffix"), Some("prefix-middle-"));
        assert_eq!(t.matching_prefix("prefix-middle-other"), None);
        let t = token("--key=\"na", "me with space\" | wc", false);
        assert_eq!(t.quote, Some(('"', 6)));
        assert_eq!(t.suffix, "me with space");
        assert_eq!(t.matching_prefix("--key=na-any-me with space"), Some("--key=na-any-"));
        let t = token("a", "\\ b stop", false);
        assert_eq!(t.suffix, " b");
        assert_eq!(token("a", " | wc", false).right_len, 0);
        assert_eq!(token("a", "'b'c && wc", false).suffix, "bc");
    }

    #[test]
    fn preserve_an_attached_value_and_escape_fish_edits() {
        let t = token("-oPub", "Authentication=yes host", true);
        assert_eq!(t.fish_insertion("-oPubkeyAuthentication=").as_deref(), Some("key"));
        let t = token("'a", "z' next", true);
        assert_eq!(t.fish_insertion("a'qz").as_deref(), Some("\\'q"));
        let t = token("a", "z next", true);
        assert_eq!(t.fish_insertion("a qz").as_deref(), Some("\\ q"));
        assert_eq!(t.fish_insertion("az"), None);
    }
}
