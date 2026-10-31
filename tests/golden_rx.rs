//! regex + find helpers must reproduce the Python goldens exactly.
use complete_me::{findhelp, rx};

fn golden(name: &str) -> String {
    std::fs::read_to_string(format!("tests/golden/{name}")).unwrap()
}

fn py_repr(s: &str) -> String {
    let q = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut o = String::from(q);
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\t' => o.push_str("\\t"),
            c if c == q => {
                o.push('\\');
                o.push(c);
            }
            c => o.push(c),
        }
    }
    o.push(q);
    o
}

fn py_list(v: &[&str]) -> String {
    format!("[{}]", v.iter().map(|s| py_repr(s)).collect::<Vec<_>>().join(", "))
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

fn compare(name: &str, got: &str, fails: &mut Vec<String>) {
    let want = golden(name);
    if want == got {
        return;
    }
    let (w, g): (Vec<&str>, Vec<&str>) = (want.lines().collect(), got.lines().collect());
    for i in 0..w.len().max(g.len()) {
        let (a, b) = (w.get(i).copied().unwrap_or("<EOF>"), g.get(i).copied().unwrap_or("<EOF>"));
        if a != b {
            fails.push(format!("{name} line {}:\n  want: {a}\n  got : {b}", i + 1));
            return;
        }
    }
    fails.push(format!("{name}: trailing whitespace differs"));
}

const PATS: &[&str] = &[r"^\d{3}-[A-Za-z_]+(foo|bar)?$", r"error\(s\)\{2,\}\|warn+", r"(?<=id=)\w+?\b", r"\bTODO\b.*\.py",
    r"[^[:space:]]+@x\.com", "a{2,5}", "a{x", "(ab", r"a\(b\)\{2\}c+", r"(?i)foo(?:bar)+", r"x*?y+?",
    r"\1(a)", r"[]a-]", r"\p{L}+\x41", "plain", "", r"(?P<year>\d{4})-(?!00)", r"\Kx\h"];

#[test]
fn rx_goldens() {
    let mut fails = vec![];
    // explain grid
    let mut out = vec![];
    for p in PATS {
        for (fl, _) in rx::FLAVORS {
            out.push(format!("=== {} {fl}", py_repr(p)));
            out.push(rx::render_explain(p, fl, false));
        }
    }
    compare("regex.explain.txt", &(out.join("\n") + "\n"), &mut fails);
    // complete
    let mut out = vec![];
    let toksets: &[&[&str]] = &[&["grep"], &["grep", "-E"], &["grep", "-P"], &["rg"], &["rg", "-P"], &["sed"], &["find", "."], &["grep", "-F"], &["string"]];
    for t in toksets {
        for cur in ["", "'", "'err", "\"ab\\", "x"] {
            out.push(format!("=== {} {}", py_list(t), py_repr(cur)));
            for (v, d) in rx::candidates(&s(t), cur) {
                out.push(format!("{v}\t{d}"));
            }
        }
    }
    compare("regex.complete.txt", &(out.join("\n") + "\n"), &mut fails);
    // detect
    let dets: &[&[&str]] = &[&["grep", "-rnE", "x"], &["grep", "x"], &["egrep"], &["fgrep"], &["rg", "x"], &["rg", "--pcre2"], &["rg", "--engine=pcre2"],
        &["sed", "-E"], &["sed", "-r"], &["sed"], &["awk"], &["find", ".", "-regextype", "posix-extended"], &["find", "."],
        &["string", "match"], &["python3"], &["foo"], &["ugrep"], &["grep", "--perl-regexp"], &["grep", "-F"]];
    let out: Vec<String> = dets.iter().map(|t| { let (a, b) = rx::detect(&s(t)); format!("{}\t{a} {b}", py_list(t)) }).collect();
    compare("regex.detect.txt", &(out.join("\n") + "\n"), &mut fails);
    // quote
    let out: Vec<String> = [r"\d{4}", "bar's", "a\\\\b", "end\\", "", "'", "a'b\\'c"].iter().map(|x| format!("{}\t{}", py_repr(x), rx::fish_quote(x))).collect();
    compare("regex.quote.txt", &(out.join("\n") + "\n"), &mut fails);
    // find explain
    let lines = ["find . -type f -name *.log -mtime +7 -exec rm {} ;", "find . -name *.tmp -o -name *.bak -exec rm {} +",
        "find /var -delete -name *.log", "find . -path ./.git -prune -o ( -type f -size +100M ) -print",
        "find . ! -user root -perm -o+w -exec ls -l", "find", "find . -size 10k -mmin -5 -newer x",
        "find -L a b -iname X -o ( -empty -a -readable ) -ls", "find . -execdir echo {} + -quit", "find . -bogus 1"];
    let mut out = vec![];
    for l in lines {
        out.push(format!("=== {l}"));
        out.push(findhelp::explain(&l.split_whitespace().map(String::from).collect::<Vec<_>>()));
    }
    compare("find.explain.txt", &(out.join("\n") + "\n"), &mut fails);
    // find values, in an empty directory
    let dir = std::env::temp_dir().join(format!("cm-empty-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let here = std::env::current_dir().unwrap();
    std::env::set_current_dir(&dir).unwrap();
    let mut out = vec![];
    for prev in ["-type", "-xtype", "-size", "-mtime", "-atime", "-ctime", "-mmin", "-amin", "-cmin", "-perm", "-name",
        "-iname", "-path", "-ipath", "-wholename", "-maxdepth", "-mindepth", "-regextype", "-printf", "-newermt", "-nothing"] {
        out.push(format!("=== {prev}"));
        out.extend(findhelp::values_for(prev).into_iter().map(|(x, y)| format!("{x}\t{y}")));
    }
    out.push("=== operators".into());
    out.extend(findhelp::operators().into_iter().map(|(x, y)| format!("{x}\t{y}")));
    std::env::set_current_dir(here).unwrap();
    let _ = std::fs::remove_dir(&dir);
    compare("find.values.txt", &(out.join("\n") + "\n"), &mut fails);
    assert!(fails.is_empty(), "\n{}", fails.join("\n\n"));
}
