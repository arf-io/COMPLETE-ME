//! bash, understood: a parser (`parse`) and a translator to fish (`fish`). Built in, so typing
//! bash into fish needs nothing else installed.
pub mod ast;
pub mod fish;
pub mod parse;
pub mod pattern;

/// Translate bash source to fish source.
pub fn to_fish(src: &str) -> Result<String, String> {
    let f = parse::parse(src)?;
    fish::translate(&f)
}
