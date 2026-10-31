//! complete-me command line: dispatch.
use crate::*;

pub const USAGE: &str = "complete-me — completions for everything, safely

usage: complete-me <command> [args]

  gen CMD…            generate completions (--force --quick --quiet)
  show CMD            print what complete-me understands (--json --quick)
  forget CMD…         delete generated completions
  list                list commands with generated completions
  explain -- LINE…    explain a command line: find expressions, regexes, flags
  regex …             explain | complete | build | flavor
  find …              explain | values | operators
  ssh-options         ssh -oKey=Value candidates (--current X)
  alias / func        aliases and functions that keep their completions
  snippets …          list | phrasebook | fish-abbrs | pick
  bash2fish [CODE]    translate bash to fish with a safety check
  extras --shell S -- LINE   bash/zsh: regex, find, ssh, clipboard and history candidates
  clip git|url|path|dir      the clipboard, if it fits that slot
  --version
";

pub fn main(args: &[String]) -> i32 {
    let Some(sub) = args.first() else {
        eprint!("{USAGE}");
        return 2;
    };
    let rest = &args[1..];
    match sub.as_str() {
        "--version" | "-V" => {
            println!("complete-me {VERSION}");
            0
        }
        "--help" | "-h" | "help" => {
            print!("{USAGE}");
            0
        }
        "gen" => cmd_gen(rest),
        "show" => cmd_show(rest),
        "forget" => cmd_forget(rest),
        "list" => cmd_list(rest),
        "explain" => cmd_explain(rest),
        "regex" => rx::run(rest),
        "find" => findhelp::run(rest),
        "ssh-options" => sshopts::run(rest),
        "alias" => aliases::run(rest, true),
        "func" => aliases::run(rest, false),
        "snippets" => snippets::run(rest),
        "bash2fish" => bashfish::run(rest),
        "extras" => extras::run(rest),
        "clip" => extras::run_clip(rest),
        other => {
            eprintln!("complete-me: unknown command {other:?}\n\n{USAGE}");
            2
        }
    }
}

const MISS_TTL: u64 = 7 * 86400;

fn age_secs(p: &std::path::Path) -> Option<u64> {
    let m = std::fs::metadata(p).ok()?.modified().ok()?;
    Some(std::time::SystemTime::now().duration_since(m).map(|d| d.as_secs()).unwrap_or(0))
}

/// True if fish ships or has a completion file for name (excluding man-page guesses and ours).
pub fn fish_has_completions(name: &str) -> bool {
    if util::which("fish").is_none() {
        return false;
    }
    let script = "status get-file completions/$argv[1].fish >/dev/null 2>&1; and exit 0; \
        for d in $fish_complete_path; string match -q -- '*/generated_completions' $d; and continue; \
        string match -q -- '*/complete-me/fish' $d; and continue; test -e $d/$argv[1].fish; and exit 0; end; exit 1";
    std::process::Command::new("fish")
        .args(["--no-config", "-c", script, name])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn cmd_gen(args: &[String]) -> i32 {
    let a = util::Args::parse(args, &[]);
    let (force, quick, quiet) = (a.has("--force"), a.has("--quick"), a.has("--quiet"));
    if a.rest.is_empty() {
        eprintln!("usage: complete-me gen [--force] [--quick] [--quiet] CMD…");
        return 2;
    }
    let mut rc = 0;
    for name in &a.rest {
        if !probe::safe_name(name) {
            eprintln!("complete-me: refusing unsafe name {name:?}");
            rc = 1;
            continue;
        }
        let miss = util::cache_dir("miss").join(name);
        let target = util::cache_dir("fish").join(format!("{name}.fish"));
        if !force {
            if target.exists() && !quick {
                let head: String = std::fs::read_to_string(&target).unwrap_or_default().chars().take(300).collect();
                if age_secs(&target).unwrap_or(u64::MAX) < MISS_TTL && !head.contains("--quick") {
                    continue;
                }
            }
            if miss.exists() && age_secs(&miss).unwrap_or(u64::MAX) < MISS_TTL {
                continue;
            }
            if fish_has_completions(name) {
                if !quiet {
                    eprintln!("{name}: fish already has completions for this; not shadowing them (use --force)");
                }
                continue;
            }
        }
        let model = match probe::gather(name, quick) {
            Some(m) if !(m.options.is_empty() && m.subcommands.is_empty()) => m,
            _ => {
                let _ = std::fs::write(&miss, "");
                if !quiet {
                    eprintln!("{name}: nothing found to complete");
                }
                rc = 1;
                continue;
            }
        };
        let _ = std::fs::remove_file(&miss);
        let (Ok(mut fishs), Ok(bashs), Ok(zshs)) = (render::fish(&model), render::bash(&model), render::zsh(&model)) else {
            rc = 1;
            continue;
        };
        if quick {
            fishs = fishs.replacen("# Regenerate:", "# --quick pass; full pass pending. Regenerate:", 1);
        }
        let ok = util::atomic_write(&target, &fishs).is_ok()
            && util::atomic_write(&util::cache_dir("bash").join(name), &bashs).is_ok()
            && util::atomic_write(&util::cache_dir("zsh").join(format!("{name}.zsh")), &zshs).is_ok()
            && util::atomic_write(&util::cache_dir("json").join(format!("{name}.json")), &model.to_json()).is_ok();
        if !ok {
            eprintln!("{name}: could not write the cache");
            rc = 1;
            continue;
        }
        if !quiet {
            let walk = model.walk();
            let n_sub = walk.iter().filter(|(p, _)| !p.is_empty()).count();
            let n_opt: usize = walk.iter().map(|(_, c)| c.options.len()).sum();
            println!("{name}: {n_opt} options, {n_sub} subcommands from {} -> {}", model.sources.join(", "), target.display());
        }
    }
    rc
}

pub fn cmd_show(args: &[String]) -> i32 {
    let a = util::Args::parse(args, &[]);
    let Some(name) = a.rest.first() else {
        eprintln!("usage: complete-me show [--json] [--quick] CMD");
        return 2;
    };
    let Some(model) = probe::gather(name, a.has("--quick")) else {
        eprintln!("{name}: nothing found");
        return 1;
    };
    if a.has("--json") {
        println!("{}", model.to_json());
        return 0;
    }
    for (path, node) in model.walk() {
        let pad = "  ".repeat(path.len());
        let mut full = vec![model.name.clone()];
        full.extend(path.iter().cloned());
        println!("{pad}{}  {}", full.join(" "), node.desc);
        for o in &node.options {
            let arg = o.arg.as_ref().map(|a| format!(" {a}")).unwrap_or_default();
            let vals = if o.values.is_empty() {
                String::new()
            } else {
                format!("  [{}]", o.values.iter().map(|(v, _)| v.as_str()).collect::<Vec<_>>().join(", "))
            };
            let kind = if !o.kind.is_empty() && o.kind != "enum" { format!("  ({})", o.kind) } else { String::new() };
            println!("{pad}    {}{arg}{vals}{kind}  {}", o.flags().join(", "), o.desc);
        }
    }
    0
}

pub fn cmd_forget(args: &[String]) -> i32 {
    for name in args.iter().filter(|a| !a.starts_with('-')) {
        for p in [
            util::cache_dir("fish").join(format!("{name}.fish")),
            util::cache_dir("bash").join(name),
            util::cache_dir("zsh").join(format!("{name}.zsh")),
            util::cache_dir("json").join(format!("{name}.json")),
            util::cache_dir("miss").join(name),
        ] {
            let _ = std::fs::remove_file(p);
        }
    }
    0
}

pub fn cmd_list(_args: &[String]) -> i32 {
    let mut names: Vec<String> = std::fs::read_dir(util::cache_dir("fish"))
        .map(|rd| rd.flatten().filter_map(|e| e.file_name().to_string_lossy().strip_suffix(".fish").map(String::from)).collect())
        .unwrap_or_default();
    names.sort();
    for n in names {
        println!("{n}");
    }
    0
}

const RG_ARGOPTS: &[&str] = &["-A", "-B", "-C", "-m", "-d", "-g", "-t", "-T", "-j", "-M", "-r", "-E", "--max-count", "--glob",
    "--iglob", "--type", "--type-not", "--threads", "--max-columns", "--replace", "--max-depth"];
const GREP_ARGOPTS: &[&str] = &["-A", "-B", "-C", "-m", "-d", "-D", "--max-count", "--include", "--exclude", "--exclude-dir"];

fn basename(s: &str) -> &str {
    s.rsplit('/').next().unwrap_or(s)
}

pub fn pattern_of(tokens: &[String]) -> Option<String> {
    let tool = basename(&tokens[0]);
    let mut rest = &tokens[1..];
    if tool == "find" {
        for i in 0..rest.len().saturating_sub(1) {
            if rest[i] == "-regex" || rest[i] == "-iregex" {
                return Some(rest[i + 1].clone());
            }
        }
        return None;
    }
    let argopts: &[&str] = if tool == "string" {
        rest = if rest.is_empty() { rest } else { &rest[1..] };
        &[]
    } else if tool == "rg" {
        RG_ARGOPTS
    } else {
        GREP_ARGOPTS
    };
    let mut skip = false;
    for (i, t) in rest.iter().enumerate() {
        if skip {
            skip = false;
            continue;
        }
        if (t == "-e" || t == "--regexp" || t == "--expression") && i + 1 < rest.len() {
            return Some(rest[i + 1].clone());
        }
        if let Some(v) = t.strip_prefix("--regexp=") {
            return Some(v.to_string());
        }
        if t.starts_with('-') {
            skip = argopts.contains(&t.as_str());
            continue;
        }
        return Some(t.clone());
    }
    None
}

pub fn cmd_explain(args: &[String]) -> i32 {
    let toks: Vec<String> = match args.iter().position(|a| a == "--") {
        // bash/zsh pass the raw command line: `explain --line "$READLINE_LINE"`
        _ if args.first().map(|s| s.as_str()) == Some("--line") => {
            let l = extras::split_line(&args[1..].join(" "));
            let mut w = l.words;
            if !l.cur.is_empty() {
                w.push(l.cur);
            }
            w
        }
        Some(i) => args[i + 1..].to_vec(),
        None => args.to_vec(),
    };
    if toks.is_empty() {
        return 0;
    }
    let tool = basename(&toks[0]).to_string();
    let color = util::is_tty(1);
    if tool == "find" {
        println!("{}", findhelp::explain(&toks));
        if let Some(p) = pattern_of(&toks) {
            println!();
            println!("{}", rx::render_explain(&p, &rx::detect(&toks).1, color));
        }
        return 0;
    }
    if ["grep", "egrep", "fgrep", "rgrep", "zgrep", "ugrep", "rg", "sed", "gsed", "string", "awk"].contains(&tool.as_str()) {
        if let Some(p) = pattern_of(&toks) {
            println!("{}", rx::render_explain(&p, &rx::detect(&toks).1, color));
            return 0;
        }
    }
    let Some(model) = probe::gather(&tool, true) else {
        println!("{tool}: no help available to explain with");
        return 1;
    };
    let mut node = &model;
    for t in &toks[1..] {
        if t.starts_with('-') {
            let flag = t.split('=').next().unwrap_or(t);
            let o = node.option(flag).or_else(|| model.option(flag));
            if o.is_none() && !t.starts_with("--") && util::char_len(t) > 2 {
                for ch in t.chars().skip(1) {
                    let f = format!("-{ch}");
                    let oo = node.option(&f).or_else(|| model.option(&f));
                    println!("  -{ch:<22} {}", oo.map(|o| o.desc.as_str()).unwrap_or("?"));
                }
                continue;
            }
            let d = o.map(|o| o.desc.as_str()).filter(|d| !d.is_empty()).unwrap_or("?");
            println!("  {t:<23} {d}");
        } else if let Some(sub) = node.subcommands.get(t.as_str()) {
            node = sub;
            println!("  {t:<23} subcommand: {}", node.desc);
        } else {
            println!("  {t:<23} argument");
        }
    }
    0
}
