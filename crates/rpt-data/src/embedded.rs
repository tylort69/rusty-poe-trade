//! Game data embedded at compile time from `assets/data`.

pub struct LangFiles {
    pub client_strings: &'static str,
    pub items: &'static str,
    pub stats: &'static str,
    pub app_i18n: &'static str,
}

macro_rules! lang {
    ($l:literal) => {
        LangFiles {
            client_strings: include_str!(concat!("../../../assets/data/", $l, "/client_strings.json")),
            items: include_str!(concat!("../../../assets/data/", $l, "/items.ndjson")),
            stats: include_str!(concat!("../../../assets/data/", $l, "/stats.ndjson")),
            app_i18n: include_str!(concat!("../../../assets/data/", $l, "/app_i18n.json")),
        }
    };
}

pub fn files(language: &str) -> Option<LangFiles> {
    Some(match language {
        "en" => lang!("en"),
        "ru" => lang!("ru"),
        "ko" => lang!("ko"),
        "cmn-Hant" => lang!("cmn-Hant"),
        _ => return None,
    })
}

pub const ITEM_DROP: &str = include_str!("../../../assets/data/item-drop.json");
pub const PATRONS: &str = include_str!("../../../assets/data/patrons.json");
