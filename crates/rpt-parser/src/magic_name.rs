//! Port of upstream `renderer/src/parser/magic-name.ts`.

use rpt_data::{GameData, Namespace};

use crate::js;

/// Upstream `magicBasetype`: finds the longest contiguous word sequence of a
/// magic item name that is a craftable base type.
pub fn magic_basetype(name: &str) -> Option<String> {
    magic_basetype_with(&rpt_data::data(), name)
}

pub(crate) fn magic_basetype_with(data: &GameData, name: &str) -> Option<String> {
    // Chinese and Japanese don't use spaces to separate words, so fallback to characters
    let (separator, words): (&str, Vec<String>) = if name.contains(' ') {
        (" ", name.split(' ').map(str::to_owned).collect())
    } else {
        ("", name.chars().map(String::from).collect())
    };

    let mut perm: Vec<String> = Vec::new();
    for start in 0..words.len() {
        for idx in 0..(words.len() - start) {
            perm.push(words[start..start + idx + 1].join(separator));
        }
    }

    let mut result: Vec<String> = perm
        .into_iter()
        .filter(|name| {
            crate::parser::item_by_translated(data, Namespace::Item, name)
                .is_some_and(|r| r[0].craftable.is_some())
        })
        .collect();
    // stable sort, like JS
    result.sort_by_key(|n| std::cmp::Reverse(js::js_len(n)));
    result.into_iter().next()
}
