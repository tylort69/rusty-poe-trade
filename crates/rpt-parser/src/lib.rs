//! Item clipboard parser for Rusty POE Trade.
//! Port of upstream `renderer/src/parser/*`.

pub mod modifiers;
pub mod types;
pub use types::*;

/// Upstream `parseClipboard`. Returns `Err(message_key)` for unrecognised items
/// (upstream used i18n keys like `item.parse_error`).
pub fn parse_clipboard(_clipboard: &str) -> Result<ParsedItem, String> {
    todo!("ported by the parser task")
}
