//! Item clipboard parser for Rusty POE Trade.
//! Port of upstream `renderer/src/parser/*`.

pub mod advanced_mod_desc;
mod js;
pub mod magic_name;
pub mod modifiers;
mod parser;
pub mod stat_translations;
pub mod types;
pub use types::*;

pub use parser::{make_identified_unique, parse_clipboard};
