//! Shell glob → regular expression, as mvdan.cc/sh's pattern.Regexp (BSD-3-Clause) does it in
//! the mode babelfish uses (no braces, no filename rules; `shortest` makes `*` lazy).

fn quote_meta(c: char, out: &mut String) {
    if "\\.+*?()|[]{}^$".contains(c) {
        out.push('\\');
    }
    out.push(c);
}

fn char_class(s: &[char]) -> Result<Option<usize>, String> {
    let st: String = s.iter().take(3).collect();
    if st == "[[." || st == "[[=" {
        return Err("collating features not available".into());
    }
    if st != "[[:" {
        return Ok(None);
    }
    let rest: String = s[3..].iter().collect();
    let Some(end) = rest.find(":]]") else { return Err("[[: was not matched with a closing :]]".into()) };
    let name = &rest[..end];
    match name {
        "alnum" | "alpha" | "ascii" | "blank" | "cntrl" | "digit" | "graph" | "lower" | "print" | "punct" | "space" | "upper"
        | "word" | "xdigit" => Ok(Some(name.chars().count() + 6)),
        _ => Err(format!("invalid character class: {name:?}")),
    }
}

pub fn regexp(pat: &str, shortest: bool) -> Result<String, String> {
    if !pat.chars().any(|c| "*?[\\.+()|]{}^$".contains(c)) {
        return Ok(pat.to_string());
    }
    let p: Vec<char> = pat.chars().collect();
    let mut buf = String::from("(?s)");
    let mut dot_meta = false;
    let mut i = 0;
    while i < p.len() {
        let c = p[i];
        match c {
            '*' => {
                buf.push_str(".*");
                dot_meta = true;
                if shortest {
                    buf.push('?');
                }
            }
            '?' => {
                buf.push('.');
                dot_meta = true;
            }
            '\\' => {
                i += 1;
                if i >= p.len() {
                    return Err("\\ at end of pattern".into());
                }
                quote_meta(p[i], &mut buf);
            }
            '[' => {
                if let Some(n) = char_class(&p[i..])? {
                    buf.extend(&p[i..i + n]);
                    i += n;
                    continue;
                }
                buf.push('[');
                i += 1;
                if i >= p.len() {
                    return Err("[ was not matched with a closing ]".into());
                }
                if p[i] == '!' || p[i] == '^' {
                    buf.push('^');
                    i += 1;
                    if i >= p.len() {
                        return Err("[ was not matched with a closing ]".into());
                    }
                }
                if p[i] == ']' {
                    buf.push(']');
                    i += 1;
                    if i >= p.len() {
                        return Err("[ was not matched with a closing ]".into());
                    }
                }
                let mut range_start: Option<char> = None;
                let mut closed = false;
                while i < p.len() {
                    let c = p[i];
                    buf.push(c);
                    if c == '\\' {
                        i += 1;
                        if i < p.len() {
                            buf.push(p[i]);
                        }
                        i += 1;
                        continue;
                    }
                    if c == ']' {
                        closed = true;
                        break;
                    }
                    if let Some(rs) = range_start {
                        if rs > c {
                            return Err(format!("invalid range: {rs}-{c}"));
                        }
                    }
                    range_start = if c == '-' { Some(p[i - 1]) } else { None };
                    i += 1;
                }
                if !closed {
                    return Err("[ was not matched with a closing ]".into());
                }
            }
            c if (c as u32) > 128 => buf.push(c),
            c => quote_meta(c, &mut buf),
        }
        i += 1;
    }
    if !dot_meta {
        return Ok(buf[4..].to_string());
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    #[test]
    fn like_mvdan() {
        assert_eq!(super::regexp("/", false).unwrap(), "/");
        assert_eq!(super::regexp("*/", false).unwrap(), "(?s).*/");
        assert_eq!(super::regexp(":", true).unwrap(), ":");
        assert_eq!(super::regexp("*.txt", true).unwrap(), "(?s).*?\\.txt");
        assert_eq!(super::regexp("[[:digit:]]x", false).unwrap(), "[[:digit:]]x");
    }
}
