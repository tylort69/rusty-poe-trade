//! App UI translations from upstream `app_i18n.json` (vue-i18n format).
//! Supports nested keys (`"price_check.title"`), `{0}`/`{name}` placeholders,
//! and fallback to English.

use std::sync::Arc;

use serde_json::Value;

#[derive(Clone)]
pub struct I18n {
    lang: Arc<Value>,
    en: Arc<Value>,
}

impl I18n {
    pub fn new(data: &rpt_data::GameData) -> Self {
        let en = if data.language == "en" {
            data.app_i18n.clone()
        } else {
            rpt_data::GameData::load("en").map(|d| d.app_i18n).unwrap_or(Value::Null)
        };
        Self { lang: Arc::new(data.app_i18n.clone()), en: Arc::new(en) }
    }

    fn lookup<'a>(root: &'a Value, key: &str) -> Option<&'a str> {
        // Some upstream keys are literal dotted strings at the top level (`"map.mods.heist"`).
        if let Some(s) = root.get(key).and_then(Value::as_str) {
            return Some(s);
        }
        let mut cur = root;
        for part in key.split('.') {
            cur = cur.get(part)?;
        }
        cur.as_str()
    }

    /// Translate `key`, falling back to English, then to the key itself.
    pub fn t(&self, key: &str) -> String {
        Self::lookup(&self.lang, key)
            .or_else(|| Self::lookup(&self.en, key))
            .map(str::to_owned)
            .unwrap_or_else(|| key.to_owned())
    }

    /// Translate with positional args: `{0}`, `{1}`, ...
    pub fn tf(&self, key: &str, args: &[&str]) -> String {
        let mut s = self.t(key);
        for (i, a) in args.iter().enumerate() {
            s = s.replace(&format!("{{{i}}}"), a);
        }
        s
    }

    /// Translate with named args: `{name}`.
    pub fn tn(&self, key: &str, args: &[(&str, &str)]) -> String {
        let mut s = self.t(key);
        for (k, v) in args {
            s = s.replace(&format!("{{{k}}}"), v);
        }
        s
    }

    /// vue-i18n plural form "a | b | c": picks by count.
    pub fn tc(&self, key: &str, n: i64) -> String {
        let s = self.t(key);
        let forms: Vec<&str> = s.split(" | ").collect();
        let idx = match forms.len() {
            1 => 0,
            2 => usize::from(n != 1),
            _ => (n.max(0) as usize).min(forms.len() - 1),
        };
        forms[idx].replace("{n}", &n.to_string()).replace("{count}", &n.to_string())
    }
}
