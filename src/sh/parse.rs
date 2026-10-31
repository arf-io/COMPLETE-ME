//! A bash parser: recursive descent straight over the characters. It produces the tree in
//! `ast.rs`, attaching comments the way mvdan.cc/sh does (a statement owns the comments before
//! it plus a trailing one on its last line), so translations match babelfish's.
use super::ast::*;

pub type PResult<T> = Result<T, String>;

const CLOSERS: &[&str] = &["then", "elif", "else", "fi", "do", "done", "esac", "}"];
const DECLS: &[&str] = &["export", "local", "declare", "typeset", "readonly", "nameref"];

#[derive(Clone, Copy, PartialEq)]
enum Ctx {
    Normal,
    /// inside `[[ … ]]`: ( ) < > are operators
    Test,
    /// right of `=~`: ( ) | are part of the regex
    Regex,
    /// a case pattern: | and ) end it
    Pattern,
}

struct Pending {
    delim: String,
    strip_tabs: bool,
    quoted: bool,
    idx: usize,
}

pub struct Parser {
    s: Vec<char>,
    i: usize,
    line_starts: Vec<usize>,
    /// comments read but not yet attached: (line, text)
    acc: Vec<(usize, String)>,
    pending: Vec<Pending>,
    heredocs: Vec<Word>,
    /// index of the last character of the last token read
    tok_end: usize,
}

pub fn parse(src: &str) -> PResult<File> {
    let mut p = Parser::new(src);
    let stmts = p.stmt_list(&[], false, false)?;
    p.skip_space_nl()?;
    if p.i < p.s.len() {
        return Err(p.err("unexpected input"));
    }
    if !p.pending.is_empty() {
        let d = p.pending[0].delim.clone();
        p.read_heredocs()?;
        if p.heredocs.len() < p.pending.len() {
            return Err(format!("here-document delimited by {d:?} is never closed"));
        }
    }
    let last = p.acc.drain(..).map(|(_, t)| t).collect();
    Ok(File { stmts, last, heredocs: p.heredocs })
}

impl Parser {
    fn new(src: &str) -> Parser {
        let s: Vec<char> = src.chars().collect();
        let mut line_starts = vec![0];
        for (i, c) in s.iter().enumerate() {
            if *c == '\n' {
                line_starts.push(i + 1);
            }
        }
        Parser { s, i: 0, line_starts, acc: vec![], pending: vec![], heredocs: vec![], tok_end: 0 }
    }

    // ------------------------------------------------------------ low level

    fn peek(&self, k: usize) -> Option<char> {
        self.s.get(self.i + k).copied()
    }
    fn at(&self, lit: &str) -> bool {
        lit.chars().enumerate().all(|(k, c)| self.peek(k) == Some(c))
    }
    fn eof(&self) -> bool {
        self.i >= self.s.len()
    }
    fn line_of(&self, idx: usize) -> usize {
        match self.line_starts.binary_search(&idx) {
            Ok(n) => n + 1,
            Err(n) => n,
        }
    }
    fn line(&self) -> usize {
        self.line_of(self.i)
    }
    fn err(&self, msg: &str) -> String {
        let near: String = self.s[self.i.min(self.s.len())..].iter().take(20).collect();
        format!("line {}: {msg}{}", self.line(), if near.is_empty() { String::new() } else { format!(" near {near:?}") })
    }
    fn bump(&mut self, n: usize) {
        self.i += n;
        self.tok_end = self.i.saturating_sub(1);
    }

    /// Spaces, tabs and backslash-newline continuations.
    fn skip_blanks(&mut self) {
        loop {
            match self.peek(0) {
                Some(' ') | Some('\t') | Some('\r') => self.i += 1,
                Some('\\') if self.peek(1) == Some('\n') => self.i += 2,
                _ => break,
            }
        }
    }
    /// Blanks, then a comment to the end of the line if one starts here.
    fn skip_blanks_comments(&mut self) {
        self.skip_blanks();
        if self.peek(0) == Some('#') {
            let line = self.line();
            let start = self.i + 1;
            while !self.eof() && self.peek(0) != Some('\n') {
                self.i += 1;
            }
            let text: String = self.s[start..self.i].iter().collect();
            self.acc.push((line, text.trim_end_matches('\r').to_string()));
        }
    }
    /// Consume one newline, then any here-document bodies it starts.
    fn newline(&mut self) -> PResult<()> {
        self.i += 1;
        if !self.pending.is_empty() {
            self.read_heredocs()?;
        }
        Ok(())
    }
    fn skip_space_nl(&mut self) -> PResult<()> {
        loop {
            self.skip_blanks_comments();
            if self.peek(0) == Some('\n') {
                self.newline()?;
            } else {
                return Ok(());
            }
        }
    }

    fn read_heredocs(&mut self) -> PResult<()> {
        let pending = std::mem::take(&mut self.pending);
        for p in pending {
            let mut body = String::new();
            let mut closed = false;
            while !self.eof() {
                let start = self.i;
                while !self.eof() && self.peek(0) != Some('\n') {
                    self.i += 1;
                }
                let mut line: String = self.s[start..self.i].iter().collect();
                if !self.eof() {
                    self.i += 1;
                }
                if p.strip_tabs {
                    line = line.trim_start_matches('\t').to_string();
                }
                if line == p.delim {
                    closed = true;
                    break;
                }
                body.push_str(&line);
                body.push('\n');
            }
            if !closed {
                return Err(format!("here-document delimited by {:?} is never closed", p.delim));
            }
            let w = if p.quoted {
                Word { parts: vec![Part::Lit(body)] }
            } else {
                let mut sub = Parser::new(&body);
                Word { parts: sub.dq_parts(true)? }
            };
            while self.heredocs.len() <= p.idx {
                self.heredocs.push(Word::default());
            }
            self.heredocs[p.idx] = w;
        }
        Ok(())
    }

    fn is_delim(c: Option<char>) -> bool {
        matches!(c, None | Some(' ' | '\t' | '\r' | '\n' | ';' | '&' | '|' | '<' | '>' | '(' | ')'))
    }

    /// The plain word at the cursor, if it is one (no quotes or expansions), without consuming it.
    fn peek_plain(&self) -> Option<String> {
        let mut k = 0;
        let mut w = String::new();
        while let Some(c) = self.peek(k) {
            if Self::is_delim(Some(c)) {
                break;
            }
            if "'\"\\$`".contains(c) {
                return None;
            }
            w.push(c);
            k += 1;
        }
        (!w.is_empty()).then_some(w)
    }
    /// A reserved word in command position.
    fn peek_reserved(&self) -> Option<String> {
        if self.at("((") {
            return Some("((".into());
        }
        if self.peek(0) == Some('(') {
            return Some("(".into());
        }
        let w = self.peek_plain()?;
        let reserved = ["if", "then", "elif", "else", "fi", "do", "done", "case", "esac", "while", "until", "for", "in",
            "function", "select", "time", "coproc", "{", "}", "[[", "!", "let"];
        reserved.contains(&w.as_str()).then_some(w)
    }
    fn eat_reserved(&mut self, w: &str) -> bool {
        if self.peek_reserved().as_deref() == Some(w) {
            self.bump(w.chars().count());
            true
        } else {
            false
        }
    }
    fn expect_reserved(&mut self, w: &str) -> PResult<()> {
        self.skip_space_nl()?;
        if self.eat_reserved(w) {
            Ok(())
        } else {
            Err(self.err(&format!("expected {w:?}")))
        }
    }

    fn take_comments(&mut self) -> Vec<String> {
        self.acc.drain(..).map(|(_, t)| t).collect()
    }

    // ------------------------------------------------------------ statements

    fn at_case_end(&self) -> bool {
        self.at(";;") || self.at(";&")
    }

    fn stmt_list(&mut self, stops: &[&str], close_paren: bool, in_case: bool) -> PResult<Vec<Stmt>> {
        let mut out = vec![];
        loop {
            self.skip_space_nl()?;
            if self.eof() || close_paren && self.peek(0) == Some(')') || in_case && self.at_case_end() {
                break;
            }
            if let Some(w) = self.peek_reserved() {
                if stops.contains(&w.as_str()) {
                    break;
                }
                if CLOSERS.contains(&w.as_str()) {
                    return Err(self.err(&format!("unexpected {w:?}")));
                }
            }
            if self.peek(0) == Some(')') {
                return Err(self.err("unexpected ')'"));
            }
            if self.at_case_end() {
                return Err(self.err("';;' outside a case"));
            }
            let (s, terminated) = self.stmt(true, false)?;
            out.push(s);
            self.skip_blanks_comments();
            let ok = terminated
                || self.eof()
                || self.peek(0) == Some('\n')
                || close_paren && self.peek(0) == Some(')')
                || in_case && self.at_case_end()
                || self.peek_reserved().is_some_and(|w| stops.contains(&w.as_str()));
            if !ok {
                return Err(self.err("statements must be separated by &, ; or a newline"));
            }
        }
        Ok(out)
    }

    /// An and-or list, with its terminator when `read_end`. Returns (stmt, terminated).
    fn stmt(&mut self, read_end: bool, bin: bool) -> PResult<(Stmt, bool)> {
        let comments = self.take_comments();
        self.skip_blanks();
        let negated = if self.peek_plain().as_deref() == Some("!") {
            self.bump(1);
            self.skip_blanks();
            true
        } else {
            false
        };
        let mut s = self.pipeline(comments)?;
        if negated {
            s.negated = true;
        }
        loop {
            self.skip_blanks_comments();
            let op = if self.at("&&") { BinOp::And } else if self.at("||") { BinOp::Or } else { break };
            if bin {
                return Ok((s, false));
            }
            self.bump(2);
            self.skip_space_nl()?;
            let (y, _) = self.stmt(false, true)?;
            let comments = std::mem::take(&mut s.comments);
            let (line, end_line) = (s.line, y.end_line);
            s = Stmt { comments, negated: false, background: false, coprocess: false,
                cmd: Cmd::Binary { op, x: Box::new(s), y: Box::new(y) }, redirs: vec![], line, end_line };
        }
        let mut terminated = false;
        if read_end {
            self.skip_blanks_comments();
            if self.peek(0) == Some(';') && !self.at_case_end() {
                self.i += 1;
                terminated = true;
            } else if self.peek(0) == Some('&') && !self.at("&&") && !self.at("&>") {
                self.i += 1;
                s.background = true;
                terminated = true;
            }
        }
        if !bin {
            self.skip_blanks_comments();
            if self.acc.first().is_some_and(|(l, _)| *l == s.end_line) {
                let (_, c) = self.acc.remove(0);
                s.comments.push(c);
            }
        }
        Ok((s, terminated))
    }

    fn pipeline(&mut self, comments: Vec<String>) -> PResult<Stmt> {
        let mut s = self.command(comments)?;
        self.skip_blanks_comments();
        if self.peek(0) == Some('|') && !self.at("||") {
            let op = if self.at("|&") { BinOp::PipeAll } else { BinOp::Pipe };
            self.bump(if op == BinOp::PipeAll { 2 } else { 1 });
            self.skip_space_nl()?;
            let more = self.take_comments();
            let y = self.pipeline(more)?;
            let comments = std::mem::take(&mut s.comments);
            let (line, end_line) = (s.line, y.end_line);
            s = Stmt { comments, negated: false, background: false, coprocess: false,
                cmd: Cmd::Binary { op, x: Box::new(s), y: Box::new(y) }, redirs: vec![], line, end_line };
        }
        Ok(s)
    }

    fn new_stmt(&self, comments: Vec<String>, line: usize, cmd: Cmd) -> Stmt {
        Stmt { comments, negated: false, background: false, coprocess: false, cmd, redirs: vec![], line, end_line: line }
    }

    fn command(&mut self, comments: Vec<String>) -> PResult<Stmt> {
        self.skip_blanks();
        let line = self.line();
        let reserved = self.peek_reserved();
        let mut s = match reserved.as_deref() {
            Some("if") => {
                self.bump(2);
                let c = self.if_body()?;
                self.new_stmt(comments, line, Cmd::If(c))
            }
            Some(w @ ("while" | "until")) => {
                let until = w == "until";
                self.bump(w.len());
                let cond = self.stmt_list(&["do"], false, false)?;
                self.expect_reserved("do")?;
                let body = self.stmt_list(&["done"], false, false)?;
                self.expect_reserved("done")?;
                self.new_stmt(comments, line, Cmd::While { until, cond, body })
            }
            Some(w @ ("for" | "select")) => {
                let select = w == "select";
                self.bump(w.len());
                let mut st = self.new_stmt(comments, line, Cmd::Coproc);
                st.cmd = self.for_clause(select, &mut st.comments)?;
                st
            }
            Some("case") => {
                self.bump(4);
                let c = self.case_clause()?;
                self.new_stmt(comments, line, c)
            }
            Some("{") => {
                self.bump(1);
                let body = self.stmt_list(&["}"], false, false)?;
                self.expect_reserved("}")?;
                self.new_stmt(comments, line, Cmd::Block(body))
            }
            Some("((") => {
                let save = (self.i, self.acc.len());
                self.bump(2);
                match self.arith_until("))") {
                    Ok(a) => {
                        self.bump(2);
                        self.new_stmt(comments, line, Cmd::Arith(a))
                    }
                    Err(_) => {
                        // `((a) | b)`: a subshell holding a subshell
                        self.i = save.0;
                        self.acc.truncate(save.1);
                        self.subshell(comments, line)?
                    }
                }
            }
            Some("(") => self.subshell(comments, line)?,
            Some("[[") => {
                self.bump(2);
                let e = self.test_or()?;
                self.skip_space_nl()?;
                if !self.at("]]") {
                    return Err(self.err("expected ]]"));
                }
                self.bump(2);
                self.new_stmt(comments, line, Cmd::Test(e))
            }
            Some("function") => {
                self.bump(8);
                self.skip_blanks();
                let name = self.peek_plain().ok_or_else(|| self.err("expected a function name"))?;
                let name = name.trim_end_matches("()").to_string();
                self.bump(name.chars().count());
                self.skip_blanks();
                if self.at("()") {
                    self.bump(2);
                } else if self.peek(0) == Some('(') {
                    self.bump(1);
                    self.skip_blanks();
                    if self.peek(0) != Some(')') {
                        return Err(self.err("expected )"));
                    }
                    self.bump(1);
                }
                self.func_body(comments, line, name)?
            }
            Some("time") => {
                self.bump(4);
                self.skip_blanks();
                if self.at("-p ") {
                    self.bump(2);
                    self.skip_blanks();
                }
                let inner = if Self::is_delim(self.peek(0)) && !matches!(self.peek(0), Some('<' | '>' | '(')) {
                    None
                } else {
                    let c = self.take_comments();
                    Some(Box::new(self.pipeline(c)?))
                };
                self.new_stmt(comments, line, Cmd::Time(inner))
            }
            Some("coproc") => return Err(self.err("coproc is not supported")),
            Some("let") => {
                self.bump(3);
                let mut exprs = vec![];
                loop {
                    self.skip_blanks();
                    if Self::is_delim(self.peek(0)) {
                        break;
                    }
                    let w = self.word(Ctx::Normal)?.ok_or_else(|| self.err("expected an expression"))?;
                    let text = unquote_simple(&w).ok_or_else(|| self.err("let with expansions"))?;
                    let mut sub = Parser::new(&text);
                    exprs.push(sub.arith_until("")?);
                }
                self.new_stmt(comments, line, Cmd::Let(exprs))
            }
            Some(w) if CLOSERS.contains(&w) || w == "in" => return Err(self.err(&format!("unexpected {w:?}"))),
            _ => self.simple(comments, line)?,
        };
        // redirections after a compound command
        if !matches!(s.cmd, Cmd::Call { .. } | Cmd::Decl { .. }) {
            loop {
                self.skip_blanks();
                if self.redirect_ahead() {
                    let r = self.redirect()?;
                    s.redirs.push(r);
                } else {
                    break;
                }
            }
        }
        s.end_line = self.line_of(self.tok_end).max(line);
        Ok(s)
    }

    fn subshell(&mut self, comments: Vec<String>, line: usize) -> PResult<Stmt> {
        self.bump(1);
        let body = self.stmt_list(&[], true, false)?;
        self.skip_space_nl()?;
        if self.peek(0) != Some(')') {
            return Err(self.err("expected )"));
        }
        self.bump(1);
        Ok(self.new_stmt(comments, line, Cmd::Subshell(body)))
    }

    fn func_body(&mut self, comments: Vec<String>, line: usize, name: String) -> PResult<Stmt> {
        self.skip_space_nl()?;
        let c = self.take_comments();
        let body = self.command(c)?;
        if !matches!(body.cmd, Cmd::Block(_) | Cmd::Subshell(_) | Cmd::If(_) | Cmd::While { .. } | Cmd::For { .. }
            | Cmd::Case { .. } | Cmd::Test(_) | Cmd::Arith(_) | Cmd::CFor { .. }) {
            return Err(self.err("a function body must be a compound command"));
        }
        Ok(self.new_stmt(comments, line, Cmd::Func { name, body: Box::new(body) }))
    }

    fn if_body(&mut self) -> PResult<IfClause> {
        let cond = self.stmt_list(&["then"], false, false)?;
        self.expect_reserved("then")?;
        let then = self.stmt_list(&["elif", "else", "fi"], false, false)?;
        self.skip_space_nl()?;
        let els = if self.eat_reserved("elif") {
            let mut inner = self.if_body()?;
            inner.elif = true;
            return Ok(IfClause { cond, then, els: Some(Box::new(inner)), elif: false });
        } else if self.eat_reserved("else") {
            let body = self.stmt_list(&["fi"], false, false)?;
            Some(Box::new(IfClause { cond: vec![], then: body, els: None, elif: false }))
        } else {
            None
        };
        self.expect_reserved("fi")?;
        Ok(IfClause { cond, then, els, elif: false })
    }

    fn for_clause(&mut self, select: bool, comments: &mut Vec<String>) -> PResult<Cmd> {
        self.skip_blanks();
        if !select && self.at("((") {
            self.bump(2);
            let init = self.arith_opt(";")?;
            self.bump(1);
            let cond = self.arith_opt(";")?;
            self.bump(1);
            let post = self.arith_opt("))")?;
            self.bump(2);
            self.skip_blanks();
            if self.peek(0) == Some(';') {
                self.bump(1);
            }
            self.skip_space_nl()?;
            let (start, end) = if self.peek_plain().as_deref() == Some("{") { ("{", "}") } else { ("do", "done") };
            self.expect_reserved(start)?;
            let body = self.stmt_list(&[end], false, false)?;
            self.expect_reserved(end)?;
            return Ok(Cmd::CFor { init, cond, post, body });
        }
        let name = self.peek_plain().filter(|n| valid_name(n)).ok_or_else(|| self.err("expected a loop variable"))?;
        self.bump(name.len());
        self.skip_space_nl()?;
        let mut items = None;
        if self.eat_reserved("in") {
            let mut ws = vec![];
            loop {
                self.skip_blanks();
                if matches!(self.peek(0), None | Some(';' | '\n')) {
                    break;
                }
                match self.word(Ctx::Normal)? {
                    Some(w) => ws.push(w),
                    None => return Err(self.err("unexpected token in a for list")),
                }
            }
            items = Some(ws);
        }
        self.skip_blanks_comments();
        if self.peek(0) == Some(';') {
            self.bump(1);
        }
        self.skip_space_nl()?;
        let (start, end) = if self.peek_plain().as_deref() == Some("{") { ("{", "}") } else { ("do", "done") };
        self.expect_reserved(start)?;
        // comments in the header (after `do`) belong to the for statement, as in mvdan.cc/sh
        self.skip_blanks_comments();
        comments.extend(self.take_comments());
        let body = self.stmt_list(&[end], false, false)?;
        self.expect_reserved(end)?;
        Ok(Cmd::For { name, items, body, select })
    }

    fn case_clause(&mut self) -> PResult<Cmd> {
        self.skip_blanks();
        let word = self.word(Ctx::Normal)?.ok_or_else(|| self.err("case needs a word"))?;
        self.expect_reserved("in")?;
        let mut items = vec![];
        loop {
            self.skip_space_nl()?;
            if self.eat_reserved("esac") {
                break;
            }
            if self.eof() {
                return Err(self.err("expected esac"));
            }
            let _ = self.take_comments(); // case-item comments: babelfish drops them
            if self.peek(0) == Some('(') {
                self.bump(1);
            }
            let mut patterns = vec![];
            loop {
                self.skip_blanks();
                let w = self.word(Ctx::Pattern)?.ok_or_else(|| self.err("case patterns must be words"))?;
                patterns.push(w);
                self.skip_blanks();
                match self.peek(0) {
                    Some('|') => self.bump(1),
                    Some(')') => {
                        self.bump(1);
                        break;
                    }
                    _ => return Err(self.err("case patterns are separated by |")),
                }
            }
            let stmts = self.stmt_list(&["esac"], false, true)?;
            self.skip_space_nl()?;
            let op = if self.at(";;&") {
                ";;&"
            } else if self.at(";;") {
                ";;"
            } else if self.at(";&") {
                ";&"
            } else {
                items.push(CaseItem { patterns, stmts, op: ";;".into() });
                self.expect_reserved("esac")?;
                break;
            };
            self.bump(op.len());
            items.push(CaseItem { patterns, stmts, op: op.into() });
        }
        Ok(Cmd::Case { word, items })
    }

    // ------------------------------------------------------------ simple commands

    fn assignment_ahead(&self) -> bool {
        let mut k = 0;
        match self.peek(0) {
            Some(c) if c == '_' || c.is_ascii_alphabetic() => {}
            _ => return false,
        }
        while self.peek(k).is_some_and(|c| c == '_' || c.is_ascii_alphanumeric()) {
            k += 1;
        }
        if self.peek(k) == Some('[') {
            let mut depth = 0;
            while let Some(c) = self.peek(k) {
                if c == '[' {
                    depth += 1;
                } else if c == ']' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                } else if c == '\n' {
                    return false;
                }
                k += 1;
            }
            k += 1;
        }
        if self.peek(k) == Some('+') {
            k += 1;
        }
        self.peek(k) == Some('=')
    }

    fn assignment(&mut self) -> PResult<Assign> {
        let mut name = String::new();
        while let Some(c) = self.peek(0).filter(|c| *c == '_' || c.is_ascii_alphanumeric()) {
            name.push(c);
            self.i += 1;
        }
        let mut index = None;
        if self.peek(0) == Some('[') {
            self.i += 1;
            index = Some(self.inner_word(&[']'])?);
            self.i += 1;
        }
        let append = self.peek(0) == Some('+');
        if append {
            self.i += 1;
        }
        self.bump(1); // '='
        let mut a = Assign { name, append, naked: false, index, value: None, array: None };
        if self.peek(0) == Some('(') {
            self.bump(1);
            let mut elems = vec![];
            loop {
                self.skip_space_nl()?;
                let _ = self.take_comments();
                match self.peek(0) {
                    Some(')') => {
                        self.bump(1);
                        break;
                    }
                    None => return Err(self.err("unclosed array")),
                    _ => {}
                }
                if self.peek(0) == Some('[') {
                    return Err(self.err("array elements with [index]= are not supported"));
                }
                let w = self.word(Ctx::Normal)?.ok_or_else(|| self.err("bad array element"))?;
                elems.push(w);
            }
            a.array = Some(elems);
        } else if !Self::is_delim(self.peek(0)) {
            a.value = self.word(Ctx::Normal)?;
        }
        Ok(a)
    }

    fn redirect_ahead(&self) -> bool {
        let mut k = 0;
        while self.peek(k).is_some_and(|c| c.is_ascii_digit()) {
            k += 1;
        }
        match self.peek(k) {
            Some('<') | Some('>') => self.peek(k + 1) != Some('('),
            Some('&') if k == 0 => self.peek(1) == Some('>'),
            _ => false,
        }
    }

    fn redirect(&mut self) -> PResult<Redir> {
        let mut n = String::new();
        while let Some(c) = self.peek(0).filter(|c| c.is_ascii_digit()) {
            n.push(c);
            self.i += 1;
        }
        let ops = ["<<<", "<<-", "<<", "<>", "<&", "<", "&>>", "&>", ">>", ">&", ">|", ">"];
        let op = ops.iter().find(|o| self.at(o)).ok_or_else(|| self.err("bad redirection"))?.to_string();
        self.bump(op.len());
        self.skip_blanks();
        let word = self.word(Ctx::Normal)?.ok_or_else(|| self.err(&format!("{op} needs a word")))?;
        let mut hdoc = None;
        if op == "<<" || op == "<<-" {
            let (delim, quoted) = heredoc_delim(&word);
            let idx = self.heredocs.len() + self.pending.len();
            self.pending.push(Pending { delim, strip_tabs: op == "<<-", quoted, idx });
            hdoc = Some(idx);
        }
        Ok(Redir { n: (!n.is_empty()).then_some(n), op, word, hdoc })
    }

    fn simple(&mut self, comments: Vec<String>, line: usize) -> PResult<Stmt> {
        let mut assigns = vec![];
        let mut args: Vec<Word> = vec![];
        let mut redirs = vec![];
        let mut decl: Option<(String, Vec<DeclArg>)> = None;
        loop {
            self.skip_blanks();
            if self.peek(0) == Some('#') {
                break; // a comment ends the command; skip_blanks_comments picks it up
            }
            if self.redirect_ahead() {
                redirs.push(self.redirect()?);
                continue;
            }
            if matches!(self.peek(0), None | Some('\n' | ';' | '|' | ')')) || self.peek(0) == Some('&') {
                break;
            }
            if let Some((_, dargs)) = decl.as_mut() {
                if self.assignment_ahead() {
                    let a = self.assignment()?;
                    dargs.push(DeclArg::Assign(a));
                } else if let Some(n) = self.peek_plain().filter(|n| valid_name(n)) {
                    self.bump(n.len());
                    dargs.push(DeclArg::Assign(Assign { name: n, append: false, naked: true, index: None, value: None, array: None }));
                } else {
                    let w = self.word(Ctx::Normal)?.ok_or_else(|| self.err("unexpected token"))?;
                    dargs.push(DeclArg::Word(w));
                }
                continue;
            }
            if args.is_empty() && self.assignment_ahead() {
                assigns.push(self.assignment()?);
                continue;
            }
            if args.is_empty() && assigns.is_empty() {
                if let Some(d) = self.peek_plain().filter(|w| DECLS.contains(&w.as_str())) {
                    self.bump(d.len());
                    decl = Some((d, vec![]));
                    continue;
                }
            }
            let Some(w) = self.word(Ctx::Normal)? else {
                return Err(self.err("unexpected token"));
            };
            // name() { … }
            if args.is_empty() && assigns.is_empty() && redirs.is_empty() {
                if let Some(name) = w.as_lit().filter(|n| !n.is_empty() && !n.contains('=')) {
                    let save = self.i;
                    self.skip_blanks();
                    if self.peek(0) == Some('(') {
                        self.i += 1;
                        self.skip_blanks();
                        if self.peek(0) == Some(')') {
                            self.bump(1);
                            return self.func_body(comments, line, name);
                        }
                    }
                    self.i = save;
                }
            }
            args.push(w);
        }
        let cmd = match decl {
            Some((variant, dargs)) => Cmd::Decl { variant, args: dargs },
            None => {
                if args.is_empty() && assigns.is_empty() && redirs.is_empty() {
                    return Err(self.err("expected a command"));
                }
                Cmd::Call { assigns, args }
            }
        };
        let mut s = self.new_stmt(comments, line, cmd);
        s.redirs = redirs;
        Ok(s)
    }

    // ------------------------------------------------------------ words

    fn flush(lit: &mut String, parts: &mut Vec<Part>) {
        if !lit.is_empty() {
            parts.push(Part::Lit(std::mem::take(lit)));
        }
    }

    fn word(&mut self, ctx: Ctx) -> PResult<Option<Word>> {
        let mut parts = vec![];
        let mut lit = String::new();
        let mut depth = 0usize;
        let start = self.i;
        while let Some(c) = self.peek(0) {
            match c {
                ' ' | '\t' | '\r' | '\n' | ';' | '&' => {
                    if ctx == Ctx::Regex && depth > 0 && c != '\n' {
                        lit.push(c);
                        self.i += 1;
                        continue;
                    }
                    break;
                }
                '|' => {
                    if ctx == Ctx::Regex {
                        lit.push(c);
                        self.i += 1;
                        continue;
                    }
                    break;
                }
                '<' | '>' => {
                    if self.peek(1) == Some('(') && ctx != Ctx::Test {
                        Self::flush(&mut lit, &mut parts);
                        let out = c == '>';
                        self.i += 2;
                        let stmts = self.stmt_list(&[], true, false)?;
                        if self.peek(0) != Some(')') {
                            return Err(self.err("unclosed process substitution"));
                        }
                        self.i += 1;
                        parts.push(Part::ProcSubst { out, stmts });
                        continue;
                    }
                    if ctx == Ctx::Regex {
                        lit.push(c);
                        self.i += 1;
                        continue;
                    }
                    break;
                }
                '(' => {
                    if ctx == Ctx::Regex {
                        depth += 1;
                        lit.push(c);
                        self.i += 1;
                        continue;
                    }
                    if lit.ends_with(['?', '*', '+', '@', '!']) {
                        let mut d = 0;
                        let mut raw = String::new();
                        while let Some(ch) = self.peek(0) {
                            raw.push(ch);
                            self.i += 1;
                            if ch == '(' {
                                d += 1;
                            } else if ch == ')' {
                                d -= 1;
                                if d == 0 {
                                    break;
                                }
                            }
                        }
                        let op = lit.pop().unwrap();
                        Self::flush(&mut lit, &mut parts);
                        parts.push(Part::ExtGlob(format!("{op}{raw}")));
                        continue;
                    }
                    break;
                }
                ')' => {
                    if ctx == Ctx::Regex && depth > 0 {
                        depth -= 1;
                        lit.push(c);
                        self.i += 1;
                        continue;
                    }
                    break;
                }
                '\'' => {
                    Self::flush(&mut lit, &mut parts);
                    self.i += 1;
                    let st = self.i;
                    while self.peek(0).is_some_and(|c| c != '\'') {
                        self.i += 1;
                    }
                    if self.eof() {
                        return Err(self.err("unclosed '"));
                    }
                    parts.push(Part::Sgl(self.s[st..self.i].iter().collect()));
                    self.i += 1;
                }
                '"' => {
                    Self::flush(&mut lit, &mut parts);
                    self.i += 1;
                    let inner = self.dq_parts(false)?;
                    parts.push(Part::Dbl(inner));
                }
                '$' if self.peek(1) == Some('\'') => {
                    Self::flush(&mut lit, &mut parts);
                    self.i += 2;
                    let st = self.i;
                    while let Some(c) = self.peek(0) {
                        if c == '\\' {
                            self.i += 2;
                            continue;
                        }
                        if c == '\'' {
                            break;
                        }
                        self.i += 1;
                    }
                    if self.eof() {
                        return Err(self.err("unclosed $'"));
                    }
                    parts.push(Part::AnsiC(self.s[st..self.i].iter().collect()));
                    self.i += 1;
                }
                '$' if self.peek(1) == Some('"') => {
                    Self::flush(&mut lit, &mut parts);
                    self.i += 2;
                    let inner = self.dq_parts(false)?;
                    parts.push(Part::Dbl(inner));
                }
                '$' => {
                    let p = self.dollar()?;
                    match p {
                        Part::Lit(l) => lit.push_str(&l),
                        p => {
                            Self::flush(&mut lit, &mut parts);
                            parts.push(p);
                        }
                    }
                }
                '`' => {
                    Self::flush(&mut lit, &mut parts);
                    parts.push(self.backtick()?);
                }
                '\\' => {
                    if self.peek(1) == Some('\n') {
                        self.i += 2;
                        continue;
                    }
                    lit.push('\\');
                    if let Some(n) = self.peek(1) {
                        lit.push(n);
                    }
                    self.i += 2.min(self.s.len() - self.i);
                }
                _ => {
                    lit.push(c);
                    self.i += 1;
                }
            }
        }
        Self::flush(&mut lit, &mut parts);
        if self.i > start {
            self.tok_end = self.i - 1;
        }
        Ok((!parts.is_empty()).then_some(Word { parts }))
    }

    /// Text inside `${ … }` (or an array index): quotes and expansions work, spaces are literal,
    /// and it ends at one of `stops` (not consumed).
    fn inner_word(&mut self, stops: &[char]) -> PResult<Word> {
        let mut parts = vec![];
        let mut lit = String::new();
        let mut braces = 0;
        while let Some(c) = self.peek(0) {
            if stops.contains(&c) && braces == 0 {
                break;
            }
            match c {
                '{' => {
                    braces += 1;
                    lit.push(c);
                    self.i += 1;
                }
                '}' => {
                    braces -= 1;
                    lit.push(c);
                    self.i += 1;
                }
                '\'' => {
                    Self::flush(&mut lit, &mut parts);
                    self.i += 1;
                    let st = self.i;
                    while self.peek(0).is_some_and(|c| c != '\'') {
                        self.i += 1;
                    }
                    parts.push(Part::Sgl(self.s[st..self.i.min(self.s.len())].iter().collect()));
                    self.i += 1;
                }
                '"' => {
                    Self::flush(&mut lit, &mut parts);
                    self.i += 1;
                    let inner = self.dq_parts(false)?;
                    parts.push(Part::Dbl(inner));
                }
                '$' => {
                    let p = self.dollar()?;
                    match p {
                        Part::Lit(l) => lit.push_str(&l),
                        p => {
                            Self::flush(&mut lit, &mut parts);
                            parts.push(p);
                        }
                    }
                }
                '`' => {
                    Self::flush(&mut lit, &mut parts);
                    parts.push(self.backtick()?);
                }
                '\\' => {
                    lit.push('\\');
                    if let Some(n) = self.peek(1) {
                        lit.push(n);
                    }
                    self.i += 2.min(self.s.len() - self.i);
                }
                _ => {
                    lit.push(c);
                    self.i += 1;
                }
            }
        }
        if self.eof() {
            return Err(self.err("unclosed ${"));
        }
        Self::flush(&mut lit, &mut parts);
        Ok(Word { parts })
    }

    /// Inside double quotes (opening quote already consumed), or a here-document body.
    fn dq_parts(&mut self, heredoc: bool) -> PResult<Vec<Part>> {
        let mut parts = vec![];
        let mut lit = String::new();
        loop {
            let Some(c) = self.peek(0) else {
                if heredoc {
                    break;
                }
                return Err(self.err("unclosed \""));
            };
            match c {
                '"' if !heredoc => {
                    self.i += 1;
                    break;
                }
                '$' => {
                    let p = self.dollar()?;
                    match p {
                        Part::Lit(l) => lit.push_str(&l),
                        p => {
                            Self::flush(&mut lit, &mut parts);
                            parts.push(p);
                        }
                    }
                }
                '`' => {
                    Self::flush(&mut lit, &mut parts);
                    parts.push(self.backtick()?);
                }
                '\\' => {
                    if self.peek(1) == Some('\n') {
                        self.i += 2;
                        continue;
                    }
                    lit.push('\\');
                    if let Some(n) = self.peek(1) {
                        lit.push(n);
                    }
                    self.i += 2.min(self.s.len() - self.i);
                }
                _ => {
                    lit.push(c);
                    self.i += 1;
                }
            }
        }
        Self::flush(&mut lit, &mut parts);
        if self.i > 0 {
            self.tok_end = self.i - 1;
        }
        Ok(parts)
    }

    fn backtick(&mut self) -> PResult<Part> {
        self.i += 1;
        let mut inner = String::new();
        loop {
            match self.peek(0) {
                None => return Err(self.err("unclosed `")),
                Some('`') => {
                    self.i += 1;
                    break;
                }
                Some('\\') if matches!(self.peek(1), Some('`' | '\\' | '$')) => {
                    inner.push(self.peek(1).unwrap());
                    self.i += 2;
                }
                Some(c) => {
                    inner.push(c);
                    self.i += 1;
                }
            }
        }
        let mut sub = Parser::new(&inner);
        let stmts = sub.stmt_list(&[], false, false)?;
        sub.skip_space_nl()?;
        if !sub.eof() {
            return Err(sub.err("unexpected input in `…`"));
        }
        Ok(Part::CmdSubst(stmts))
    }

    /// `$…` at the cursor. A lone `$` comes back as `Lit("$")`.
    fn dollar(&mut self) -> PResult<Part> {
        match self.peek(1) {
            Some('(') if self.peek(2) == Some('(') => {
                let save = self.i;
                self.i += 3;
                if let Ok(a) = self.arith_until("))") {
                    self.i += 2;
                    self.tok_end = self.i - 1;
                    return Ok(Part::Arith(a));
                }
                self.i = save;
                self.cmd_subst()
            }
            Some('(') => self.cmd_subst(),
            Some('{') => {
                self.i += 2;
                let p = self.param_braced()?;
                Ok(Part::Param(Box::new(p)))
            }
            Some(c) if c == '_' || c.is_ascii_alphabetic() => {
                self.i += 1;
                let mut name = String::new();
                while let Some(c) = self.peek(0).filter(|c| *c == '_' || c.is_ascii_alphanumeric()) {
                    name.push(c);
                    self.i += 1;
                }
                Ok(Part::Param(Box::new(ParamExp { short: true, param: name, ..Default::default() })))
            }
            Some(c) if c.is_ascii_digit() || "@*#?$!-".contains(c) => {
                self.i += 2;
                Ok(Part::Param(Box::new(ParamExp { short: true, param: c.to_string(), ..Default::default() })))
            }
            _ => {
                self.i += 1;
                Ok(Part::Lit("$".into()))
            }
        }
    }

    fn cmd_subst(&mut self) -> PResult<Part> {
        self.i += 2;
        let stmts = self.stmt_list(&[], true, false)?;
        self.skip_space_nl()?;
        if self.peek(0) != Some(')') {
            return Err(self.err("unclosed $("));
        }
        self.i += 1;
        self.tok_end = self.i - 1;
        Ok(Part::CmdSubst(stmts))
    }

    /// After `${`.
    fn param_braced(&mut self) -> PResult<ParamExp> {
        let mut p = ParamExp::default();
        if self.peek(0) == Some('#') && !matches!(self.peek(1), Some('}') | None) {
            // ${#} is $#; ${#x} is a length
            if !(self.peek(1) == Some(':') || self.peek(1) == Some('-') && self.peek(2) == Some('}')) {
                p.length = true;
                self.i += 1;
            }
        } else if self.peek(0) == Some('!') && self.peek(1).is_some_and(|c| c == '_' || c.is_ascii_alphanumeric() || c == '@' || c == '*') {
            p.excl = true;
            self.i += 1;
        }
        let mut name = String::new();
        match self.peek(0) {
            Some(c) if c == '_' || c.is_ascii_alphabetic() => {
                while let Some(c) = self.peek(0).filter(|c| *c == '_' || c.is_ascii_alphanumeric()) {
                    name.push(c);
                    self.i += 1;
                }
            }
            Some(c) if c.is_ascii_digit() => {
                while let Some(c) = self.peek(0).filter(|c| c.is_ascii_digit()) {
                    name.push(c);
                    self.i += 1;
                }
            }
            Some(c) if "@*#?$!-".contains(c) => {
                name.push(c);
                self.i += 1;
            }
            _ => return Err(self.err("bad ${…}")),
        }
        p.param = name;
        if p.excl && matches!(self.peek(0), Some('*' | '@')) && self.peek(1) == Some('}') {
            p.excl = false;
            p.names = true;
            self.i += 1;
        }
        if self.peek(0) == Some('[') {
            self.i += 1;
            p.index = Some(self.inner_word(&[']'])?);
            self.i += 1;
        }
        match self.peek(0) {
            Some('}') => {}
            Some(':') if matches!(self.peek(1), Some('-' | '=' | '?' | '+')) => {
                let op: String = [':', self.peek(1).unwrap()].iter().collect();
                self.i += 2;
                let w = self.inner_word(&['}'])?;
                p.exp = Some((op, (!w.parts.is_empty()).then_some(w)));
            }
            Some(':') => {
                self.i += 1;
                let off = self.inner_word(&[':', '}'])?;
                let len = if self.peek(0) == Some(':') {
                    self.i += 1;
                    Some(self.inner_word(&['}'])?)
                } else {
                    None
                };
                p.slice = Some((off, len));
            }
            Some(c @ ('-' | '=' | '?' | '+')) => {
                self.i += 1;
                let w = self.inner_word(&['}'])?;
                p.exp = Some((c.to_string(), (!w.parts.is_empty()).then_some(w)));
            }
            Some(c @ ('#' | '%' | '^' | ',')) => {
                let op = if self.peek(1) == Some(c) { format!("{c}{c}") } else { c.to_string() };
                self.i += op.len();
                let w = self.inner_word(&['}'])?;
                p.exp = Some((op, (!w.parts.is_empty()).then_some(w)));
            }
            Some('/') => {
                self.i += 1;
                let (all, anchor) = match self.peek(0) {
                    Some('/') => {
                        self.i += 1;
                        (true, None)
                    }
                    Some(a @ ('#' | '%')) => {
                        self.i += 1;
                        (false, Some(a))
                    }
                    _ => (false, None),
                };
                let orig = self.inner_word(&['/', '}'])?;
                let with = if self.peek(0) == Some('/') {
                    self.i += 1;
                    Some(self.inner_word(&['}'])?).filter(|w| !w.parts.is_empty())
                } else {
                    None
                };
                p.repl = Some(Repl { all, anchor, orig, with });
            }
            Some('@') => {
                self.i += 1;
                let op: String = self.peek(0).map(|c| c.to_string()).unwrap_or_default();
                self.i += 1;
                p.exp = Some((format!("@{op}"), None));
            }
            _ => return Err(self.err("bad ${…}")),
        }
        if self.peek(0) != Some('}') {
            return Err(self.err("unclosed ${"));
        }
        self.i += 1;
        self.tok_end = self.i - 1;
        Ok(p)
    }

    // ------------------------------------------------------------ [[ … ]]

    fn test_skip(&mut self) -> PResult<()> {
        self.skip_space_nl()
    }
    fn test_or(&mut self) -> PResult<TestExpr> {
        let mut x = self.test_and()?;
        loop {
            self.test_skip()?;
            if self.at("||") {
                self.bump(2);
                let y = self.test_and()?;
                x = TestExpr::Bin("||".into(), Box::new(x), Box::new(y));
            } else {
                return Ok(x);
            }
        }
    }
    fn test_and(&mut self) -> PResult<TestExpr> {
        let mut x = self.test_not()?;
        loop {
            self.test_skip()?;
            if self.at("&&") {
                self.bump(2);
                let y = self.test_not()?;
                x = TestExpr::Bin("&&".into(), Box::new(x), Box::new(y));
            } else {
                return Ok(x);
            }
        }
    }
    fn test_not(&mut self) -> PResult<TestExpr> {
        self.test_skip()?;
        if self.peek(0) == Some('!') && matches!(self.peek(1), Some(' ' | '\t' | '(')) {
            self.bump(1);
            let x = self.test_not()?;
            return Ok(TestExpr::Un("!".into(), Box::new(x)));
        }
        self.test_primary()
    }
    fn test_primary(&mut self) -> PResult<TestExpr> {
        self.test_skip()?;
        if self.peek(0) == Some('(') {
            self.bump(1);
            let x = self.test_or()?;
            self.test_skip()?;
            if self.peek(0) != Some(')') {
                return Err(self.err("expected ) in [[ ]]"));
            }
            self.bump(1);
            return Ok(TestExpr::Paren(Box::new(x)));
        }
        const UNARY: &[&str] = &["-a", "-b", "-c", "-d", "-e", "-f", "-g", "-h", "-k", "-p", "-r", "-s", "-t", "-u", "-w",
            "-x", "-G", "-L", "-N", "-O", "-S", "-z", "-n", "-o", "-v", "-R"];
        if let Some(op) = self.peek_plain().filter(|w| UNARY.contains(&w.as_str())) {
            let save = self.i;
            self.bump(op.len());
            self.test_skip()?;
            if !(self.at("]]") || self.at("&&") || self.at("||") || self.peek(0) == Some(')')) {
                let w = self.word(Ctx::Test)?.ok_or_else(|| self.err("expected a word"))?;
                return Ok(TestExpr::Un(op, Box::new(TestExpr::Word(w))));
            }
            self.i = save;
        }
        let x = self.word(Ctx::Test)?.ok_or_else(|| self.err("expected a word in [[ ]]"))?;
        self.skip_blanks();
        const BIN: &[&str] = &["==", "=", "!=", "=~", "<", ">", "-eq", "-ne", "-lt", "-le", "-gt", "-ge", "-nt", "-ot", "-ef"];
        let op = if matches!(self.peek(0), Some('<' | '>')) {
            Some(self.peek(0).unwrap().to_string())
        } else {
            self.peek_plain().filter(|w| BIN.contains(&w.as_str()))
        };
        if let Some(op) = op {
            self.bump(op.len());
            self.skip_blanks();
            let ctx = if op == "=~" { Ctx::Regex } else { Ctx::Test };
            let y = self.word(ctx)?.ok_or_else(|| self.err("expected a word after the operator"))?;
            return Ok(TestExpr::Bin(op, Box::new(TestExpr::Word(x)), Box::new(TestExpr::Word(y))));
        }
        Ok(TestExpr::Word(x))
    }

    // ------------------------------------------------------------ arithmetic

    fn arith_opt(&mut self, end: &str) -> PResult<Option<Arith>> {
        self.skip_space_nl()?;
        if self.at(end) {
            return Ok(None);
        }
        Ok(Some(self.arith_until(end)?))
    }
    /// An arithmetic expression that must be followed by `end` (not consumed; "" means EOF).
    fn arith_until(&mut self, end: &str) -> PResult<Arith> {
        let e = self.a_comma()?;
        self.skip_space_nl()?;
        let ok = if end.is_empty() { self.eof() } else { self.at(end) };
        if !ok {
            return Err(self.err(&format!("expected {end:?} after an arithmetic expression")));
        }
        Ok(e)
    }
    fn a_skip(&mut self) {
        let _ = self.skip_space_nl();
    }
    fn a_op(&mut self, ops: &[&str]) -> Option<String> {
        self.a_skip();
        for o in ops {
            if self.at(o) {
                // don't take `=` from `==`, `<` from `<=` / `<<`, `&` from `&&`, …
                let next = self.peek(o.len());
                let longer = match *o {
                    "=" => next == Some('='),
                    "<" => matches!(next, Some('<' | '=')),
                    ">" => matches!(next, Some('>' | '=')),
                    "&" => matches!(next, Some('&' | '=')),
                    "|" => matches!(next, Some('|' | '=')),
                    "*" => matches!(next, Some('*' | '=')),
                    "+" | "-" | "/" | "%" | "^" => next == Some('='),
                    "<<" | ">>" => next == Some('='),
                    "!" => next == Some('='),
                    _ => false,
                };
                if longer {
                    continue;
                }
                if *o == ")" {
                    return None;
                }
                self.i += o.len();
                return Some(o.to_string());
            }
        }
        None
    }
    fn a_comma(&mut self) -> PResult<Arith> {
        let mut x = self.a_assign()?;
        while let Some(op) = self.a_op(&[","]) {
            let y = self.a_assign()?;
            x = Arith::Bin(op, Box::new(x), Box::new(y));
        }
        Ok(x)
    }
    fn a_assign(&mut self) -> PResult<Arith> {
        let x = self.a_cond()?;
        if let Some(op) = self.a_op(&["<<=", ">>=", "*=", "/=", "%=", "+=", "-=", "&=", "^=", "|=", "="]) {
            let y = self.a_assign()?;
            return Ok(Arith::Bin(op, Box::new(x), Box::new(y)));
        }
        Ok(x)
    }
    fn a_cond(&mut self) -> PResult<Arith> {
        let c = self.a_bin(0)?;
        if self.a_op(&["?"]).is_some() {
            let t = self.a_assign()?;
            if self.a_op(&[":"]).is_none() {
                return Err(self.err("expected : in ?:"));
            }
            let f = self.a_assign()?;
            return Ok(Arith::Cond(Box::new(c), Box::new(t), Box::new(f)));
        }
        Ok(c)
    }
    fn a_bin(&mut self, level: usize) -> PResult<Arith> {
        const LEVELS: &[&[&str]] = &[&["||"], &["&&"], &["|"], &["^"], &["&"], &["==", "!="], &["<=", ">=", "<", ">"],
            &["<<", ">>"], &["+", "-"], &["*", "/", "%"]];
        if level == LEVELS.len() {
            return self.a_pow();
        }
        let mut x = self.a_bin(level + 1)?;
        while let Some(op) = self.a_op(LEVELS[level]) {
            let y = self.a_bin(level + 1)?;
            x = Arith::Bin(op, Box::new(x), Box::new(y));
        }
        Ok(x)
    }
    fn a_pow(&mut self) -> PResult<Arith> {
        let x = self.a_unary()?;
        if let Some(op) = self.a_op(&["**"]) {
            let y = self.a_pow()?;
            return Ok(Arith::Bin(op, Box::new(x), Box::new(y)));
        }
        Ok(x)
    }
    fn a_unary(&mut self) -> PResult<Arith> {
        self.a_skip();
        for op in ["++", "--", "!", "~", "+", "-"] {
            if self.at(op) && !(op == "!" && self.peek(1) == Some('=')) {
                self.i += op.len();
                let x = self.a_unary()?;
                return Ok(Arith::Un(op.into(), Box::new(x), false));
            }
        }
        let mut x = self.a_primary()?;
        self.skip_blanks();
        for op in ["++", "--"] {
            if self.at(op) {
                self.i += 2;
                x = Arith::Un(op.into(), Box::new(x), true);
            }
        }
        Ok(x)
    }
    fn a_primary(&mut self) -> PResult<Arith> {
        self.a_skip();
        match self.peek(0) {
            Some('(') => {
                self.i += 1;
                let x = self.a_comma()?;
                self.a_skip();
                if self.peek(0) != Some(')') {
                    return Err(self.err("expected ) in an arithmetic expression"));
                }
                self.i += 1;
                Ok(Arith::Paren(Box::new(x)))
            }
            Some('$') => {
                let p = self.dollar()?;
                Ok(Arith::Word(Word { parts: vec![p] }))
            }
            Some('"') => {
                self.i += 1;
                let inner = self.dq_parts(false)?;
                Ok(Arith::Word(Word { parts: vec![Part::Dbl(inner)] }))
            }
            Some(c) if c.is_ascii_alphanumeric() || c == '_' => {
                let mut t = String::new();
                while let Some(c) = self.peek(0).filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '#') {
                    t.push(c);
                    self.i += 1;
                }
                if self.peek(0) == Some('[') && valid_name(&t) {
                    return Err(self.err("array elements in arithmetic are not supported"));
                }
                Ok(Arith::Word(Word::lit(&t)))
            }
            _ => Err(self.err("expected a number or a name")),
        }
    }
}

/// A here-document delimiter: its text with quoting removed, and whether any was quoted.
fn heredoc_delim(w: &Word) -> (String, bool) {
    let mut s = String::new();
    let mut quoted = false;
    for p in &w.parts {
        match p {
            Part::Lit(l) => {
                let mut it = l.chars();
                while let Some(c) = it.next() {
                    if c == '\\' {
                        quoted = true;
                        if let Some(n) = it.next() {
                            s.push(n);
                        }
                    } else {
                        s.push(c);
                    }
                }
            }
            Part::Sgl(t) => {
                quoted = true;
                s.push_str(t);
            }
            Part::Dbl(ps) => {
                quoted = true;
                for p in ps {
                    if let Part::Lit(l) = p {
                        s.push_str(l);
                    }
                }
            }
            _ => {}
        }
    }
    (s, quoted)
}

/// The text of a word made only of literals and quotes (for `let 'i += 1'`).
fn unquote_simple(w: &Word) -> Option<String> {
    let mut s = String::new();
    for p in &w.parts {
        match p {
            Part::Lit(l) => s.push_str(&l.replace('\\', "")),
            Part::Sgl(t) => s.push_str(t),
            Part::Dbl(ps) => {
                for p in ps {
                    match p {
                        Part::Lit(l) => s.push_str(l),
                        _ => return None,
                    }
                }
            }
            _ => return None,
        }
    }
    Some(s)
}
