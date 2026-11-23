//! complete-me alias / func: shell aliases and functions that pass their arguments through
//! and keep the wrapped command's TAB completions.
//!
//!   complete-me alias gco git checkout          # gco <TAB> completes branches like git checkout
//!   complete-me alias logs 'journalctl -u $1 -f' # $1 goes where you put it
//!   complete-me alias --abbr gs git status      # fish abbreviation: expands as you type
//!   complete-me func mkcd 'mkdir -p $1; and cd $1'
//!
//! Fish gets an autoloaded file in ~/.config/fish/functions using --wraps.
//! Bash/zsh get ~/.config/complete-me/aliases.{bash,zsh}, which the installer sources.

use crate::re;
use crate::util::*;
use std::path::PathBuf;

const MARK: &str = "# Created by complete-me";
const FISH_RESERVED: &[&str] = &[
    "and", "or", "not", "if", "else", "end", "for", "while", "function", "switch", "case", "begin", "return", "break",
    "continue", "set", "test", "string", "builtin", "command", "exec", "time", "status", "math", "count", "cd", "eval",
    "source", "read",
];

fn placeholder() -> &'static fancy_regex::Regex {
    re!(r#"\$(?:argv(?:\[\d+\])?|[1-9@*])|"\$@""#)
}

pub fn fish_functions_dir() -> PathBuf {
    config_home().join("fish").join("functions")
}
pub fn sh_file(shell: &str) -> PathBuf {
    config_home().join("complete-me").join(format!("aliases.{shell}"))
}
pub fn abbr_file() -> PathBuf {
    config_home().join("fish").join("conf.d").join("complete_me_abbrs.fish")
}

pub fn to_fish(body: &str) -> String {
    let b = body.replace("\"$@\"", "$argv").replace("$@", "$argv").replace("$*", "$argv");
    replace_all(re!(r"\$([1-9])"), &b, "$$argv[$1]")
}

pub fn to_sh(body: &str) -> String {
    let b = replace_all(re!(r"\$argv\[(\d)\]"), body, "\"$$$1\"").replace("$argv", "\"$@\"");
    let b = replace_all(re!(r#"(?<!")\$([1-9])(?!")"#), &b, "\"$$$1\"");
    b.replace("; and ", " && ").replace("; or ", " || ")
}

/// The command words the alias stands for, up to the first placeholder or control operator.
pub fn wraps_of(body: &str) -> String {
    if body.trim().is_empty() {
        return String::new();
    }
    let head = body.split(';').next().unwrap_or("").split("&&").next().unwrap_or("").split('|').next().unwrap_or("");
    let mut words = vec![];
    for w in shell_words(head) {
        if is_match(placeholder(), &w) || w == "command" || w == "builtin" {
            if w == "command" || w == "builtin" {
                continue;
            }
            break;
        }
        words.push(w);
    }
    words.iter().map(|w| shquote(w)).collect::<Vec<_>>().join(" ")
}

fn check_name(name: &str) -> Result<(), String> {
    if !is_match(re!(r"^[A-Za-z_,.][\w.,+-]*$"), name) || FISH_RESERVED.contains(&name) {
        return Err(format!("complete-me: {name:?} can't be used as a name (reserved or unsafe)"));
    }
    Ok(())
}

fn rest_args_suffix(body: &str) -> Option<usize> {
    if is_match(re!(r"\$argv(?!\[)|\$[@*]"), body) {
        return None;
    }
    find_all(re!(r"\$(?:argv\[)?([1-9])"), body).iter().filter_map(|c| group(c, 1).parse().ok()).max()
}

pub fn write_fish(name: &str, body: &str, desc: &str, is_alias: bool) -> Result<PathBuf, String> {
    let mut fbody = to_fish(body);
    if fbody.split_whitespace().next() == Some(name) {
        fbody = format!("command {fbody}"); // alias ls 'ls -la' must not call itself
    }
    if is_alias && !is_match(placeholder(), body) {
        fbody += " $argv";
    } else if is_alias {
        if let Some(h) = rest_args_suffix(body) {
            fbody += &format!(" $argv[{}..]", h + 1); // pass the rest through too
        }
    }
    let wraps = wraps_of(body);
    let mut head = format!("function {name}");
    if !wraps.is_empty() && shell_words(&wraps).first().map(|s| s.as_str()) != Some(name) {
        head += &format!(" --wraps {}", fq(&wraps));
    }
    head += &format!(" --description {}", fq(desc));
    let indented: Vec<String> = fbody.split('\n').map(|l| format!("    {l}")).collect();
    let path = fish_functions_dir().join(format!("{name}.fish"));
    if path.exists() {
        let t = std::fs::read_to_string(&path).unwrap_or_default();
        if !t.chars().take(200).collect::<String>().contains(MARK) {
            return Err(format!("complete-me: {} exists and was not made by complete-me; not overwriting", path.display()));
        }
    }
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let kind = if is_alias { "alias" } else { "func" };
    std::fs::write(&path, format!("{MARK} ({kind}). Remove: complete-me alias --rm {name}\n{head}\n{}\nend\n", indented.join("\n")))
        .map_err(|e| e.to_string())?;
    Ok(path)
}

pub fn write_sh(shell: &str, name: &str, body: &str, is_alias: bool) -> PathBuf {
    let mut sbody = to_sh(body);
    if is_alias && !is_match(placeholder(), body) {
        sbody += " \"$@\"";
    } else if is_alias {
        if let Some(h) = rest_args_suffix(body) {
            sbody += &format!(" \"${{@:{}}}\"", h + 1);
        }
    }
    if sbody.split_whitespace().next() == Some(name) {
        sbody = format!("command {sbody}");
    }
    let wraps = wraps_of(body);
    let path = sh_file(shell);
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    let existing = std::fs::read_to_string(&path).ok();
    let mut lines: Vec<String> = match &existing {
        Some(t) => t.split('\n').map(String::from).collect(),
        None => vec![format!("{MARK}: aliases and functions. Sourced from your rc file.")],
    };
    let tag = format!("# cm:{name}");
    lines.retain(|l| !l.ends_with(&tag));
    lines.push(format!("{name}() {{ {sbody}; }}  {tag}"));
    if !wraps.is_empty() {
        if shell == "bash" {
            lines.push(format!("_complete_me_alias_register {name} {wraps}  {tag}"));
        } else {
            let first = shell_words(&wraps).into_iter().next().unwrap_or_default();
            lines.push(format!("compdef {name}={first} 2>/dev/null  {tag}"));
        }
    }
    let text: Vec<String> = lines.into_iter().filter(|l| !l.is_empty()).collect();
    let _ = std::fs::write(&path, text.join("\n") + "\n");
    path
}

pub fn write_abbr(name: &str, body: &str) -> PathBuf {
    let p = abbr_file();
    let tag = format!("# cm:{name}");
    let mut lines: Vec<String> = match std::fs::read_to_string(&p) {
        Ok(t) => t.split('\n').map(String::from).collect(),
        Err(_) => vec![format!("{MARK}: abbreviations (complete-me alias --abbr).")],
    };
    lines.retain(|l| !l.is_empty() && !l.ends_with(&tag));
    let cursor = if body.contains('%') { " --set-cursor" } else { "" };
    lines.push(format!("abbr -a{cursor} -- {name} {}  {tag}", fq(body)));
    let _ = std::fs::create_dir_all(p.parent().unwrap());
    let _ = std::fs::write(&p, lines.join("\n") + "\n");
    p
}

pub fn remove(name: &str) {
    let p = fish_functions_dir().join(format!("{name}.fish"));
    if let Ok(t) = std::fs::read_to_string(&p) {
        if t.chars().take(200).collect::<String>().contains(MARK) {
            let _ = std::fs::remove_file(&p);
            println!("removed {}", p.display());
        }
    }
    let tag = format!("# cm:{name}");
    for shell in ["bash", "zsh"] {
        let f = sh_file(shell);
        if let Ok(t) = std::fs::read_to_string(&f) {
            let lines: Vec<&str> = t.split('\n').collect();
            let kept: Vec<&str> = lines.iter().copied().filter(|l| !l.ends_with(&tag)).collect();
            if kept.len() != lines.len() {
                let _ = std::fs::write(&f, kept.join("\n"));
                println!("removed {name} from {}", f.display());
            }
        }
    }
    let a = abbr_file();
    if let Ok(t) = std::fs::read_to_string(&a) {
        let lines: Vec<&str> = t.split('\n').collect();
        let kept: Vec<&str> = lines.iter().copied().filter(|l| !l.ends_with(&tag)).collect();
        if kept.len() != lines.len() {
            let _ = std::fs::write(&a, kept.join("\n"));
            println!("removed abbreviation {name}");
        }
    }
}

fn pad(s: &str, w: usize) -> String {
    let n = s.chars().count();
    if n >= w { s.into() } else { format!("{s}{}", " ".repeat(w - n)) }
}

pub fn list_all() {
    let d = fish_functions_dir();
    let mut files: Vec<PathBuf> = std::fs::read_dir(&d)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().map(|e| e == "fish").unwrap_or(false)).collect())
        .unwrap_or_default();
    files.sort();
    for p in files {
        let t = std::fs::read_to_string(&p).unwrap_or_default();
        if t.chars().take(200).collect::<String>().contains(MARK) {
            let lines: Vec<&str> = t.split('\n').collect();
            let body: Vec<&str> = if lines.len() > 4 { lines[2..lines.len() - 2].iter().map(|l| l.trim()).collect() } else { vec![] };
            let stem = p.file_stem().unwrap().to_string_lossy().into_owned();
            println!("{} fish  {}", pad(&stem, 14), body.join("; "));
        }
    }
    if let Ok(t) = std::fs::read_to_string(abbr_file()) {
        for l in t.split('\n').skip(1) {
            if let Some(m) = find(re!(r"^abbr -a(?: --set-cursor)? -- (\S+) (.*)  # cm:"), l) {
                println!("{} abbr  {}", pad(group(&m, 1), 14), group(&m, 2));
            }
        }
    }
}

fn edit_new(name: &str, wraps: Option<&str>) -> i32 {
    let p = fish_functions_dir().join(format!("{name}.fish"));
    if !p.exists() {
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        let w = wraps.map(|w| format!(" --wraps {}", fq(w))).unwrap_or_default();
        let _ = std::fs::write(
            &p,
            format!(
                "{MARK} (func). Remove: complete-me alias --rm {name}\nfunction {name}{w} --description ''\n    argparse h/help -- $argv\n    or return\n    if set -q _flag_help\n        echo \"usage: {name} ARGS…\"\n        return 0\n    end\n    # $argv holds the remaining arguments; $argv[1] is the first\n    \nend\n"
            ),
        );
    }
    let editor = std::env::var("VISUAL").ok().filter(|s| !s.is_empty())
        .or_else(|| std::env::var("EDITOR").ok().filter(|s| !s.is_empty()))
        .unwrap_or_else(|| "nano".into());
    let mut words = shell_words(&editor);
    if words.is_empty() {
        words.push("nano".into());
    }
    std::process::Command::new(&words[0]).args(&words[1..]).arg(&p).status().map(|s| s.code().unwrap_or(1)).unwrap_or(1)
}

pub const HELP: &str = "usage: complete-me alias|func NAME [COMMAND…] [--shell fish|bash|zsh|all] [--abbr] [--wraps CMD] [--desc TEXT]
       complete-me alias --list
       complete-me alias --rm NAME…

  complete-me alias gco git checkout           # gco <TAB> completes like git checkout
  complete-me alias logs 'journalctl -u $1 -f' # $1 goes where you put it; extra args are passed on
  complete-me alias --abbr gs git status       # fish abbreviation: expands as you type
  complete-me func mkcd 'mkdir -p $1; and cd $1'
  complete-me func NAME                        # opens $EDITOR on a template
";

pub fn run(args: &[String], is_alias: bool) -> i32 {
    // argparse-style: known flags anywhere, everything else positional
    let (mut shell, mut abbr, mut wraps, mut desc, mut list, mut rm) = ("fish".to_string(), false, None::<String>, None::<String>, false, None::<Vec<String>>);
    let mut pos: Vec<String> = vec![];
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        match a.as_str() {
            "-h" | "--help" => {
                print!("{HELP}");
                return 0;
            }
            "--shell" => {
                i += 1;
                shell = args.get(i).cloned().unwrap_or_default();
            }
            "--abbr" => abbr = true,
            "--list" => list = true,
            "--wraps" => {
                i += 1;
                wraps = args.get(i).cloned();
            }
            "--desc" => {
                i += 1;
                desc = args.get(i).cloned();
            }
            "--rm" => {
                rm = Some(args[i + 1..].to_vec());
                break;
            }
            "--" => {
                pos.extend(args[i + 1..].iter().cloned());
                break;
            }
            s if s.starts_with("--shell=") => shell = s["--shell=".len()..].into(),
            _ => pos.push(a.clone()),
        }
        i += 1;
    }
    if !["fish", "bash", "zsh", "all"].contains(&shell.as_str()) {
        eprintln!("complete-me: --shell must be fish, bash, zsh or all");
        return 2;
    }
    if list {
        list_all();
        return 0;
    }
    if let Some(names) = rm {
        for n in names {
            remove(&n);
        }
        return 0;
    }
    let Some(name) = pos.first().cloned() else {
        eprintln!("usage: complete-me alias NAME COMMAND…   (see --help)");
        return 2;
    };
    if let Err(e) = check_name(&name) {
        eprintln!("{e}");
        return 2;
    }
    let body_parts = &pos[1..];
    let mut body = body_parts.join(" ");
    if body.is_empty() && !is_alias {
        if !is_tty(0) {
            use std::io::Read;
            let _ = std::io::stdin().read_to_string(&mut body);
        }
        if body.is_empty() {
            return edit_new(&name, wraps.as_deref());
        }
    }
    if body.is_empty() {
        eprintln!("complete-me: give the command the alias stands for");
        return 2;
    }
    let desc = desc.unwrap_or_else(|| if is_alias { format!("alias {name}={body}") } else { format!("function {name}") });
    let shells: Vec<&str> = if shell == "all" { vec!["fish", "bash", "zsh"] } else { vec![shell.as_str()] };
    for sh in shells {
        if sh == "fish" {
            if abbr {
                println!("fish abbreviation -> {}  (new shells; or run: source it)", write_abbr(&name, &body).display());
            } else {
                let p = match write_fish(&name, &body, &desc, is_alias) {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("{e}");
                        return 1;
                    }
                };
                if let Some(w) = &wraps {
                    let t = std::fs::read_to_string(&p).unwrap_or_default();
                    let t = re!(r" --wraps '[^']*'").replace(&t, "").into_owned();
                    let t = t.replacen(&format!("function {name}"), &format!("function {name} --wraps {}", fq(w)), 1);
                    let _ = std::fs::write(&p, t);
                }
                let like = wraps.clone().unwrap_or_else(|| wraps_of(&body));
                println!("fish -> {}  (available immediately in every fish; TAB completes like: {})", p.display(), if like.is_empty() { "n/a".into() } else { like });
            }
        } else {
            println!("{sh} -> {}", write_sh(sh, &name, &body, is_alias).display());
        }
    }
    0
}
