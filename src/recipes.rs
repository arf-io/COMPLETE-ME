//! Project recipes are literal argv templates. Completion reuses the live providers.
use crate::util::{fq, sq};
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{self, Read};

type Recipes = BTreeMap<String, Vec<String>>;
const MAX_INPUT: u64 = 256 * 1024;

fn name_ok(word: &str) -> bool {
    !word.is_empty() && word.len() <= 64
        && word.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn slot(word: &str) -> Option<&str> {
    word.strip_prefix('{')?.strip_suffix('}').filter(|name| name_ok(name))
}

fn read() -> Result<Recipes, &'static str> {
    let cwd = std::env::current_dir().map_err(|_| "cannot determine the working directory")?;
    for ancestor in cwd.ancestors() {
        let mut reader = match crate::util::open_regular(ancestor.join(".complete-me/recipes.json")) {
            Ok(reader) => reader.take(MAX_INPUT + 1),
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(_) => return Err("cannot read .complete-me/recipes.json"),
        };
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).map_err(|_| "cannot read .complete-me/recipes.json")?;
        if bytes.len() as u64 > MAX_INPUT {
            return Err(".complete-me/recipes.json exceeds 256 KiB");
        }
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| "invalid JSON in .complete-me/recipes.json")?;
        let map = value.as_object().ok_or("expected an object of recipe names and argument arrays")?;
        return map.iter().map(|(name, argv)| {
            if !name_ok(name) {
                return Err("invalid recipe name");
            }
            let argv: Vec<String> = argv.as_array().ok_or("expected a recipe argument array")?.iter().map(|word| {
                word.as_str().filter(|s| !s.chars().any(char::is_control)).map(str::to_owned)
                    .ok_or("expected a string without control characters")
            }).collect::<Result<_, _>>()?;
            if !argv.first().is_some_and(|cmd| !cmd.is_empty() && slot(cmd).is_none()) {
                return Err("expected a literal command in every recipe");
            }
            Ok((name.to_owned(), argv))
        }).collect();
    }
    Ok(Recipes::new())
}

fn slots(argv: &[String]) -> Vec<&str> {
    let mut slots = vec![];
    for name in argv.iter().filter_map(|word| slot(word)) {
        if !slots.contains(&name) {
            slots.push(name);
        }
    }
    slots
}

fn render(argv: impl IntoIterator<Item = impl AsRef<str>>, fish: bool) -> String {
    argv.into_iter().enumerate().map(|(index, word)| {
        let word = word.as_ref();
        // Quote the command word to keep reserved words and assignment-like names literal.
        if index > 0 && !word.is_empty() && word.bytes().all(|b| b.is_ascii_alphanumeric() || b"_-./:@%+,=".contains(&b)) {
            word.to_owned()
        } else if fish {
            fq(word)
        } else {
            sq(word)
        }
    }).collect::<Vec<_>>().join(" ")
}

fn expand(argv: &[String], values: &[String], fish: bool) -> Result<String, &'static str> {
    let slots = slots(argv);
    if values.len() != slots.len() || values.iter().any(|word| word.chars().any(char::is_control)) {
        return Err("expected one literal value per recipe slot (see recipe show NAME)");
    }
    let words = argv.iter().map(|word| {
        slot(word).and_then(|name| slots.iter().position(|&s| s == name))
            .map_or(word, |i| &values[i])
    });
    Ok(render(words, fish))
}

fn recipe_args(words: &[String]) -> Option<&[String]> {
    match words {
        [command, rest @ ..] if command == "cm-recipe" => Some(rest),
        [command, sub, rest @ ..] if command == "complete-me" && sub == "recipe" => Some(rest),
        _ => None,
    }
}

pub(crate) fn candidates(words: &[String], current: &str) -> Vec<(String, String)> {
    let Some(args) = recipe_args(words) else { return vec![] };
    match args {
        [] => ["list", "show", "expand"].into_iter()
            .filter(|name| name.starts_with(current)).map(|name| (name.into(), String::new())).collect(),
        [mode] if matches!(mode.as_str(), "show" | "expand") => {
            read().unwrap_or_default().into_iter().filter(|(name, _)| name.starts_with(current))
                .map(|(name, argv)| (name, crate::render::clean_desc(&argv.join(" ")))).collect()
        }
        [mode, name, values @ ..] if mode == "expand" => {
            let Ok(recipes) = read() else { return vec![] };
            let Some(argv) = recipes.get(name) else { return vec![] };
            let slots = slots(argv);
            let Some(current_slot) = slots.get(values.len()) else { return vec![] };
            // Only known providers can be invoked by completion. The first
            // unresolved word is the cursor slot in the underlying command.
            if !crate::live::has_provider(&argv[0]) {
                return vec![];
            }
            let prefix: Vec<_> = argv.iter().take_while(|word| slot(word) != Some(current_slot))
                .map(|word| match slot(word) {
                    Some(s) => values[slots.iter().position(|&name| name == s).unwrap()].clone(),
                    None => word.clone(),
                })
                .collect();
            crate::live::candidates(&prefix, current)
        }
        _ => vec![],
    }
}

pub(crate) fn run(args: &[String]) -> i32 {
    let result = match args {
        [mode] if mode == "list" => read().map(|recipes| {
            for (name, argv) in recipes {
                println!("{name}\t{}", crate::render::clean_desc(&argv.join(" ")));
            }
        }),
        [mode, name] if mode == "show" => read().and_then(|recipes| {
            let argv = recipes.get(name).ok_or("unknown recipe")?;
            println!("{}", render(argv, false));
            let slots = slots(argv);
            if !slots.is_empty() {
                println!("Slots: {}", slots.join(", "));
            }
            Ok(())
        }),
        [mode, shell, values @ ..] if mode == "_complete" && shell == "fish" => {
            let mut words = vec!["complete-me".into(), "recipe".into()];
            let (current, prior) = values.split_last().map_or(("", &[][..]), |(last, prior)| (last.as_str(), prior));
            words.extend_from_slice(prior);
            for (value, description) in candidates(&words, current) {
                println!("{value}\t{description}");
            }
            Ok(())
        }
        [mode, name, values @ ..] if mode == "expand" => read().and_then(|recipes| {
            let argv = recipes.get(name).ok_or("unknown recipe")?;
            println!("{}", expand(argv, values, false)?);
            Ok(())
        }),
        [mode, shell] if mode == "_edit" && matches!(shell.as_str(), "fish" | "zsh" | "bash") => {
            return edit(shell == "fish");
        }
        _ => {
            eprintln!("usage: complete-me recipe list | show NAME | expand NAME [VALUE…]");
            return 2;
        }
    };
    if let Err(err) = result {
        eprintln!("complete-me recipe: {err}");
        return 1;
    }
    0
}

fn edit(fish: bool) -> i32 {
    let mut line = String::new();
    if io::stdin().take(MAX_INPUT + 1).read_to_string(&mut line).is_err() || line.len() as u64 > MAX_INPUT {
        return 1;
    }
    let Some(words) = crate::cursor::literal_words(&line, fish) else { return 1 };
    let Some([mode, name, values @ ..]) = recipe_args(&words) else { return 1 };
    if mode != "expand" {
        return 1;
    }
    let Ok(recipes) = read() else { return 1 };
    let Some(argv) = recipes.get(name) else { return 1 };
    let Ok(line) = expand(argv, values, fish) else { return 1 };
    println!("{line}");
    0
}
