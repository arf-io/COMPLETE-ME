//! A bash syntax tree, shaped like mvdan.cc/sh's (which babelfish translates from), so the
//! translation rules can follow babelfish's one for one.

#[derive(Debug, Clone, Default)]
pub struct File {
    pub stmts: Vec<Stmt>,
    /// Comments after the last statement.
    pub last: Vec<String>,
    /// Here-document bodies; `Redir::hdoc` indexes into this.
    pub heredocs: Vec<Word>,
}

#[derive(Debug, Clone)]
pub struct Stmt {
    /// Comment texts (without the `#`): the ones before the statement, then a trailing one.
    pub comments: Vec<String>,
    pub negated: bool,
    pub background: bool,
    pub coprocess: bool,
    pub cmd: Cmd,
    pub redirs: Vec<Redir>,
    /// First and last line of the statement itself (comments excluded), 1-based.
    pub line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone)]
pub enum Cmd {
    /// `a=1 b=2 cmd args…` (no args: plain assignments)
    Call { assigns: Vec<Assign>, args: Vec<Word> },
    Binary { op: BinOp, x: Box<Stmt>, y: Box<Stmt> },
    Block(Vec<Stmt>),
    Subshell(Vec<Stmt>),
    If(IfClause),
    While { until: bool, cond: Vec<Stmt>, body: Vec<Stmt> },
    /// `for name [in items]` (items None: no `in`, loops over "$@")
    For { name: String, items: Option<Vec<Word>>, body: Vec<Stmt>, select: bool },
    /// `for ((init; cond; post))`
    CFor { init: Option<Arith>, cond: Option<Arith>, post: Option<Arith>, body: Vec<Stmt> },
    Case { word: Word, items: Vec<CaseItem> },
    Func { name: String, body: Box<Stmt> },
    /// `export|local|declare|typeset|readonly|nameref …`
    Decl { variant: String, args: Vec<DeclArg> },
    Test(TestExpr),
    Arith(Arith),
    Time(Option<Box<Stmt>>),
    Let(Vec<Arith>),
    Coproc,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    And,
    Or,
    Pipe,
    PipeAll,
}

#[derive(Debug, Clone)]
pub struct IfClause {
    pub cond: Vec<Stmt>,
    pub then: Vec<Stmt>,
    /// `elif …` (an IfClause with `elif: true`) or a plain `else` (`elif: false`, empty cond).
    pub els: Option<Box<IfClause>>,
    pub elif: bool,
}

#[derive(Debug, Clone)]
pub struct CaseItem {
    pub patterns: Vec<Word>,
    pub stmts: Vec<Stmt>,
    /// `;;` `;&` `;;&`
    pub op: String,
}

#[derive(Debug, Clone)]
pub enum DeclArg {
    Assign(Assign),
    /// an option like `-x`, or anything that is not an assignment
    Word(Word),
}

#[derive(Debug, Clone)]
pub struct Assign {
    pub name: String,
    pub append: bool,
    /// `export A` (no `=`)
    pub naked: bool,
    pub index: Option<Word>,
    pub value: Option<Word>,
    pub array: Option<Vec<Word>>,
}

#[derive(Debug, Clone)]
pub struct Redir {
    /// fd number before the operator (`2>`), if any
    pub n: Option<String>,
    pub op: String,
    pub word: Word,
    pub hdoc: Option<usize>,
}

#[derive(Debug, Clone, Default)]
pub struct Word {
    pub parts: Vec<Part>,
}

#[derive(Debug, Clone)]
pub enum Part {
    /// Source text as written (backslash escapes kept).
    Lit(String),
    Sgl(String),
    /// `$'…'`, raw (escapes not yet decoded)
    AnsiC(String),
    Dbl(Vec<Part>),
    Param(Box<ParamExp>),
    CmdSubst(Vec<Stmt>),
    Arith(Arith),
    /// `<(…)` (out: false) or `>(…)` (out: true)
    ProcSubst { out: bool, stmts: Vec<Stmt> },
    ExtGlob(String),
}

#[derive(Debug, Clone, Default)]
pub struct ParamExp {
    /// `$a` rather than `${a}`
    pub short: bool,
    pub param: String,
    /// `${!a}`
    pub excl: bool,
    /// `${#a}`
    pub length: bool,
    /// `${a[i]}`
    pub index: Option<Word>,
    /// `${a:off:len}`
    pub slice: Option<(Word, Option<Word>)>,
    /// `${a/x/y}` (all: `//`; anchor: `#` or `%`)
    pub repl: Option<Repl>,
    /// `${a:-x}` etc: (operator, word)
    pub exp: Option<(String, Option<Word>)>,
    /// `${!prefix*}`
    pub names: bool,
}

#[derive(Debug, Clone)]
pub struct Repl {
    pub all: bool,
    pub anchor: Option<char>,
    pub orig: Word,
    pub with: Option<Word>,
}

#[derive(Debug, Clone)]
pub enum TestExpr {
    Bin(String, Box<TestExpr>, Box<TestExpr>),
    Un(String, Box<TestExpr>),
    Paren(Box<TestExpr>),
    Word(Word),
}

#[derive(Debug, Clone)]
pub enum Arith {
    Word(Word),
    Bin(String, Box<Arith>, Box<Arith>),
    /// op, operand, postfix
    Un(String, Box<Arith>, bool),
    Paren(Box<Arith>),
    Cond(Box<Arith>, Box<Arith>, Box<Arith>),
}

impl Word {
    pub fn lit(s: &str) -> Word {
        Word { parts: vec![Part::Lit(s.to_string())] }
    }
    /// The word's text when it is made only of literal parts (babelfish's `lit`).
    pub fn as_lit(&self) -> Option<String> {
        let mut s = String::new();
        for p in &self.parts {
            match p {
                Part::Lit(l) => s.push_str(l),
                _ => return None,
            }
        }
        Some(s)
    }
}

pub fn valid_name(s: &str) -> bool {
    let mut c = s.chars();
    matches!(c.next(), Some(ch) if ch == '_' || ch.is_ascii_alphabetic()) && c.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}
