//! Port of upstream `filters/pseudo/reflection-rules.ts` (Mirrored Tablet).

use crate::filters::StatFilter;

struct FilterRule {
    ref_: &'static str,
    disabled: Option<bool>,
    difficulty_any: bool,
}

const fn r(ref_: &'static str) -> FilterRule {
    FilterRule { ref_, disabled: None, difficulty_any: false }
}
const fn enabled(ref_: &'static str) -> FilterRule {
    FilterRule { ref_, disabled: Some(false), difficulty_any: false }
}

const RULES: &[FilterRule] = &[
    r("Reflection of the Breachlord (Difficulty #)"),
    FilterRule { ref_: "Reflection of Delirium (Difficulty #)", disabled: None, difficulty_any: true },
    r("Reflection of Tyranny (Difficulty #)"),
    r("Reflection of the Trove (Difficulty #)"),
    r("Reflection of Thralldom (Difficulty #)"),
    r("Reflection of Phaaryl (Difficulty #)"),
    r("Reflection of Perverted Faith (Difficulty #)"),
    enabled("Reflection of Kalandra (Difficulty #)"),
    r("Reflection of Experimentation (Difficulty #)"),
    enabled("Reflection of the Sun (Difficulty #)"),
    r("Reflection of the Monolith (Difficulty #)"),
    r("Reflection of the Nightmare (Difficulty #)"),
    r("Reflection of Azurite (Difficulty #)"),
    enabled("Reflection of Paradise (Difficulty #)"),
    enabled("Reflection of Angling (Difficulty #)"),
];

pub fn used_stat_refs() -> Vec<&'static str> {
    RULES.iter().map(|r| r.ref_).collect()
}

pub fn apply_rules(filters: &mut Vec<StatFilter>) {
    let mut i = 0;
    while i < filters.len() {
        let filter = &mut filters[i];
        match RULES.iter().find(|rule| rule.ref_ == filter.stat_ref) {
            None => {
                // NOTE: upstream reads `filter.roll!.value` here
                let difficulty = filter.roll.as_ref().map(|r| r.value).unwrap_or(f64::INFINITY);
                if difficulty < 8.0 {
                    filters.remove(i);
                    continue;
                } else {
                    filter.hidden = Some("low_tier_reflection".to_owned());
                }
            }
            Some(rule) => {
                if rule.difficulty_any {
                    filter.roll = None;
                }
                if let Some(d) = rule.disabled {
                    filter.disabled = d;
                }
            }
        }
        i += 1;
    }
}
