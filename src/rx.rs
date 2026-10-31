//! Regex help: flavour detection, a TAB palette, plain-English explanations, a live builder.
//!
//! Flavours
//!   bre    grep, sed (no -E): ( ) { } | + ? are LITERAL unless backslashed (GNU)
//!   ere    grep -E, egrep, sed -E, awk
//!   pcre   grep -P, rg -P / --pcre2, fish `string match -r`
//!   rust   rg default engine (no look-around, no back-references)
//!   emacs  find -regex default (GNU findutils)
//!   fixed  grep -F, rg -F: no regex at all

use crate::re;
use crate::util::*;

pub const FLAVORS: &[(&str, &str)] = &[
    ("bre", "basic regex (grep / sed default)"),
    ("ere", "extended regex (grep -E, sed -E, awk)"),
    ("pcre", "PCRE (grep -P, rg -P, fish string -r)"),
    ("rust", "Rust regex (rg default)"),
    ("emacs", "Emacs regex (find -regex default)"),
    ("python", "Python re"),
    ("fixed", "fixed string (no regex)"),
];

fn flavor_name(f: &str) -> &str {
    FLAVORS.iter().find(|(k, _)| *k == f).map(|(_, v)| *v).unwrap_or(f)
}
fn modern(f: &str) -> bool {
    matches!(f, "pcre" | "rust" | "python")
}
fn gnu_posix(f: &str) -> bool {
    matches!(f, "bre" | "ere" | "emacs")
}

// ------------------------------------------------------------ flavour detection

fn basename(s: &str) -> &str {
    s.rsplit('/').next().unwrap_or(s)
}

pub fn detect(tokens: &[String]) -> (String, String) {
    if tokens.is_empty() {
        return (String::new(), "ere".into());
    }
    let tool = basename(&tokens[0]).to_string();
    let rest = &tokens[1..];
    let longs: Vec<&str> = rest.iter().filter(|t| t.starts_with("--")).map(|t| t.split('=').next().unwrap_or("")).collect();
    let shorts: Vec<char> = rest.iter().filter(|t| t.starts_with('-') && !t.starts_with("--")).flat_map(|t| t.chars().skip(1)).collect();
    let l = |x: &str| longs.contains(&x);
    let s = |c: char| shorts.contains(&c);
    let r = |f: &str| (tool.clone(), f.to_string());
    match tool.as_str() {
        "string" => r("pcre"),
        "grep" | "egrep" | "fgrep" | "rgrep" | "zgrep" | "ugrep" | "ug" => {
            if tool == "fgrep" || s('F') || l("--fixed-strings") {
                r("fixed")
            } else if s('P') || l("--perl-regexp") {
                r("pcre")
            } else if matches!(tool.as_str(), "egrep" | "ugrep" | "ug") || s('E') || l("--extended-regexp") {
                r("ere")
            } else {
                r("bre")
            }
        }
        "rg" => {
            if s('F') || l("--fixed-strings") {
                r("fixed")
            } else if s('P') || l("--pcre2") || rest.iter().any(|t| t.starts_with("--engine=pcre2")) {
                r("pcre")
            } else {
                r("rust")
            }
        }
        "sed" | "gsed" => r(if s('E') || s('r') || l("--regexp-extended") { "ere" } else { "bre" }),
        "awk" | "gawk" | "mawk" => r("ere"),
        "find" => {
            for i in 0..rest.len() {
                if rest[i] == "-regextype" && i + 1 < rest.len() {
                    return r(match rest[i + 1].as_str() {
                        "posix-extended" | "egrep" | "posix-egrep" => "ere",
                        "posix-basic" | "grep" => "bre",
                        _ => "emacs",
                    });
                }
            }
            r("emacs")
        }
        "python" | "python3" => r("python"),
        _ => r("ere"),
    }
}

// ------------------------------------------------------------ explanation

fn escape_info(c: char) -> Option<(&'static str, &'static [&'static str])> {
    const M: &[&str] = &["pcre", "rust", "python"];
    const MG: &[&str] = &["pcre", "rust", "python", "bre", "ere", "emacs"];
    const G: &[&str] = &["bre", "ere", "emacs"];
    Some(match c {
        'd' => ("a digit 0-9", M),
        'D' => ("anything except a digit", M),
        'w' => ("a word character: letter, digit or _", MG),
        'W' => ("anything except a word character", MG),
        's' => ("whitespace: space, tab, newline…", MG),
        'S' => ("anything except whitespace", MG),
        'b' => ("word boundary (edge between \\w and \\W), matches no characters", MG),
        'B' => ("NOT a word boundary", MG),
        '<' => ("start of a word (GNU)", G),
        '>' => ("end of a word (GNU)", G),
        'A' => ("start of the whole input", M),
        'z' => ("end of the whole input", M),
        'Z' => ("end of input (before a final newline)", &["pcre", "python"]),
        'K' => ("forget everything matched so far: the match 'starts' here", &["pcre"]),
        'G' => ("where the previous match ended", &["pcre"]),
        'n' => ("newline", M),
        't' => ("tab", M),
        'r' => ("carriage return", M),
        'h' => ("horizontal whitespace (space or tab)", &["pcre"]),
        'R' => ("any line break (\\n, \\r\\n, …)", &["pcre"]),
        '`' => ("start of the buffer (GNU)", G),
        '\'' => ("end of the buffer (GNU)", G),
        _ => return None,
    })
}

fn suggest(c: char) -> Option<&'static str> {
    Some(match c {
        'd' => "use [0-9] or [[:digit:]], or switch to grep -P",
        'D' => "use [^0-9]",
        'A' => "use ^",
        'z' => "use $",
        'n' => "patterns are matched per line",
        't' => "use [[:blank:]]",
        'K' => "only PCRE has \\K: grep -P or rg -P",
        '<' | '>' => "use \\b",
        _ => return None,
    })
}

fn posix_class(n: &str) -> Option<&'static str> {
    Some(match n {
        "alpha" => "a letter",
        "digit" => "a digit",
        "alnum" => "a letter or digit",
        "upper" => "an uppercase letter",
        "lower" => "a lowercase letter",
        "space" => "whitespace",
        "blank" => "space or tab",
        "punct" => "punctuation",
        "xdigit" => "a hex digit",
        "cntrl" => "a control character",
        "print" => "a printable character",
        "graph" => "a visible character",
        "word" => "a word character",
        _ => return None,
    })
}

/// (bare, escaped): metacharacters special bare vs. only when backslashed.
fn special(flavor: &str) -> (&'static str, &'static str) {
    match flavor {
        "bre" => (".*^$[", "(){}|+?"),
        "emacs" => (".*^$[+?", "(){}|"),
        _ => (".*^$[(){}|+?", ""),
    }
}

/// Python repr() of a one-character string.
fn py_repr_char(c: char) -> String {
    match c {
        '\'' => "\"'\"".into(),
        '\\' => "'\\\\'".into(),
        '\n' => "'\\n'".into(),
        '\t' => "'\\t'".into(),
        c => format!("'{c}'"),
    }
}

fn describe_class(body: &[char], negated: bool) -> String {
    let mut parts: Vec<String> = vec![];
    let mut i = 0;
    let s: String = body.iter().collect();
    while i < body.len() {
        let rest: String = body[i..].iter().collect();
        if let Some(m) = find(re!(r"^\[:(\w+):\]"), &rest) {
            let whole = m.get(0).unwrap().as_str();
            parts.push(posix_class(group(&m, 1)).map(String::from).unwrap_or_else(|| whole.to_string()));
            i += whole.chars().count();
            continue;
        }
        if body[i] == '\\' && i + 1 < body.len() {
            let e = body[i + 1];
            parts.push(escape_info(e).map(|x| x.0.to_string()).unwrap_or_else(|| format!("'{e}'")));
            i += 2;
            continue;
        }
        if i + 2 < body.len() && body[i + 1] == '-' {
            parts.push(format!("{}-{}", body[i], body[i + 2]));
            i += 3;
            continue;
        }
        parts.push(if body[i] != ' ' { py_repr_char(body[i]) } else { "space".into() });
        i += 1;
    }
    let _ = s;
    let lead = if negated { "any ONE character EXCEPT" } else { "any ONE of" };
    format!("{lead}: {}", parts.join(", "))
}

fn quantifier_text(q: &str, lazy: bool) -> String {
    let t = match q {
        "*" => "the previous item, 0 or more times".to_string(),
        "+" => "the previous item, 1 or more times".to_string(),
        "?" => "the previous item is optional (0 or 1 time)".to_string(),
        _ => match find(re!(r"^\{(\d*)(,?)(\d*)\}$"), q) {
            Some(m) => {
                let (lo, comma, hi) = (group(&m, 1), group(&m, 2), group(&m, 3));
                let lo0 = if lo.is_empty() { "0" } else { lo };
                if comma.is_empty() {
                    format!("the previous item exactly {lo} times")
                } else if hi.is_empty() {
                    format!("the previous item {lo0} or more times")
                } else {
                    format!("the previous item {lo0} to {hi} times")
                }
            }
            None => q.to_string(),
        },
    };
    t + if lazy { " — as FEW as possible (lazy)" } else { " (as many as possible)" }
}

pub type Row = (usize, String, String, String);

struct Ex {
    rows: Vec<Row>,
    depth: usize,
    lit: String,
}

impl Ex {
    fn flush(&mut self) {
        if !self.lit.is_empty() {
            let lit = std::mem::take(&mut self.lit);
            self.rows.push((self.depth, lit.clone(), format!("the literal text '{lit}'"), String::new()));
        }
    }
    fn add(&mut self, tok: &str, meaning: &str, warn: &str) {
        self.flush();
        self.rows.push((self.depth, tok.into(), meaning.into(), warn.into()));
    }
}

fn starts_at(p: &[char], i: usize, s: &str) -> bool {
    let s: Vec<char> = s.chars().collect();
    i + s.len() <= p.len() && p[i..i + s.len()] == s[..]
}

/// Return rows of (depth, token, meaning, warning).
pub fn explain(pattern: &str, flavor: &str) -> Vec<Row> {
    if flavor == "fixed" {
        return vec![(0, pattern.into(), "matched literally: this tool is in fixed-string mode (-F)".into(), String::new())];
    }
    let (bare, escaped) = special(flavor);
    let p: Vec<char> = pattern.chars().collect();
    let n = p.len();
    let mut ex = Ex { rows: vec![], depth: 0, lit: String::new() };
    let mut group_no = 0;
    let mut i = 0;
    let fname = flavor_name(flavor).to_string();
    while i < n {
        let mut c = p[i];
        let nxt = p.get(i + 1).copied();
        if let Some(nx) = nxt.filter(|_| c == '\\') {
            if escaped.contains(nx) {
                c = nx;
                i += 2; // BRE \( etc: treat as the metachar
            } else if nx.is_ascii_digit() && nx != '0' {
                let warn = if flavor == "rust" { "back-references need rg -P" } else { "" };
                ex.add(&format!("\\{nx}"), &format!("the same text that group {nx} matched (back-reference)"), warn);
                i += 2;
                continue;
            } else if let Some((meaning, ok)) = escape_info(nx) {
                let mut warn = String::new();
                if !ok.contains(&flavor) {
                    warn = format!("not supported in {fname}");
                    if let Some(sg) = suggest(nx) {
                        warn += &format!("; {sg}");
                    }
                }
                ex.add(&format!("\\{nx}"), meaning, &warn);
                i += 2;
                continue;
            } else if (nx == 'p' || nx == 'P') && modern(flavor) {
                let rest: String = p[i..].iter().collect();
                let tok = match find(re!(r"^\\[pP](\{[^}]*\}|\w)"), &rest) {
                    Some(m) => m.get(0).unwrap().as_str().to_string(),
                    None => p[i..(i + 2).min(n)].iter().collect(),
                };
                let cls: String = tok.chars().skip(2).collect::<String>().trim_matches(|c| c == '{' || c == '}').to_string();
                let not = if nx == 'P' { "NOT " } else { "" };
                ex.add(&tok, &format!("{not}a character in Unicode class {cls}"), "");
                i += tok.chars().count();
                continue;
            } else if nx == 'x' && modern(flavor) {
                let rest: String = p[i..].iter().collect();
                let tok = match find(re!(r"^\\x(\{[0-9a-fA-F]+\}|[0-9a-fA-F]{2})"), &rest) {
                    Some(m) => m.get(0).unwrap().as_str().to_string(),
                    None => "\\x".into(),
                };
                let code: String = tok.chars().skip(2).collect::<String>().trim_matches(|c| c == '{' || c == '}').to_string();
                ex.add(&tok, &format!("the character with hex code {code}"), "");
                i += tok.chars().count();
                continue;
            } else {
                i += 2;
                if ".*^$[]()|+?{}\\/".contains(nx) {
                    ex.add(&format!("\\{nx}"), &format!("a literal '{nx}' (escaped)"), "");
                } else {
                    ex.lit.push(nx);
                }
                continue;
            }
        } else if escaped.contains(c) && (flavor == "bre" || flavor == "emacs") {
            ex.add(&c.to_string(), &format!("a literal '{c}'"), &format!("in {fname} a bare {c} is literal; write \\{c} or use -E"));
            i += 1;
            continue;
        } else if !bare.contains(c) {
            ex.lit.push(c);
            i += 1;
            continue;
        } else {
            i += 1;
        }

        // c is an active metacharacter here (bare, or BRE-escaped)
        match c {
            '.' => ex.add(".", "any single character (except newline)", ""),
            '^' => {
                let m = if ex.rows.is_empty() && ex.lit.is_empty() { "start of the line" } else { "start of line (only an anchor at the start)" };
                ex.add("^", m, "");
            }
            '$' => ex.add("$", "end of the line", ""),
            '[' => {
                let mut j = i;
                if j < n && p[j] == '^' {
                    j += 1;
                }
                if j < n && p[j] == ']' {
                    j += 1;
                }
                while j < n && p[j] != ']' {
                    if starts_at(&p, j, "[:") {
                        let mut k = j + 2;
                        let mut found = None;
                        while k + 1 < n {
                            if p[k] == ':' && p[k + 1] == ']' {
                                found = Some(k);
                                break;
                            }
                            k += 1;
                        }
                        j = match found { Some(k) => k + 2, None => j + 1 };
                        continue;
                    }
                    if p[j] == '\\' && modern(flavor) {
                        j += 1;
                    }
                    j += 1;
                }
                let j_end = j.min(n);
                let body: Vec<char> = p[i..j_end].to_vec();
                let negated = body.first() == Some(&'^');
                let shown: String = body.iter().collect();
                let desc = describe_class(if negated { &body[1..] } else { &body[..] }, negated);
                ex.add(&format!("[{shown}]"), &desc, if j < n { "" } else { "unclosed [ — add ]" });
                i = j + 1;
            }
            '*' | '+' | '?' | '{' => {
                let mut tok = c.to_string();
                if c == '{' {
                    if flavor == "bre" {
                        let rest: String = p[i..].iter().collect();
                        match find(re!(r"^(\d*,?\d*)\\\}"), &rest) {
                            Some(m2) => {
                                tok = format!("{{{}}}", group(&m2, 1));
                                i += m2.get(0).unwrap().as_str().chars().count();
                            }
                            None => {
                                ex.add("\\{", "a literal '{'", "");
                                continue;
                            }
                        }
                    } else {
                        let rest: String = p[i - 1..].iter().collect();
                        match find(re!(r"^\{\d*,?\d*\}"), &rest) {
                            Some(m) => {
                                tok = m.get(0).unwrap().as_str().to_string();
                                i += tok.chars().count() - 1;
                            }
                            None => {
                                ex.lit.push('{');
                                continue;
                            }
                        }
                    }
                }
                let lazy = modern(flavor) && i < n && p[i] == '?';
                if lazy {
                    i += 1;
                }
                let poss = flavor == "pcre" && i < n && p[i] == '+';
                if poss {
                    i += 1;
                }
                let mut disp = if flavor == "bre" && (c == '+' || c == '?') { format!("\\{tok}") } else { tok.clone() };
                if lazy {
                    disp.push('?');
                }
                if poss {
                    disp.push('+');
                }
                let mut warn = if !ex.rows.is_empty() || !ex.lit.is_empty() { "" } else { "nothing to repeat" }.to_string();
                if gnu_posix(flavor) && i < n && p[i] == '?' && !lazy {
                    warn = "lazy quantifiers (*?, +?) need PCRE: grep -P / rg".into();
                }
                let extra = if poss { " without backtracking (possessive)" } else { "" };
                ex.add(&disp, &(quantifier_text(&tok, lazy) + extra), &warn);
            }
            '|' => ex.add(if flavor != "bre" { "|" } else { "\\|" }, "OR: either the part before or the part after", ""),
            '(' => {
                let rest: String = p[i..].iter().collect();
                let m = find(re!(r"^\?(:|=|!|<=|<!|P?<(\w+)>|'(\w+)'|[imsxU-]+:|[imsxU-]+\))"), &rest);
                match m {
                    Some(m) if modern(flavor) => {
                        let kind = group(&m, 1).to_string();
                        i += m.get(0).unwrap().as_str().chars().count();
                        if kind.ends_with(')') {
                            let flags = &kind[..kind.len() - 1];
                            ex.add(&format!("(?{kind}"), &format!("turn on flags {flags} from here (i = ignore case, s = . matches newline, m = ^/$ per line, x = verbose)"), "");
                            continue;
                        }
                        let label = match kind.as_str() {
                            ":" => Some("group (not captured)"),
                            "=" => Some("LOOKAHEAD: must be followed by …"),
                            "!" => Some("NEGATIVE LOOKAHEAD: must NOT be followed by …"),
                            "<=" => Some("LOOKBEHIND: must be preceded by …"),
                            "<!" => Some("NEGATIVE LOOKBEHIND: must NOT be preceded by …"),
                            _ => None,
                        };
                        let warn = if flavor == "rust" && matches!(kind.as_str(), "=" | "!" | "<=" | "<!") { "look-around needs rg -P (PCRE2)" } else { "" };
                        if let Some(l) = label {
                            ex.add(&format!("(?{kind}"), l, warn);
                        } else if kind.ends_with(':') {
                            ex.add(&format!("(?{kind}"), &format!("group with flags {} (i = ignore case)", &kind[..kind.len() - 1]), "");
                        } else {
                            group_no += 1;
                            let nm = if !group(&m, 2).is_empty() { group(&m, 2) } else { group(&m, 3) };
                            ex.add(&format!("(?{kind}"), &format!("named capture group '{nm}' (#{group_no})"), "");
                        }
                    }
                    Some(m) => {
                        ex.add(&format!("(?{}", group(&m, 1)), "special group syntax", &format!("(?…) groups are not supported in {fname}"));
                        i += m.get(0).unwrap().as_str().chars().count();
                    }
                    None => {
                        group_no += 1;
                        ex.add(if flavor != "bre" { "(" } else { "\\(" }, &format!("start capture group #{group_no}"), "");
                    }
                }
                ex.depth += 1;
            }
            ')' => {
                ex.flush();
                ex.depth = ex.depth.saturating_sub(1);
                ex.add(if flavor != "bre" { ")" } else { "\\)" }, "end of group", "");
            }
            '}' => ex.lit.push('}'),
            _ => {}
        }
    }
    ex.flush();
    if ex.depth > 0 {
        let d = ex.depth;
        ex.rows.push((0, String::new(), String::new(), format!("{d} group(s) not closed")));
    }
    ex.rows
}

pub fn render_explain(pattern: &str, flavor: &str, color: bool) -> String {
    let (b, d, r, c, x) = if color { ("\x1b[1m", "\x1b[2m", "\x1b[31m", "\x1b[36m", "\x1b[0m") } else { ("", "", "", "", "") };
    let mut out = vec![format!("{d}{}{x}", flavor_name(flavor)), String::new()];
    if pattern.is_empty() {
        out.push(format!("{d}type a pattern…  TAB in the pattern slot shows every piece with its meaning{x}"));
        return out.join("\n");
    }
    for (depth, tok, meaning, warn) in explain(pattern, flavor) {
        let pad = "  ".repeat(depth);
        if !tok.is_empty() {
            out.push(format!("{pad}{c}{b}{tok:<10}{x} {meaning}"));
        }
        if !warn.is_empty() {
            out.push(format!("{pad}{:10} {r}⚠ {warn}{x}", ""));
        }
    }
    out.join("\n")
}

// ------------------------------------------------------------ palette & recipes

pub fn palette(flavor: &str) -> Vec<(String, String)> {
    let bs = flavor == "bre";
    let e = |s: &str| if bs { format!("\\{s}") } else { s.to_string() };
    let mut p: Vec<(String, String)> = vec![
        (".".into(), "any single character".into()),
        (".*".into(), "anything (0 or more of any char)".into()),
        ("^".into(), "start of line".into()),
        ("$".into(), "end of line".into()),
        ("*".into(), "previous item 0+ times".into()),
        (e("+"), "previous item 1+ times".into()),
        (e("?"), "previous item optional".into()),
        (format!("{}2{}", e("{"), e("}")), "previous item exactly 2 times".into()),
        (format!("{}2,5{}", e("{"), e("}")), "previous item 2 to 5 times".into()),
        (format!("{}3,{}", e("{"), e("}")), "previous item 3 or more times".into()),
        (e("("), format!("start a group  (close with {})", e(")"))),
        (e(")"), "end a group".into()),
        (e("|"), "OR: this or that".into()),
        ("[0-9]".into(), "one digit".into()),
        ("[a-z]".into(), "one lowercase letter".into()),
        ("[A-Za-z]".into(), "one letter".into()),
        ("[^ ]".into(), "one non-space character".into()),
        ("[[:space:]]".into(), "one whitespace character".into()),
        ("[[:alnum:]_]".into(), "one word character".into()),
        ("\\.".into(), "a literal dot".into()),
    ];
    let add = |p: &mut Vec<(String, String)>, v: &[(&str, &str)]| p.extend(v.iter().map(|(a, b)| (a.to_string(), b.to_string())));
    if modern(flavor) {
        add(&mut p, &[("\\d", "a digit"), ("\\d+", "a number (1+ digits)"), ("\\w+", "a word"), ("\\s+", "whitespace"),
            ("\\b", "word boundary"), ("(?i)", "ignore case from here"), ("(?:", "group without capturing"),
            (".*?", "anything, as little as possible (lazy)")]);
    }
    if gnu_posix(flavor) {
        add(&mut p, &[("\\w", "a word character (GNU)"), ("\\s", "whitespace (GNU)"), ("\\b", "word boundary (GNU)"),
            ("\\<", "start of word (GNU)"), ("\\>", "end of word (GNU)")]);
    }
    if flavor == "pcre" {
        add(&mut p, &[("(?=", "lookahead: followed by…"), ("(?!", "negative lookahead: not followed by…"),
            ("(?<=", "lookbehind: preceded by…"), ("(?<!", "negative lookbehind"), ("\\K", "keep only what follows")]);
    }
    p
}

/// name, ERE form, modern (PCRE/Rust) form
pub const RECIPES: &[(&str, &str, &str)] = &[
    ("IPv4 address", r"([0-9]{1,3}\.){3}[0-9]{1,3}", r"\b(?:\d{1,3}\.){3}\d{1,3}\b"),
    ("email address", r"[[:alnum:]._%+-]+@[[:alnum:].-]+\.[[:alpha:]]{2,}", r"[\w.%+-]+@[\w.-]+\.[A-Za-z]{2,}"),
    ("URL", r#"https?://[^[:space:]\"'<>]+"#, r#"https?://[^\s\"'<>]+"#),
    ("ISO date 2026-10-30", r"[0-9]{4}-[0-9]{2}-[0-9]{2}", r"\b\d{4}-\d{2}-\d{2}\b"),
    ("time 23:59(:59)", r"[0-2][0-9]:[0-5][0-9](:[0-5][0-9])?", r"\b[0-2]\d:[0-5]\d(?::[0-5]\d)?\b"),
    ("UUID", r"[0-9a-fA-F]{8}-([0-9a-fA-F]{4}-){3}[0-9a-fA-F]{12}", r"\b[0-9a-fA-F]{8}(?:-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}\b"),
    ("hex colour #ff8800", r"#[0-9a-fA-F]{6}", r"#[0-9a-fA-F]{6}\b"),
    ("MAC address", r"([0-9a-fA-F]{2}:){5}[0-9a-fA-F]{2}", r"\b(?:[0-9a-fA-F]{2}:){5}[0-9a-fA-F]{2}\b"),
    ("semver 1.2.3", r"[0-9]+\.[0-9]+\.[0-9]+", r"\bv?\d+\.\d+\.\d+(?:-[\w.]+)?\b"),
    ("TODO / FIXME / XXX", r"TODO|FIXME|XXX|HACK", r"\b(?:TODO|FIXME|XXX|HACK)\b"),
    ("trailing whitespace", r"[[:space:]]+$", r"\s+$"),
    ("blank line", r"^[[:space:]]*$", r"^\s*$"),
    ("comment line (#)", r"^[[:space:]]*#", r"^\s*#"),
    ("number (int or decimal)", r"-?[0-9]+(\.[0-9]+)?", r"-?\d+(?:\.\d+)?"),
    ("hex number 0x1f", r"0x[0-9a-fA-F]+", r"\b0x[0-9a-fA-F]+\b"),
    ("double-quoted string", r#"\"[^\"]*\""#, r#"\"(?:[^\"\\]|\\.)*\""#),
    ("function definition", r"^[[:space:]]*(def|function|fn|func)[[:space:]]+[[:alnum:]_]+", r"^\s*(?:def|function|fn|func)\s+\w+"),
    ("whole word (edit WORD)", r"\bWORD\b", r"\bWORD\b"),
    ("line starting with WORD", r"^WORD", r"^WORD"),
];

pub fn ere_to_bre(p: &str) -> String {
    let c: Vec<char> = p.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut in_class = false;
    while i < c.len() {
        let ch = c[i];
        if ch == '\\' && i + 1 < c.len() {
            out.push(ch);
            out.push(c[i + 1]);
            i += 2;
            continue;
        }
        if in_class {
            in_class = ch != ']';
            out.push(ch);
        } else if ch == '[' {
            in_class = true;
            out.push(ch);
            if c.get(i + 1) == Some(&'^') {
                out.push('^');
                i += 1;
            }
            if c.get(i + 1) == Some(&']') {
                out.push(']');
                i += 1;
            }
        } else if "(){}|+?".contains(ch) {
            out.push('\\');
            out.push(ch);
        } else {
            out.push(ch);
        }
        i += 1;
    }
    out
}

pub fn recipe_for(flavor: &str, ere: &str, modern_form: &str) -> String {
    if modern(flavor) {
        return modern_form.into();
    }
    match flavor {
        "bre" => ere_to_bre(ere),
        "emacs" => format!(".*{}.*", ere_to_bre(ere).replace("\\{", "{").replace("\\}", "}")),
        _ => ere.into(),
    }
}

pub fn candidates(tokens: &[String], current: &str) -> Vec<(String, String)> {
    let (_, flavor) = detect(tokens);
    if flavor == "fixed" {
        return vec![];
    }
    let raw = current.strip_prefix('\'').or_else(|| current.strip_prefix('"')).unwrap_or(current);
    let mut out = vec![];
    if raw.is_empty() {
        for (name, ere, m) in RECIPES {
            out.push((recipe_for(&flavor, ere, m), format!("recipe: {name}")));
        }
    }
    let trailing = raw.len() - raw.trim_end_matches('\\').len();
    let trailing_escape = trailing % 2 == 1;
    let base = if trailing_escape { &raw[..raw.len() - 1] } else { raw };
    for (tok, desc) in palette(&flavor) {
        if !raw.is_empty() && tok == "^" || trailing_escape && !tok.starts_with('\\') {
            continue;
        }
        out.push((format!("{base}{tok}"), desc));
    }
    if trailing_escape {
        for ch in ".*+?()[]{}|^$/".chars() {
            out.push((format!("{base}\\{ch}"), format!("a literal '{ch}'")));
        }
    }
    out
}

// ------------------------------------------------------------ quoting

/// Single-quote for fish, doubling a backslash only where fish would otherwise eat it.
pub fn fish_quote(s: &str) -> String {
    let c: Vec<char> = s.chars().collect();
    let mut out = String::from("'");
    for (i, ch) in c.iter().enumerate() {
        match ch {
            '\\' => {
                let next = c.get(i + 1);
                if next.is_none() || next == Some(&'\\') || next == Some(&'\'') {
                    out.push_str("\\\\");
                } else {
                    out.push('\\');
                }
            }
            '\'' => out.push_str("\\'"),
            c => out.push(*c),
        }
    }
    out.push('\'');
    out
}

/// Quote a pattern for the shell it is going back into.
pub fn quote_for(shell: &str, s: &str) -> String {
    match shell {
        "bash" | "zsh" | "sh" => crate::util::sq(s),
        _ => fish_quote(s),
    }
}

// ------------------------------------------------------------ live builder

pub fn search_command(tool: &str, flavor: &str, paths: &[String]) -> String {
    let qp = if paths.is_empty() { ".".to_string() } else { paths.iter().map(|p| shquote(p)).collect::<Vec<_>>().join(" ") };
    if which("rg").is_some() && (tool == "rg" || tool.is_empty()) || tool == "rg" {
        let eng = if flavor == "pcre" { "-P " } else { "" };
        return format!("rg --color=always -n --no-heading --max-columns=200 -m 50 {eng}-e {{q}} -- {qp} 2>&1 | head -2000");
    }
    if tool == "find" {
        let rt = match flavor { "ere" => "posix-extended", "bre" => "posix-basic", _ => "emacs" };
        return format!("find {qp} -regextype {rt} -regex {{q}} 2>/dev/null | head -2000");
    }
    if ["sed", "awk", "string", "python"].contains(&tool) {
        let mode = match flavor { "ere" => "-E", "pcre" | "python" => "-P", _ => "" };
        return format!("grep -rnI --color=always {mode} -e {{q}} -- {qp} 2>&1 | head -2000");
    }
    let mode = match flavor { "ere" => "-E", "pcre" => "-P", "bre" => "-G", _ => "-E" };
    format!("grep -rnI --color=always {mode} -e {{q}} -- {qp} 2>&1 | head -2000")
}

pub fn build(tokens: &[String], initial: &str, paths: &[String], shell: &str) -> i32 {
    let (tool, mut flavor) = detect(tokens);
    if flavor == "fixed" {
        flavor = "ere".into();
    }
    if which("fzf").is_none() {
        return fallback_builder(&flavor, initial, shell);
    }
    let exe = std::env::current_exe().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| "complete-me".into());
    let search = search_command(&tool, &flavor, paths);
    let shown_tool = if tool.is_empty() { "grep" } else { &tool };
    let out = std::process::Command::new("fzf")
        .args([
            "--disabled", "--ansi", "--print-query", "--query", initial,
            "--prompt", &format!("regex [{shown_tool} · {flavor}] › "),
            "--header", "type to search live · ENTER puts the pattern on your command line · ESC cancels · ctrl-/ hides explain",
            "--bind", &format!("start:reload:[ -n {{q}} ] && {search} || true"),
            "--bind", &format!("change:reload:sleep 0.08; [ -n {{q}} ] && {search} || true"),
            "--bind", "ctrl-/:toggle-preview",
            "--preview", &format!("{} regex explain --color --flavor {flavor} -- {{q}}", shquote(&exe)),
            "--preview-window", "right,48%,wrap",
            "--height", "80%", "--layout", "reverse", "--border", "rounded",
        ])
        .env("SHELL", "/bin/sh")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::inherit())
        .output();
    let Ok(out) = out else { return 130 };
    let code = out.status.code().unwrap_or(130);
    let text = String::from_utf8_lossy(&out.stdout);
    if !(code == 0 || code == 1) || text.is_empty() {
        return 130;
    }
    println!("{}", quote_for(shell, text.split('\n').next().unwrap_or("")));
    0
}

fn fallback_builder(flavor: &str, initial: &str, shell: &str) -> i32 {
    use std::io::{BufRead, Write};
    let (Ok(tin), Ok(mut tout)) = (std::fs::File::open("/dev/tty"), std::fs::OpenOptions::new().write(true).open("/dev/tty")) else {
        eprintln!("complete-me: no terminal; install fzf for the live builder");
        return 1;
    };
    use std::os::unix::io::AsRawFd;
    let fd = tin.as_raw_fd();
    let mut saved: libc::termios = unsafe { std::mem::zeroed() };
    let have = unsafe { libc::tcgetattr(fd, &mut saved) } == 0;
    if have {
        let mut mode = saved;
        mode.c_lflag |= libc::ECHO | libc::ICANON | libc::ISIG; // normal line editing while we prompt
        mode.c_iflag |= libc::ICRNL;
        mode.c_oflag |= libc::OPOST | libc::ONLCR;
        unsafe { libc::tcsetattr(fd, libc::TCSANOW, &mode) };
    }
    let mut pat = initial.to_string();
    let _ = write!(tout, "\n(fzf not found — simple mode. Install fzf for live matching: sudo pacman -S fzf)\n");
    let mut reader = std::io::BufReader::new(tin);
    let mut cancelled = false;
    loop {
        let _ = write!(tout, "\n{}\n\npattern (empty line = accept, ctrl-c = cancel) › ", render_explain(&pat, flavor, true));
        let _ = tout.flush();
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) if line == "\n" => break,
            Ok(_) => pat = line.trim_end_matches('\n').to_string(),
            Err(_) => {
                cancelled = true;
                break;
            }
        }
    }
    if have {
        unsafe { libc::tcsetattr(fd, libc::TCSANOW, &saved) };
    }
    if cancelled {
        return 130;
    }
    println!("{}", quote_for(shell, &pat));
    0
}

// ------------------------------------------------------------ CLI

pub fn run(args: &[String]) -> i32 {
    let Some(sub) = args.first() else {
        eprintln!("usage: complete-me regex <explain|complete|build|flavor> …");
        return 2;
    };
    let a = Args::parse(&args[1..], &["--flavor", "--tool", "--current", "--path", "--shell"]);
    match sub.as_str() {
        "explain" => {
            let pattern = a.rest.first().cloned().unwrap_or_default();
            let flavor = match a.value("--flavor") {
                Some(f) => f,
                None => match a.value("--tool") {
                    Some(t) if !t.is_empty() => detect(&shell_words(&t)).1,
                    _ => "ere".into(),
                },
            };
            println!("{}", render_explain(&pattern, &flavor, a.has("--color") || is_tty(1)));
            0
        }
        "complete" => {
            let lines: Vec<String> = candidates(&a.rest, &a.value("--current").unwrap_or_default())
                .into_iter()
                .map(|(v, d)| format!("{v}\t{d}"))
                .collect();
            println!("{}", lines.join("\n"));
            0
        }
        "build" if a.has("--line") => {
            // bash/zsh: `regex build --shell bash --line -- "$LEFT_OF_CURSOR"`
            let l = crate::extras::split_line(&a.rest.join(" "));
            build(&l.words, l.raw.trim_matches(|c| c == '\'' || c == '"'), &a.values("--path"), &a.value("--shell").unwrap_or_default())
        }
        "build" => {
            let cur = a.value("--current").unwrap_or_default();
            build(&a.rest, cur.trim_matches(|c| c == '\'' || c == '"'), &a.values("--path"), &a.value("--shell").unwrap_or_default())
        }
        "flavor" => {
            let (t, f) = detect(&a.rest);
            println!("{t}\t{f}");
            0
        }
        other => {
            eprintln!("complete-me regex: unknown subcommand {other:?}");
            2
        }
    }
}
