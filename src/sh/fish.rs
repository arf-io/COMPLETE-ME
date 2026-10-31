//! bash → fish. The rules follow babelfish's translate.go (MIT, © 2020 Bouke van der Bijl; see
//! THIRD_PARTY.md) case for case, so its output is reproduced byte for byte. Where babelfish gives
//! up or gets the semantics wrong, the rule is extended; each such place is marked `EXT:` and is
//! checked by running both the bash and the fish version (tests/bash2fish.rs).
use super::ast::*;
use super::pattern;

pub type TResult = Result<(), String>;

/// What `source FILE` and `eval "$(cmd)"` become: run the bash through this translator first.
pub const SELF_CMD: &str = "complete-me bash2fish --raw";

fn unsupported<T>(what: &str) -> Result<T, String> {
    Err(format!("not supported yet: {what}"))
}

pub struct Translator<'a> {
    buf: String,
    indent: usize,
    heredocs: &'a [Word],
    location: String,
    in_function: usize,
}

pub fn translate(f: &File) -> Result<String, String> {
    translate_with(f, SELF_CMD)
}

pub fn translate_with(f: &File, location: &str) -> Result<String, String> {
    let mut t = Translator { buf: String::new(), indent: 0, heredocs: &f.heredocs, location: location.into(), in_function: 0 };
    for (i, s) in f.stmts.iter().enumerate() {
        t.stmt(s)?;
        t.nl();
        let last = i + 1 == f.stmts.len();
        if matches!(s.cmd, Cmd::Func { .. }) && !last && s.end_line < f.stmts[i + 1].line - 1 {
            t.nl();
        }
    }
    for c in &f.last {
        t.comment(c);
    }
    Ok(t.buf)
}

/// Decode `$'…'` escapes.
pub fn ansi_c(raw: &str) -> String {
    let mut out = String::new();
    let mut it = raw.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('a') => out.push('\x07'),
            Some('b') => out.push('\x08'),
            Some('e') | Some('E') => out.push('\x1b'),
            Some('f') => out.push('\x0c'),
            Some('v') => out.push('\x0b'),
            Some('\\') => out.push('\\'),
            Some('\'') => out.push('\''),
            Some('"') => out.push('"'),
            Some('?') => out.push('?'),
            Some('x') => {
                let mut h = String::new();
                while h.len() < 2 && it.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                    h.push(it.next().unwrap());
                }
                if let Some(ch) = u32::from_str_radix(&h, 16).ok().and_then(char::from_u32) {
                    out.push(ch);
                }
            }
            Some(d @ '0'..='7') => {
                let mut o = d.to_string();
                while o.len() < 3 && it.peek().is_some_and(|c| ('0'..='7').contains(c)) {
                    o.push(it.next().unwrap());
                }
                if let Some(ch) = u32::from_str_radix(&o, 8).ok().and_then(char::from_u32) {
                    out.push(ch);
                }
            }
            Some(o) => {
                out.push('\\');
                out.push(o);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// babelfish's unescape (taken from mvdan.cc/sh): drop a backslash before / " \ $ `.
fn unescape(s: &str) -> String {
    if !s.contains('\\') {
        return s.to_string();
    }
    let b: Vec<char> = s.chars().collect();
    let mut out = String::new();
    for i in 0..b.len() {
        if b[i] == '\\' && i + 1 < b.len() && matches!(b[i + 1], '/' | '"' | '\\' | '$' | '`') {
            continue;
        }
        out.push(b[i]);
    }
    out
}

fn literal_variable(name: &str) -> Option<&'static str> {
    match name {
        "UID" => Some("(id -ru)"),
        "EUID" => Some("(id -u)"),
        "GROUPS" => Some("(id -G | string split ' ')"),
        _ => None,
    }
}

fn special_variable(name: &str) -> Option<&'static str> {
    match name {
        "?" => Some("status"),
        "$" | "BASH_PID" => Some("fish_pid"),
        "*" | "@" => Some("argv"),
        "HOSTNAME" => Some("hostname"),
        "!" => Some("last_pid"), // EXT
        _ => None,
    }
}

/// `{1..5}` / `{1..10..2}` in an unquoted literal.
fn brace_range(s: &str) -> Option<(usize, usize, String)> {
    let r = crate::re!(r"\{(-?\d+)\.\.(-?\d+)(?:\.\.(-?\d+))?\}");
    let c = crate::util::find(r, s)?;
    let m = c.get(0)?;
    let (a, b) = (crate::util::group(&c, 1), crate::util::group(&c, 2));
    let step = crate::util::group(&c, 3);
    let seq = if step.is_empty() { format!("(seq {a} {b})") } else { format!("(seq {a} {step} {b})") };
    let (a, b): (i64, i64) = (a.parse().ok()?, b.parse().ok()?);
    if a > b && step.is_empty() {
        return Some((m.start(), m.end(), format!("(seq {a} -1 {b})")));
    }
    Some((m.start(), m.end(), seq))
}

fn contains_continue(stmts: &[Stmt]) -> bool {
    fn word_has(w: &Word) -> bool {
        w.as_lit().is_some_and(|l| l == "continue")
    }
    fn s_has(s: &Stmt) -> bool {
        match &s.cmd {
            Cmd::Call { args, .. } => args.first().is_some_and(word_has),
            Cmd::Binary { x, y, .. } => s_has(x) || s_has(y),
            Cmd::Block(b) | Cmd::Subshell(b) => b.iter().any(s_has),
            Cmd::If(i) => {
                let mut c = Some(i);
                while let Some(ic) = c {
                    if ic.cond.iter().any(s_has) || ic.then.iter().any(s_has) {
                        return true;
                    }
                    c = ic.els.as_deref();
                }
                false
            }
            Cmd::While { cond, body, .. } => cond.iter().any(s_has) || body.iter().any(s_has),
            Cmd::For { body, .. } | Cmd::CFor { body, .. } => body.iter().any(s_has),
            Cmd::Case { items, .. } => items.iter().any(|i| i.stmts.iter().any(s_has)),
            _ => false,
        }
    }
    stmts.iter().any(s_has)
}

/// The regex for the right side of `[[ x == … ]]` when it has unquoted glob characters (quoted
/// parts stay literal), or None when it is a plain string.
fn glob_word_regex(w: &Word) -> Option<String> {
    let mut glob = String::new();
    let mut any = false;
    let quote = |s: &str, g: &mut String| {
        for c in s.chars() {
            if "*?[]\\".contains(c) {
                g.push('\\');
            }
            g.push(c);
        }
    };
    for p in &w.parts {
        match p {
            Part::Lit(l) => {
                let mut it = l.chars();
                while let Some(c) = it.next() {
                    if c == '\\' {
                        if let Some(n) = it.next() {
                            quote(&n.to_string(), &mut glob);
                        }
                        continue;
                    }
                    if "*?[".contains(c) {
                        any = true;
                    }
                    glob.push(c);
                }
            }
            Part::Sgl(t) => quote(t, &mut glob),
            Part::Dbl(ps) => {
                for p in ps {
                    match p {
                        Part::Lit(l) => quote(&unescape(l), &mut glob),
                        _ => return None,
                    }
                }
            }
            _ => return None,
        }
    }
    if !any {
        return None;
    }
    pattern::regexp(&glob, false).ok().map(|r| r.trim_start_matches("(?s)").to_string())
}

#[derive(Clone, Copy, PartialEq)]
enum ArithRet {
    Value,
    Status,
}

impl<'a> Translator<'a> {
    fn str(&mut self, s: &str) {
        self.buf.push_str(s);
    }
    fn nl(&mut self) {
        self.buf.push('\n');
        for _ in 0..self.indent {
            self.buf.push_str("  ");
        }
    }
    fn indent(&mut self) {
        self.indent += 1;
        self.nl();
    }
    fn outdent(&mut self) {
        self.indent -= 1;
        self.nl();
    }
    fn comment(&mut self, c: &str) {
        self.str(&format!("#{c}"));
        self.nl();
    }
    fn escaped(&mut self, lit: &str) {
        self.str("'");
        self.str(&lit.replace('\\', "\\\\").replace('\'', "\\'"));
        self.str("'");
    }
    fn capture(&mut self, f: impl FnOnce(&mut Self) -> TResult) -> TResult {
        let old = std::mem::take(&mut self.buf);
        let r = f(self);
        let inner = std::mem::replace(&mut self.buf, old);
        r?;
        self.escaped(&inner);
        Ok(())
    }

    // ------------------------------------------------------------ statements

    fn stmt(&mut self, s: &Stmt) -> TResult {
        if s.coprocess {
            return unsupported("coprocesses");
        }
        for c in &s.comments {
            self.comment(c);
        }
        if s.negated {
            self.str("! ");
        }
        self.command(s)?;
        for r in &s.redirs {
            self.str(" ");
            if let Some(n) = &r.n {
                self.str(n);
            }
            match r.op.as_str() {
                "<>" | "<" | ">" | ">>" | "<&" | ">&" => {
                    self.str(&r.op);
                    self.word(&r.word, false)?;
                }
                // EXT: babelfish uses `echo`, which adds a second newline after the body
                "<<" | "<<-" => {
                    let body = self.heredocs.get(r.hdoc.unwrap_or(usize::MAX)).cloned().unwrap_or_default();
                    self.str("<(printf '%s' ");
                    self.word(&body, true)?;
                    self.str(" | psub)");
                }
                "<<<" => {
                    self.str("<(echo ");
                    self.word(&r.word, true)?;
                    self.str("| psub)");
                }
                // EXT: fish has these too
                "&>" | "&>>" => {
                    self.str(&r.op);
                    self.word(&r.word, false)?;
                }
                ">|" => {
                    self.str(">");
                    self.word(&r.word, false)?;
                }
                op => return unsupported(&format!("the {op} redirection")),
            }
        }
        // EXT: babelfish refuses `cmd &`
        if s.background {
            self.str(" &");
        }
        Ok(())
    }

    fn stmts(&mut self, list: &[Stmt]) -> TResult {
        for (i, s) in list.iter().enumerate() {
            if i > 0 {
                self.str("; ");
            }
            self.stmt(s)?;
        }
        Ok(())
    }

    fn body(&mut self, list: &[Stmt]) -> TResult {
        for (i, s) in list.iter().enumerate() {
            if i > 0 {
                self.nl();
            }
            self.stmt(s)?;
        }
        Ok(())
    }

    fn command(&mut self, s: &Stmt) -> TResult {
        match &s.cmd {
            Cmd::Arith(e) => self.arith(e, ArithRet::Status),
            Cmd::Binary { op, x, y } => {
                self.stmt(x)?;
                self.str(match op {
                    BinOp::And => " && ",
                    BinOp::Or => " || ",
                    BinOp::Pipe => " | ",
                    BinOp::PipeAll => " &| ", // EXT
                });
                self.stmt(y)
            }
            // EXT: a bare { …; } needs begin/end in fish (babelfish drops the braces, so
            // `x || { a; b; }` would run b unconditionally); a function body does not
            Cmd::Block(b) => {
                self.str("begin");
                self.indent();
                self.body(b)?;
                self.outdent();
                self.str("end");
                Ok(())
            }
            Cmd::Call { assigns, args } => self.call(assigns, args),
            Cmd::Case { word, items } => {
                self.str("switch ");
                self.word(word, true)?;
                self.nl();
                for item in items {
                    if item.op != ";;" {
                        return unsupported(&format!("case fall-through ({})", item.op));
                    }
                    self.str("case");
                    for p in &item.patterns {
                        self.str(" ");
                        self.word(p, true)?;
                    }
                    self.indent();
                    self.body(&item.stmts)?;
                    self.outdent();
                }
                self.str("end");
                Ok(())
            }
            Cmd::Coproc => unsupported("coproc"),
            Cmd::Decl { variant, args } => self.decl(variant, args),
            Cmd::For { name, items, body, select } => {
                if *select {
                    return unsupported("select");
                }
                self.str(&format!("for {name}"));
                match items {
                    Some(ws) => {
                        self.str(" in");
                        for w in ws {
                            self.str(" ");
                            self.word(w, false)?;
                        }
                    }
                    None => self.str(" in $argv"),
                }
                self.indent();
                self.body(body)?;
                self.outdent();
                self.str("end");
                Ok(())
            }
            // EXT: C-style for → init; while cond; body; post; end
            Cmd::CFor { init, cond, post, body } => {
                if contains_continue(body) {
                    return unsupported("`continue` inside a C-style for loop");
                }
                if let Some(i) = init {
                    self.arith(i, ArithRet::Status)?;
                    self.nl();
                }
                self.str("while ");
                match cond {
                    Some(c) => self.arith(c, ArithRet::Status)?,
                    None => self.str("true"),
                }
                self.indent();
                self.body(body)?;
                if let Some(p) = post {
                    if !body.is_empty() {
                        self.nl();
                    }
                    self.arith(p, ArithRet::Status)?;
                }
                self.outdent();
                self.str("end");
                Ok(())
            }
            Cmd::Func { name, body } => {
                self.str(&format!("function {name}"));
                self.indent();
                self.in_function += 1;
                // the body's own braces are the function's: print the statements directly
                let r = match &body.cmd {
                    Cmd::Block(stmts) if body.redirs.is_empty() && !body.negated && !body.background => {
                        for c in &body.comments {
                            self.comment(c);
                        }
                        self.body(stmts)
                    }
                    _ => self.stmt(body),
                };
                self.in_function -= 1;
                r?;
                self.outdent();
                self.str("end");
                Ok(())
            }
            Cmd::If(i) => self.if_clause(i, false),
            Cmd::Let(exprs) => {
                // EXT: let a=1 b++ → the same as (( … ))
                for (k, e) in exprs.iter().enumerate() {
                    if k > 0 {
                        self.str("; ");
                    }
                    self.arith(e, ArithRet::Status)?;
                }
                Ok(())
            }
            Cmd::Subshell(stmts) => {
                self.str("fish -c ");
                self.capture(|t| t.stmts(stmts))
            }
            Cmd::Test(e) => {
                self.str("test ");
                self.test_expr(e)
            }
            Cmd::Time(inner) => {
                let Some(inner) = inner else { return unsupported("time with no command") };
                self.str("time ");
                self.stmt(inner)
            }
            Cmd::While { until, cond, body } => {
                self.str("while ");
                if *until {
                    self.str("not ");
                }
                self.stmts(cond)?;
                self.indent();
                self.body(body)?;
                self.outdent();
                self.str("end");
                Ok(())
            }
        }
    }

    fn if_clause(&mut self, i: &IfClause, elif: bool) -> TResult {
        self.str(if elif { "else if " } else { "if " });
        self.stmts(&i.cond)?;
        self.indent();
        self.body(&i.then)?;
        self.outdent();
        if let Some(el) = &i.els {
            if el.elif {
                return self.if_clause(el, true);
            }
            self.str("else");
            self.indent();
            self.body(&el.then)?;
            self.outdent();
        }
        self.str("end");
        Ok(())
    }

    fn test_expr(&mut self, e: &TestExpr) -> TResult {
        match e {
            TestExpr::Bin(op, x, y) => {
                // EXT: [[ x =~ re ]] → string match (babelfish refuses it)
                if op == "=~" {
                    let (TestExpr::Word(xw), TestExpr::Word(yw)) = (&**x, &**y) else { return unsupported("=~") };
                    // replace the "test " already written
                    if self.buf.ends_with("test ") {
                        self.buf.truncate(self.buf.len() - 5);
                    }
                    self.str("string match -qr -- ");
                    self.word(yw, true)?;
                    self.str(" ");
                    return self.word(xw, true);
                }
                // EXT: [[ x == pat* ]] matches a pattern (babelfish compared strings)
                if op == "==" || op == "=" || op == "!=" {
                    if let (TestExpr::Word(xw), TestExpr::Word(yw)) = (&**x, &**y) {
                        if let Some(re) = glob_word_regex(yw) {
                            if self.buf.ends_with("test ") {
                                self.buf.truncate(self.buf.len() - 5);
                            }
                            if op == "!=" {
                                self.str("not ");
                            }
                            self.str("string match -qr -- ");
                            self.escaped(&format!("^({re})$"));
                            self.str(" ");
                            return self.word(xw, true);
                        }
                    }
                }
                self.test_expr(x)?;
                match op.as_str() {
                    "&&" => self.str(" && test "),
                    "||" => self.str(" || test "),
                    "==" | "=" => self.str(" = "), // EXT: "=" (babelfish handles only "==")
                    "!=" => self.str(" != "),
                    "-eq" | "-ne" | "-le" | "-ge" | "-lt" | "-gt" => self.str(&format!(" {op} ")),
                    // EXT
                    "-nt" | "-ot" | "-ef" => self.str(&format!(" {op} ")),
                    op => return unsupported(&format!("the {op} test")),
                }
                self.test_expr(y)
            }
            TestExpr::Paren(x) => {
                self.str("\\( ");
                self.test_expr(x)?;
                self.str(" \\)");
                Ok(())
            }
            TestExpr::Un(op, x) => {
                match op.as_str() {
                    "-e" | "-f" | "-d" | "-c" | "-b" | "-p" | "-S" | "-L" | "-h" | "-k" | "-g" | "-u" | "-G" | "-O" | "-r"
                    | "-w" | "-x" | "-s" | "-t" | "-z" | "-n" | "!" => self.str(&format!("{} ", if op == "-h" { "-L" } else { op })),
                    // EXT: [[ -a f ]] is -e; [[ -v name ]] is set -q
                    "-a" => self.str("-e "),
                    "-v" => {
                        if self.buf.ends_with("test ") {
                            self.buf.truncate(self.buf.len() - 5);
                        }
                        self.str("set -q ");
                        return match &**x {
                            TestExpr::Word(w) => self.word(w, false),
                            _ => unsupported("-v"),
                        };
                    }
                    op => return unsupported(&format!("the {op} test")),
                }
                self.test_expr(x)
            }
            TestExpr::Word(w) => self.word(w, true),
        }
    }

    // ------------------------------------------------------------ commands

    fn assign(&mut self, prefix: &str, a: &Assign) -> TResult {
        let prefix = if a.append { format!("{prefix} -a") } else { prefix.to_string() };
        if a.naked {
            self.str(&format!("set{prefix} {} ${}", a.name, a.name));
        } else if let Some(elems) = &a.array {
            self.str(&format!("set{prefix} {}", a.name));
            for el in elems {
                self.str(" ");
                self.word(el, false)?;
            }
        } else if let Some(idx) = &a.index {
            // EXT: a[N]=v → set a[N+1] v
            let Some(n) = idx.as_lit().and_then(|l| l.parse::<usize>().ok()) else { return unsupported("an array index that is not a number") };
            self.str(&format!("set{prefix} {}[{}] ", a.name, n + 1));
            match &a.value {
                Some(v) => self.word_assign_value(v)?,
                None => self.str("''"),
            }
        } else {
            self.str(&format!("set{prefix} {} ", a.name));
            match &a.value {
                Some(v) => self.word_assign_value(v)?,
                None => self.str("''"), // EXT: babelfish writes nothing here
            }
        }
        Ok(())
    }

    /// An assignment value: quoted like babelfish, except a leading ~ still expands (EXT).
    fn word_assign_value(&mut self, w: &Word) -> TResult {
        if let Some(Part::Lit(l)) = w.parts.first() {
            if let Some(rest) = l.strip_prefix('~') {
                let user_end = rest.find('/').unwrap_or(rest.len());
                if rest[..user_end].chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
                    self.str(&format!("~{}", &rest[..user_end]));
                    let tail = &rest[user_end..];
                    if !tail.is_empty() {
                        self.escaped(&unescape(tail));
                    }
                    for p in &w.parts[1..] {
                        self.part(p, true, false)?;
                    }
                    return Ok(());
                }
            }
        }
        self.word(w, true)
    }

    fn call(&mut self, assigns: &[Assign], args: &[Word]) -> TResult {
        if args.is_empty() {
            for (n, a) in assigns.iter().enumerate() {
                if n > 0 {
                    self.str("; ");
                }
                self.assign("", a)?;
            }
            return Ok(());
        }
        for a in assigns {
            self.str(&format!("{}=", a.name));
            if let Some(v) = &a.value {
                self.word(v, true)?;
            }
            self.str(" ");
        }
        let first = &args[0];
        let l = first.as_lit().unwrap_or_default();
        match l.as_str() {
            "shift" => {
                // EXT: shift N
                match args.get(1).and_then(|w| w.as_lit()).and_then(|n| n.parse::<usize>().ok()) {
                    Some(n) if n > 1 => self.str(&format!("set -e argv[1..{n}]")),
                    _ => self.str("set -e argv[1]"),
                }
                return Ok(());
            }
            "unset" => {
                let mut first_out = true;
                let mut func = false;
                for a in &args[1..] {
                    let s = a.as_lit().unwrap_or_default();
                    if s == "-f" {
                        func = true;
                        continue;
                    } else if s == "-v" {
                        func = false;
                        continue;
                    }
                    if !first_out {
                        self.str("; ");
                    }
                    first_out = false;
                    self.str(if func { "functions -e " } else { "set -e " });
                    self.word(a, false)?;
                }
                return Ok(());
            }
            "hash" => {
                self.str("true");
                return Ok(());
            }
            "source" | "." if args.len() == 2 => {
                self.str(&self.location.clone());
                self.str(" < ");
                self.word(&args[1], false)?;
                self.str(" | source");
                return Ok(());
            }
            // EXT: eval "$(cmd)" evaluates bash; translate what cmd prints, then run it
            "eval" if args.len() == 2 => {
                let subst = match args[1].parts.as_slice() {
                    [Part::CmdSubst(s)] => Some(s),
                    [Part::Dbl(inner)] => match inner.as_slice() {
                        [Part::CmdSubst(s)] => Some(s),
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(stmts) = subst {
                    self.stmts(stmts)?;
                    self.str(&format!(" | {} | source", self.location));
                    return Ok(());
                }
            }
            // EXT: bash `set -e`, `set -o pipefail` … would erase variables in fish
            "set" if args.get(1).and_then(|w| w.as_lit()).is_some_and(|a| (a.starts_with('-') || a.starts_with('+')) && a != "--") => {
                return unsupported("bash `set` options (fish has no set -e / -u / -o pipefail)");
            }
            // EXT: fish's read has no -r (it never mangles backslashes) and spells the prompt -P
            "read" => {
                self.str("read");
                let mut k = 1;
                while k < args.len() {
                    let a = args[k].as_lit();
                    match a.as_deref() {
                        Some(f) if f.starts_with('-') && f.len() > 1 && f != "--" => {
                            let mut flags = String::new();
                            for (j, c) in f[1..].chars().enumerate() {
                                match c {
                                    'r' | 'e' => {}
                                    's' | 'a' => flags.push(c),
                                    'p' | 'd' | 'n' if j == f.len() - 2 => {
                                        let fl = match c { 'p' => " -P ", 'd' => " -d ", _ => " --nchars " };
                                        if !flags.is_empty() {
                                            self.str(&format!(" -{flags}"));
                                            flags.clear();
                                        }
                                        self.str(fl);
                                        k += 1;
                                        let Some(v) = args.get(k) else { return unsupported("read option without a value") };
                                        self.word(v, false)?;
                                    }
                                    c => return unsupported(&format!("read -{c}")),
                                }
                            }
                            if !flags.is_empty() {
                                self.str(&format!(" -{flags}"));
                            }
                        }
                        _ => {
                            self.str(" ");
                            self.word(&args[k], false)?;
                        }
                    }
                    k += 1;
                }
                return Ok(());
            }
            _ => self.word(first, false)?,
        }
        for a in &args[1..] {
            self.str(" ");
            self.word(a, false)?;
        }
        Ok(())
    }

    fn decl(&mut self, variant: &str, args: &[DeclArg]) -> TResult {
        // EXT: declare/typeset/readonly, and harmless flags (babelfish: export and local only)
        let mut prefix = match variant {
            "export" => " -gx".to_string(),
            "local" => " -l".to_string(),
            "declare" | "typeset" => if self.in_function > 0 { " -l".into() } else { " -g".into() },
            "readonly" => " -g".to_string(),
            v => return unsupported(v),
        };
        let mut n = 0;
        for a in args {
            match a {
                DeclArg::Word(w) => {
                    let Some(f) = w.as_lit().filter(|f| f.starts_with('-')) else { return unsupported(&format!("{variant} with this argument")) };
                    for c in f[1..].chars() {
                        match (variant, c) {
                            (_, 'a' | 'r' | 'i') => {}
                            ("declare" | "typeset" | "local", 'x') => prefix = format!("{prefix}x"),
                            ("declare" | "typeset", 'g') => prefix = " -g".into(),
                            _ => return unsupported(&format!("{variant} -{c}")),
                        }
                    }
                }
                DeclArg::Assign(a) => {
                    if n > 0 {
                        self.str("; ");
                    }
                    n += 1;
                    self.assign(&prefix, a)?;
                }
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------ words

    fn word(&mut self, w: &Word, must_quote: bool) -> TResult {
        // EXT: "$@" / "${a[@]}" are separate words in bash; "$argv" would join them
        if let [Part::Dbl(inner)] = w.parts.as_slice() {
            if let [Part::Param(p)] = inner.as_slice() {
                let all = p.param == "@" && p.exp.is_none() && !p.length && p.index.is_none() && p.slice.is_none()
                    || p.index.as_ref().and_then(|i| i.as_lit()).as_deref() == Some("@") && !p.length && p.exp.is_none() && p.repl.is_none();
                if all {
                    return self.param(p, false);
                }
            }
        }
        for p in &w.parts {
            self.part(p, must_quote, false)?;
        }
        Ok(())
    }

    fn part(&mut self, p: &Part, quoted: bool, in_dq: bool) -> TResult {
        match p {
            Part::Lit(s) => {
                if quoted {
                    self.escaped(&unescape(s));
                } else if let (false, Some((a, b, seq))) = (in_dq, brace_range(s)) {
                    // EXT: {1..3} → (seq 1 3)
                    self.str(&s[..a]);
                    self.str(&seq);
                    self.part(&Part::Lit(s[b..].to_string()), false, false)?;
                } else {
                    self.str(s);
                }
            }
            Part::Sgl(s) => self.escaped(s),
            Part::AnsiC(raw) => self.escaped(&ansi_c(raw)), // EXT: decode the escapes
            Part::Dbl(parts) => {
                if parts.is_empty() {
                    self.str("''");
                }
                for p in parts {
                    self.part(p, true, true)?;
                }
            }
            Part::Param(pe) => self.param(pe, quoted)?,
            Part::CmdSubst(stmts) => {
                self.str("(");
                // EXT: $(< file) → (cat file)
                if let [Stmt { cmd: Cmd::Call { assigns, args }, redirs, .. }] = stmts.as_slice() {
                    if assigns.is_empty() && args.is_empty() && redirs.len() == 1 && redirs[0].op == "<" {
                        self.str("cat ");
                        self.word(&redirs[0].word, false)?;
                        if quoted {
                            self.str(" | string collect; or echo");
                        }
                        self.str(")");
                        return Ok(());
                    }
                }
                self.stmts(stmts)?;
                if quoted {
                    self.str(" | string collect; or echo");
                }
                self.str(")");
            }
            Part::Arith(e) => self.arith(e, ArithRet::Value)?,
            Part::ProcSubst { out, stmts } => {
                if *out {
                    return unsupported(">(…) process substitution");
                }
                self.str("(");
                self.stmts(stmts)?;
                self.str(" | psub)");
            }
            Part::ExtGlob(g) => return unsupported(&format!("extended glob {g}")),
        }
        Ok(())
    }

    fn param(&mut self, p: &ParamExp, mut quoted: bool) -> TResult {
        let mut param = p.param.clone();
        let plain = !p.excl && !p.length && p.index.is_none() && p.slice.is_none() && p.repl.is_none() && p.exp.is_none() && !p.names;
        if let Some(expr) = literal_variable(&param) {
            self.str(expr);
            return Ok(());
        }
        if !param.is_empty() && param.chars().all(|c| c.is_ascii_digit()) {
            if param == "0" {
                self.str("(status filename)"); // EXT: babelfish writes $argv[0], which fish rejects
                return Ok(());
            }
            if plain {
                self.str(&format!("$argv[{param}]"));
                return Ok(());
            }
            param = format!("argv[{param}]"); // EXT: ${1:-x} and friends keep their operator
        }
        if param == "#" && plain {
            self.str("(count $argv)"); // EXT
            return Ok(());
        }
        if let Some(spec) = special_variable(&param) {
            if param == "*" {
                quoted = true;
            }
            param = spec.to_string();
        }
        if p.excl {
            // EXT: ${!name} → $$name
            if !plain && (p.length || p.index.is_some() || p.slice.is_some() || p.repl.is_some() || p.exp.is_some()) {
                return unsupported("${!name} with an operator");
            }
            self.str(&if quoted { format!("\"$${param}\"") } else { format!("$${param}") });
            return Ok(());
        }
        if p.names {
            return unsupported("${!prefix*}");
        }
        if p.length {
            let mut idx = p.index.as_ref().and_then(|i| i.as_lit());
            if p.param == "@" || p.param == "*" {
                idx = Some("@".into());
            }
            match idx.as_deref() {
                Some("@") | Some("*") => self.str(&format!("(count ${param})")),
                Some(_) => return unsupported("${#a[i]}"),
                None => self.str(&format!("(string length \"${param}\")")),
            }
            return Ok(());
        }
        if let Some(idx) = &p.index {
            match idx.as_lit().as_deref() {
                Some("@") => self.str(&format!("${param}")),
                Some("*") => self.str(&format!("\"${param}\"")),
                // EXT: ${a[N]} → $a[N+1] (bash counts from 0, fish from 1)
                Some(n) if n.parse::<usize>().is_ok() => {
                    let i = n.parse::<usize>().unwrap() + 1;
                    self.str(&if quoted { format!("\"${param}[{i}]\"") } else { format!("${param}[{i}]") });
                }
                _ => return unsupported("an array index that is not a number"),
            }
            return Ok(());
        }
        if let Some((off, len)) = &p.slice {
            // EXT: ${@:2} / ${@:2:3} → $argv[2..] / $argv[2..4]; ${x:1:2} → string sub
            let num = |w: &Word| w.as_lit().and_then(|l| l.trim().parse::<i64>().ok());
            let (Some(o), l) = (num(off), len.as_ref().map(num)) else { return unsupported("${x:offset} with a computed offset") };
            if o < 0 || matches!(l, Some(None)) || l.flatten().is_some_and(|l| l < 0) {
                return unsupported("${x:offset:length} with negative or computed numbers");
            }
            let l = l.flatten();
            if param == "argv" {
                match l {
                    Some(0) => self.str("''"),
                    Some(l) => self.str(&format!("$argv[{}..{}]", o.max(1), o.max(1) + l - 1)),
                    None => self.str(&format!("$argv[{}..]", o.max(1))),
                }
            } else {
                self.str(&format!("(string sub -s {}", o + 1));
                if let Some(l) = l {
                    self.str(&format!(" -l {l}"));
                }
                self.str(&format!(" -- \"${param}\")"));
            }
            return Ok(());
        }
        if let Some(r) = &p.repl {
            let pat = r.orig.as_lit().map(|l| unescape(&l));
            let globby = pat.as_ref().is_some_and(|l| l.contains(['*', '?', '[']));
            if r.anchor.is_some() || globby {
                // EXT: ${x/#pre/y} ${x/%suf/y} and glob patterns → regex
                let Some(l) = pat else { return unsupported("a computed ${x/pattern/…}") };
                let Ok(re) = pattern::regexp(&l, false) else { return unsupported("this ${x/pattern/…}") };
                let re = re.trim_start_matches("(?s)").to_string();
                let re = match r.anchor {
                    Some('#') => format!("^({re})"),
                    Some('%') => format!("({re})$"),
                    _ => re,
                };
                self.str("(string replace -r ");
                if r.all {
                    self.str("--all ");
                }
                self.escaped(&re);
                self.str(" ");
                match &r.with {
                    Some(w) => self.word(w, true)?,
                    None => self.str("''"),
                }
                self.str(&format!(" \"${param}\")"));
                return Ok(());
            }
            self.str("(string replace ");
            if r.all {
                self.str("--all ");
            }
            self.word(&r.orig, true)?;
            self.str(" ");
            match &r.with {
                Some(w) => self.word(w, true)?,
                None => self.str("''"),
            }
            self.str(&format!(" \"${param}\")"));
            return Ok(());
        }
        if let Some((op, w)) = &p.exp {
            let empty = Word::default();
            let word = w.as_ref().unwrap_or(&empty);
            let word_or_quotes = |t: &mut Self| if w.is_some() { t.word(word, true) } else { t.str("''"); Ok(()) };
            match op.as_str() {
                ":+" => {
                    self.str(&format!("(test -n \"${param}\" && echo "));
                    word_or_quotes(self)?;
                    self.str(" || echo)");
                }
                "+" => {
                    self.str(&format!("(set -q {param} && echo "));
                    word_or_quotes(self)?;
                    self.str(" || echo)");
                }
                ":-" => {
                    self.str(&format!("(test -n \"${param}\" && echo \"${param}\" || echo "));
                    word_or_quotes(self)?;
                    self.str(")");
                }
                "-" => {
                    self.str(&format!("(set -q {param} && echo \"${param}\" || echo "));
                    word_or_quotes(self)?;
                    self.str(")");
                }
                "#" | "##" | "%" | "%%" => {
                    let is_path = param.ends_with("PATH");
                    let suffix = op.starts_with('%');
                    let small = op.len() == 1;
                    let Some(pat) = word.as_lit() else {
                        // EXT: ${d#$HOME} — a computed but glob-free pattern is matched literally
                        let Some(lit) = self.regex_literal(word)? else { return unsupported("a computed ${x#pattern} with globs") };
                        self.str("(string replace -r -- ");
                        if suffix {
                            self.str(&format!("{lit}'$'"));
                        } else {
                            self.str(&format!("'^'{lit}"));
                        }
                        self.str(&format!(" '' \"${param}\")"));
                        return Ok(());
                    };
                    let pat = unescape(&pat);
                    let Ok(expr) = pattern::regexp(&pat, small) else { return unsupported("this ${x#pattern}") };
                    let expr = expr.trim_start_matches("(?s)").to_string();
                    let dot = if is_path && (suffix && expr.ends_with(':') || !suffix && expr.starts_with(':')) { "\\.?" } else { "" };
                    let expr = if suffix { format!("({expr}{dot})$") } else { format!("^({dot}{expr})") };
                    self.str("(string replace -r ");
                    self.escaped(&expr);
                    self.str(&format!(" '' \"${param}\")"));
                }
                // EXT: case conversion
                "^^" => self.str(&format!("(string upper -- \"${param}\")")),
                ",," => self.str(&format!("(string lower -- \"${param}\")")),
                op => return unsupported(&format!("${{x{op}…}}")),
            }
            return Ok(());
        }
        if quoted {
            self.str(&format!("\"${param}\""));
        } else {
            self.str(&format!("${param}"));
        }
        Ok(())
    }

    /// fish code for a regex matching `w` literally (no glob characters allowed), or None.
    fn regex_literal(&mut self, w: &Word) -> Result<Option<String>, String> {
        let mut out = String::new();
        let flat: Vec<&Part> = w.parts.iter().flat_map(|p| match p {
            Part::Dbl(inner) => inner.iter().collect::<Vec<_>>(),
            p => vec![p],
        }).collect();
        for p in flat {
            match p {
                Part::Lit(l) => {
                    let l = unescape(l);
                    if l.contains(['*', '?', '[']) {
                        return Ok(None);
                    }
                    let mut q = String::new();
                    for c in l.chars() {
                        if "\\.+*?()|[]{}^$".contains(c) {
                            q.push('\\');
                        }
                        q.push(c);
                    }
                    out.push('\'');
                    out.push_str(&q.replace('\\', "\\\\").replace('\'', "\\'"));
                    out.push('\'');
                }
                Part::Sgl(t) => {
                    out.push_str(&format!("(string escape --style=regex -- '{}')", t.replace('\\', "\\\\").replace('\'', "\\'")));
                }
                Part::Param(pe) => {
                    let old = std::mem::take(&mut self.buf);
                    let r = self.param(pe, true);
                    let inner = std::mem::replace(&mut self.buf, old);
                    r?;
                    out.push_str(&format!("(string escape --style=regex -- {inner})"));
                }
                _ => return Ok(None),
            }
        }
        Ok(Some(out))
    }

    // ------------------------------------------------------------ arithmetic

    /// EXT: an arithmetic expression as text for fish's `math`, or None if math can't say it.
    fn math_text(&self, e: &Arith) -> Option<String> {
        Some(match e {
            // "$x" inside arithmetic is just $x
            Arith::Word(Word { parts }) if matches!(parts.as_slice(), [Part::Dbl(_)]) => {
                let [Part::Dbl(inner)] = parts.as_slice() else { return None };
                return self.math_text(&Arith::Word(Word { parts: inner.clone() }));
            }
            Arith::Word(w) => match w.parts.as_slice() {
                [Part::Lit(l)] if valid_name(l) => {
                    if literal_variable(l).is_some() {
                        return None;
                    }
                    format!("${l}")
                }
                [Part::Lit(l)] if l.chars().all(|c| c.is_ascii_hexdigit() || c == 'x' || c == 'X') => l.clone(),
                [Part::Param(p)] if p.short || (p.exp.is_none() && p.index.is_none() && !p.length && p.slice.is_none() && p.repl.is_none()) => {
                    if p.param.chars().all(|c| c.is_ascii_digit()) {
                        format!("$argv[{}]", p.param)
                    } else if p.param == "#" {
                        return None;
                    } else if let Some(s) = special_variable(&p.param) {
                        format!("${s}")
                    } else if valid_name(&p.param) && !p.excl {
                        format!("${}", p.param)
                    } else {
                        return None;
                    }
                }
                _ => return None,
            },
            Arith::Paren(x) => format!("({})", self.math_text(x)?),
            Arith::Un(op, x, false) if op == "-" || op == "+" => format!("{op}{}", self.math_text(x)?),
            Arith::Bin(op, x, y) => {
                let o = match op.as_str() {
                    "+" | "-" | "*" | "/" | "%" => op.as_str(),
                    "**" => "^",
                    _ => return None,
                };
                format!("{} {o} {}", self.math_text(x)?, self.math_text(y)?)
            }
            _ => return None,
        })
    }

    fn math(&mut self, e: &Arith) -> TResult {
        let Some(t) = self.math_text(e) else { return unsupported("this arithmetic") };
        // integer arithmetic, like bash: -s0 truncates
        if t.contains('/') {
            self.str(&format!("(math -s0 \"{t}\")"));
        } else {
            self.str(&format!("(math \"{t}\")"));
        }
        Ok(())
    }

    fn arith_operand(&mut self, e: &Arith) -> TResult {
        if let Arith::Word(w) = e {
            if let Some(l) = w.as_lit() {
                if valid_name(&l) {
                    match literal_variable(&l) {
                        Some(expr) => self.str(expr),
                        None => self.str(&format!("\"${l}\"")),
                    }
                } else {
                    self.str(&l);
                }
                return Ok(());
            }
        }
        // EXT: anything else is computed with math
        self.math(e)
    }

    fn arith(&mut self, e: &Arith, ret: ArithRet) -> TResult {
        match e {
            Arith::Bin(op, x, y) if op == "==" || op == "!=" || ret == ArithRet::Status && matches!(op.as_str(), "<" | ">" | "<=" | ">=")
                || ret == ArithRet::Value && matches!(op.as_str(), "<" | ">" | "<=" | ">=") =>
            {
                let t = match op.as_str() { "==" => "-eq", "!=" => "-ne", "<" => "-lt", ">" => "-gt", "<=" => "-le", _ => "-ge" };
                if ret == ArithRet::Value {
                    self.str("(");
                }
                self.str("test ");
                self.arith_operand(x)?;
                self.str(&format!(" {t} "));
                self.arith_operand(y)?;
                if ret == ArithRet::Value {
                    self.str("; and echo 1; or echo 0)");
                }
                Ok(())
            }
            // EXT: (( a && b )), (( a || b )) as statuses
            Arith::Bin(op, x, y) if ret == ArithRet::Status && (op == "&&" || op == "||") => {
                self.arith(x, ret)?;
                self.str(if op == "&&" { " && " } else { " || " });
                self.arith(y, ret)
            }
            // EXT: assignments, ++ and -- as statements
            Arith::Bin(op, x, y) if ret == ArithRet::Status && op.ends_with('=') && !matches!(op.as_str(), "==" | "!=" | "<=" | ">=") => {
                let Arith::Word(w) = &**x else { return unsupported("assigning to this") };
                let Some(name) = w.as_lit().filter(|n| valid_name(n)) else { return unsupported("assigning to this") };
                self.str(&format!("set {name} "));
                if op == "=" {
                    match y.as_ref() {
                        Arith::Word(v) if v.as_lit().is_some_and(|l| l.chars().all(|c| c.is_ascii_digit())) => self.str(&v.as_lit().unwrap()),
                        _ => self.math(y)?,
                    }
                } else {
                    let bin = Arith::Bin(op.trim_end_matches('=').to_string(), x.clone(), Box::new(Arith::Paren(y.clone())));
                    self.math(&bin)?;
                }
                Ok(())
            }
            Arith::Bin(op, x, y) if ret == ArithRet::Status && op == "," => {
                self.arith(x, ret)?;
                self.str("; ");
                self.arith(y, ret)
            }
            Arith::Un(op, x, _) if ret == ArithRet::Status && (op == "++" || op == "--") => {
                let Arith::Word(w) = &**x else { return unsupported("++ on this") };
                let Some(name) = w.as_lit().filter(|n| valid_name(n)) else { return unsupported("++ on this") };
                self.str(&format!("set {name} (math \"${name} {} 1\")", &op[..1]));
                Ok(())
            }
            Arith::Un(op, x, false) if ret == ArithRet::Status && op == "!" => {
                self.str("not ");
                self.arith(x, ret)
            }
            Arith::Paren(x) if ret == ArithRet::Status => self.arith(x, ret),
            Arith::Word(w) => {
                let Some(l) = w.as_lit() else {
                    // EXT: $x and friends
                    if ret == ArithRet::Status {
                        self.str("test ");
                        self.math(e)?;
                        self.str(" != 0");
                        return Ok(());
                    }
                    return self.math(e);
                };
                if ret == ArithRet::Status {
                    self.str("test ");
                }
                if valid_name(&l) {
                    match literal_variable(&l) {
                        Some(expr) => self.str(expr),
                        None => self.str(&format!("\"${l}\"")),
                    }
                } else {
                    self.str(&l);
                }
                if ret == ArithRet::Status {
                    self.str(" != 0");
                }
                Ok(())
            }
            _ => {
                if ret == ArithRet::Status {
                    self.str("test ");
                    self.math(e)?;
                    self.str(" != 0");
                    Ok(())
                } else {
                    self.math(e)
                }
            }
        }
    }
}
