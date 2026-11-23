//! The structured model every parser fills in and every shell renderer reads.

use indexmap::IndexMap;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Opt {
    /// "a" for -a
    pub shorts: Vec<String>,
    /// "all" for --all
    pub longs: Vec<String>,
    /// "name" for find-style -name
    pub olds: Vec<String>,
    /// placeholder, e.g. "WHEN"
    pub arg: Option<String>,
    /// --color[=WHEN]
    pub arg_optional: bool,
    pub desc: String,
    /// (value, description)
    pub values: Vec<(String, String)>,
    /// file|dir|user|group|host|pid|command|number|enum|""
    pub kind: String,
}

impl Opt {
    pub fn flags(&self) -> Vec<String> {
        let mut v: Vec<String> = self.shorts.iter().map(|s| format!("-{s}")).collect();
        v.extend(self.longs.iter().map(|l| format!("--{l}")));
        v.extend(self.olds.iter().map(|o| format!("-{o}")));
        v
    }
    pub fn has_flag(&self, flag: &str) -> bool {
        self.flags().iter().any(|f| f == flag)
    }
    pub fn is_empty(&self) -> bool {
        self.shorts.is_empty() && self.longs.is_empty() && self.olds.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    pub name: String,
    pub desc: String,
    pub aliases: Vec<String>,
    pub options: Vec<Opt>,
    pub subcommands: IndexMap<String, Command>,
    pub takes_files: bool,
    pub sources: Vec<String>,
}

impl Command {
    pub fn new(name: &str) -> Self {
        Command {
            name: name.to_string(),
            desc: String::new(),
            aliases: vec![],
            options: vec![],
            subcommands: IndexMap::new(),
            takes_files: true,
            sources: vec![],
        }
    }
    pub fn with_source(name: &str, source: &str) -> Self {
        let mut c = Command::new(name);
        c.sources.push(source.to_string());
        c
    }

    pub fn option(&self, flag: &str) -> Option<&Opt> {
        self.options.iter().find(|o| o.has_flag(flag))
    }
    pub fn option_mut(&mut self, flag: &str) -> Option<&mut Opt> {
        self.options.iter_mut().find(|o| o.has_flag(flag))
    }

    /// Fold another parse of the same command into this one; existing data wins.
    pub fn merge(&mut self, other: Command) {
        if self.desc.is_empty() {
            self.desc = other.desc;
        }
        for o in other.options {
            let of = o.flags();
            let mine = self.options.iter_mut().find(|m| m.flags().iter().any(|f| of.contains(f)));
            match mine {
                None => self.options.push(o),
                Some(m) => {
                    for v in o.shorts {
                        if !m.shorts.contains(&v) {
                            m.shorts.push(v);
                        }
                    }
                    for v in o.longs {
                        if !m.longs.contains(&v) {
                            m.longs.push(v);
                        }
                    }
                    for v in o.olds {
                        if !m.olds.contains(&v) {
                            m.olds.push(v);
                        }
                    }
                    if m.desc.is_empty() {
                        m.desc = o.desc;
                    }
                    if m.arg.is_none() {
                        m.arg = o.arg;
                    }
                    if m.values.is_empty() {
                        m.values = o.values;
                    }
                    if m.kind.is_empty() {
                        m.kind = o.kind;
                    }
                }
            }
        }
        for (name, sub) in other.subcommands {
            if let Some(mine) = self.subcommands.get_mut(&name) {
                mine.merge(sub);
            } else if !self.subcommands.values().any(|s| s.aliases.contains(&name)) {
                self.subcommands.insert(name, sub);
            }
        }
        self.takes_files = self.takes_files && other.takes_files;
        for s in other.sources {
            if !self.sources.contains(&s) {
                self.sources.push(s);
            }
        }
    }

    /// Every node with its subcommand path, depth-first, in insertion order.
    pub fn walk(&self) -> Vec<(Vec<String>, &Command)> {
        let mut out = vec![];
        fn go<'a>(c: &'a Command, path: Vec<String>, out: &mut Vec<(Vec<String>, &'a Command)>) {
            out.push((path.clone(), c));
            for (n, s) in &c.subcommands {
                let mut p = path.clone();
                p.push(n.clone());
                go(s, p, out);
            }
        }
        go(self, vec![], &mut out);
        out
    }

    /// Canonical one-fact-per-line dump (matches tests/golden/parse.*.txt).
    pub fn dump(&self) -> String {
        let mut out = String::new();
        for (path, node) in self.walk() {
            let p = path.join(" ");
            out += &format!(
                "NODE\t{p}\t{}\t{}\t{}\n",
                node.desc,
                if node.takes_files { "files" } else { "nofiles" },
                node.aliases.join(",")
            );
            for o in &node.options {
                let vals: Vec<String> = o.values.iter().map(|(v, d)| format!("{v}={d}")).collect();
                out += &format!(
                    "OPT\t{p}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                    o.flags().join(" "),
                    o.arg.clone().unwrap_or_default(),
                    if o.arg_optional { "opt" } else { "" },
                    o.kind,
                    vals.join(";"),
                    o.desc
                );
            }
        }
        out
    }

    pub fn to_json(&self) -> String {
        let mut s = String::new();
        json_command(self, &mut s);
        s
    }
}

pub fn json_str(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o += "\\\"",
            '\\' => o += "\\\\",
            '\n' => o += "\\n",
            '\t' => o += "\\t",
            c if (c as u32) < 0x20 => o += &format!("\\u{:04x}", c as u32),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn json_list(v: &[String]) -> String {
    format!("[{}]", v.iter().map(|x| json_str(x)).collect::<Vec<_>>().join(","))
}

fn json_command(c: &Command, s: &mut String) {
    s.push_str(&format!(
        "{{\"name\":{},\"desc\":{},\"aliases\":{},\"takes_files\":{},\"sources\":{},\"options\":[",
        json_str(&c.name),
        json_str(&c.desc),
        json_list(&c.aliases),
        c.takes_files,
        json_list(&c.sources)
    ));
    for (i, o) in c.options.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let vals: Vec<String> =
            o.values.iter().map(|(v, d)| format!("[{},{}]", json_str(v), json_str(d))).collect();
        s.push_str(&format!(
            "{{\"shorts\":{},\"longs\":{},\"olds\":{},\"arg\":{},\"arg_optional\":{},\"desc\":{},\"values\":[{}],\"kind\":{}}}",
            json_list(&o.shorts),
            json_list(&o.longs),
            json_list(&o.olds),
            o.arg.as_deref().map(json_str).unwrap_or_else(|| "null".into()),
            o.arg_optional,
            json_str(&o.desc),
            vals.join(","),
            json_str(&o.kind)
        ));
    }
    s.push_str("],\"subcommands\":{");
    for (i, (n, sub)) in c.subcommands.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&json_str(n));
        s.push(':');
        json_command(sub, s);
    }
    s.push_str("}}");
}
