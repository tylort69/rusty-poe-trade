use std::fmt;

use serde::{Deserialize, Deserializer};

/// A JavaScript regex literal from upstream `client_strings.js`, compiled with
/// `fancy_regex` (supports lookaround/backrefs like JS does).
#[derive(Clone)]
pub struct JsRegex {
    pub source: String,
    re: fancy_regex::Regex,
}

impl JsRegex {
    pub fn new(source: &str, flags: &str) -> Result<Self, fancy_regex::Error> {
        let mut prefix = String::new();
        if flags.contains('i') {
            prefix.push_str("(?i)");
        }
        if flags.contains('s') {
            prefix.push_str("(?s)");
        }
        if flags.contains('m') {
            prefix.push_str("(?m)");
        }
        let re = fancy_regex::Regex::new(&format!("{prefix}{source}"))?;
        Ok(Self { source: source.to_owned(), re })
    }

    pub fn is_match(&self, text: &str) -> bool {
        self.re.is_match(text).unwrap_or(false)
    }

    pub fn captures<'t>(&self, text: &'t str) -> Option<fancy_regex::Captures<'t, str>> {
        self.re.captures(text).ok().flatten()
    }

    /// First capture group, the common `str.match(re)?.[1]` idiom upstream.
    pub fn group1<'t>(&self, text: &'t str) -> Option<&'t str> {
        self.captures(text).and_then(|c| c.get(1)).map(|m| m.as_str())
    }

    pub fn replace(&self, text: &str, rep: &str) -> String {
        self.re.replace(text, rep).into_owned()
    }

    pub fn inner(&self) -> &fancy_regex::Regex {
        &self.re
    }
}

impl fmt::Debug for JsRegex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "/{}/", self.source)
    }
}

impl<'de> Deserialize<'de> for JsRegex {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            #[serde(rename = "$re")]
            re: String,
            #[serde(default)]
            flags: String,
        }
        let raw = Raw::deserialize(d)?;
        JsRegex::new(&raw.re, &raw.flags).map_err(serde::de::Error::custom)
    }
}
