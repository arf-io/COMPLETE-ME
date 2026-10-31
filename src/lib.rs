//! COMPLETE-ME: tab completion for everything.
pub mod util;
pub mod model;
pub mod parse;
pub mod scriptscan;
pub mod probe;
pub mod render;
pub mod cli;
pub mod rx;
pub mod findhelp;
pub mod sshopts;
pub mod aliases;
pub mod snippets;
pub mod bashfish;
pub mod extras;
pub mod sh;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
