//! Port of upstream `client-log/client-log.ts`: parses chat lines from the
//! game's `Client.txt` (channel, sender, trade whispers).

use std::sync::OnceLock;

use regex::Regex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageChannel {
    System,
    Trade,
    Global,
    Party,
    Guild,
    WhisperFrom,
    WhisperTo,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TradeGem {
    pub level: u32,
    pub quality: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TradeItem {
    pub name: String,
    pub amount: Option<f64>,
    pub gem: Option<TradeGem>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TradePrice {
    pub name: String,
    pub amount: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TradeTab {
    pub name: String,
    pub left: u32,
    pub top: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TradeRequest {
    pub item: TradeItem,
    pub price: TradePrice,
    pub tab: Option<TradeTab>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LogEntry {
    pub channel: MessageChannel,
    pub guild_tag: Option<String>,
    pub char_name: Option<String>,
    pub body: String,
    pub trade: Option<TradeRequest>,
}

fn trade_whisper(lang: &str) -> Option<&'static Regex> {
    static RE: OnceLock<Vec<(&'static str, Regex)>> = OnceLock::new();
    let all = RE.get_or_init(|| {
        [
            ("en", r#"^Hi, I would like to buy your (?<item>.+) listed for (?<price>.+) in (?<league>.+) \(stash tab "(?<tab_name>.*)"; position: left (?<tab_left>\d+), top (?<tab_top>\d+)\)(?<message>.+)?$"#),
            ("ru", r#"^Здравствуйте, хочу купить у вас (?<item>.+) за (?<price>.+) в лиге (?<league>.+) \(секция "(?<tab_name>.*)"; позиция: (?<tab_left>\d+) столбец, (?<tab_top>\d+) ряд\)(?<message>.+)?$"#),
            ("ko", r#"^안녕하세요, (?<league>.+)\(보관함 탭 "(?<tab_name>.*)", 위치: 왼쪽 (?<tab_left>\d+), 상단 (?<tab_top>\d+)\)에 (?<price>.+)\(으\)로 올려놓은 (?<item>.+)\(을\)를 구매하고 싶습니다(?<message>.+)?$"#),
            ("de", r#"^Hi, ich möchte '(?<item>.+)' zum angebotenen Preis von (?<price>.+) in der (?<league>.+)-Liga kaufen \(Truhenfach "(?<tab_name>.*)"; Position: (?<tab_left>\d+) von links, (?<tab_top>\d+) von oben\)(?<message>.+)?$"#),
            ("fr", r#"^Bonjour, je souhaiterais t'acheter (?<item>.+) pour (?<price>.+) dans la ligue (?<league>.+) \(onglet de réserve "(?<tab_name>.*)" ; (?<tab_left>\d+)e en partant de la gauche, (?<tab_top>\d+)e en partant du haut\)(?<message>.+)?$"#),
            ("es", r#"^Hola, quisiera comprar tu (?<item>.+) listado por (?<price>.+) en (?<league>.+) \(pestaña de alijo "(?<tab_name>.*)"; posición: izquierda(?<tab_left>\d+), arriba (?<tab_top>\d+)\)(?<message>.+)?$"#),
            ("pt", r#"^Olá, eu gostaria de comprar o seu item (?<item>.+) listado por (?<price>.+) na (?<league>.+) \(aba do baú: "(?<tab_name>.*)"; posição: esquerda (?<tab_left>\d+), topo (?<tab_top>\d+)\)(?<message>.+)?$"#),
            ("th", r#"^สวัสดี, เราต้องการจะชื้อของคุณ (?<item>.+) ใน ราคา (?<price>.+) ใน (?<league>.+) \(stash tab "(?<tab_name>.*)"; ตำแหน่ง: ซ้าย (?<tab_left>\d+), บน (?<tab_top>\d+)\)(?<message>.+)?$"#),
            ("cmn-Hant", r#"^你好，我想購買 (?<item>.+) 標價 (?<price>.+) 在 (?<league>.+) \(倉庫頁 "(?<tab_name>.*)"; 位置: 左 (?<tab_left>\d+), 上 (?<tab_top>\d+)\)(?<message>.+)?$"#),
        ]
        .into_iter()
        .filter_map(|(l, s)| Regex::new(s).ok().map(|r| (l, r)))
        .collect()
    });
    all.iter().find(|(l, _)| *l == lang).map(|(_, r)| r)
}

fn trade_bulk_whisper(lang: &str) -> Option<&'static Regex> {
    static RE: OnceLock<Vec<(&'static str, Regex)>> = OnceLock::new();
    let all = RE.get_or_init(|| {
        [
            ("en", r#"^Hi, I'd like to buy your (?<item>.+) for my (?<price>.+) in (?<league>.+)\.(?<message>.+)?$"#),
            ("ru", r#"^Здравствуйте, хочу купить у вас (?<item>.+) за (?<price>.+) в лиге (?<league>.+)\.(?<message>.+)?$"#),
            ("cmn-Hant", r#"^你好，我想用 (?<price>.+) 購買 (?<item>.+) in (?<league>.+)\.(?<message>.+)?$"#),
            ("ko", r#"^_FIX_ME_$"#),
        ]
        .into_iter()
        .filter_map(|(l, s)| Regex::new(s).ok().map(|r| (l, r)))
        .collect()
    });
    all.iter().find(|(l, _)| *l == lang).map(|(_, r)| r)
}

/// `"10 chaos"` → (`"10"`, `"chaos"`) like `split(' ', 1)` / `split(' ').slice(1)`.
fn split_amount(s: &str) -> (&str, &str) {
    s.split_once(' ').unwrap_or((s, ""))
}

fn js_number(s: &str) -> f64 {
    s.trim().parse().unwrap_or(f64::NAN)
}

/// Upstream `handleLine`. `language` is the app language (`AppConfig().language`).
pub fn handle_line(line: &str, language: &str) -> Option<LogEntry> {
    let text = line.split("] ").skip(1).collect::<Vec<_>>().join("] ");
    if text.is_empty() {
        return None;
    }
    let data = rpt_data::data();
    let cs = &data.client_strings;
    let channels = [
        (&cs.CHAT_GLOBAL, MessageChannel::Global),
        (&cs.CHAT_TRADE, MessageChannel::Trade),
        (&cs.CHAT_SYSTEM, MessageChannel::System),
        (&cs.CHAT_WHISPER_TO, MessageChannel::WhisperTo),
        (&cs.CHAT_WHISPER_FROM, MessageChannel::WhisperFrom),
        (&cs.CHAT_PARTY, MessageChannel::Party),
        (&cs.CHAT_GUILD, MessageChannel::Guild),
    ];
    let (caps, channel) = channels.iter().find_map(|(re, ch)| re.captures(&text).map(|c| (c, *ch)))?;
    let group = |n: &str| caps.name(n).map(|m| m.as_str().to_owned());
    let mut entry = LogEntry { channel, guild_tag: group("guild_tag"), char_name: group("char_name"), body: group("body").unwrap_or_default(), trade: None };

    if channel == MessageChannel::WhisperFrom {
        if let Some(m) = trade_whisper(language).and_then(|re| re.captures(&entry.body)) {
            let (p_amount, p_tag) = split_amount(m.name("price").map(|x| x.as_str()).unwrap_or(""));
            let mut item_name = m.name("item").map(|x| x.as_str().to_owned()).unwrap_or_default();
            let gem = cs.CHAT_WEBTRADE_GEM.captures(&item_name).map(|g| {
                let gem = TradeGem {
                    level: g.name("gem_lvl").and_then(|x| x.as_str().parse().ok()).unwrap_or(0),
                    quality: g.name("gem_qual").and_then(|x| x.as_str().parse().ok()).unwrap_or(0),
                };
                (g.name("gem_name").map(|x| x.as_str().to_owned()).unwrap_or_default(), gem)
            });
            let gem = gem.map(|(name, g)| {
                item_name = name;
                g
            });
            entry.trade = Some(TradeRequest {
                item: TradeItem { name: item_name, amount: None, gem },
                price: TradePrice { amount: js_number(p_amount), name: p_tag.to_owned() },
                tab: Some(TradeTab {
                    name: m.name("tab_name").map(|x| x.as_str().to_owned()).unwrap_or_default(),
                    top: m.name("tab_top").and_then(|x| x.as_str().parse().ok()).unwrap_or(0),
                    left: m.name("tab_left").and_then(|x| x.as_str().parse().ok()).unwrap_or(0),
                }),
            });
        } else if let Some(m) = trade_bulk_whisper(language).and_then(|re| re.captures(&entry.body)) {
            let (p_amount, p_name) = split_amount(m.name("price").map(|x| x.as_str()).unwrap_or(""));
            let (i_amount, i_name) = split_amount(m.name("item").map(|x| x.as_str()).unwrap_or(""));
            entry.trade = Some(TradeRequest {
                item: TradeItem { name: i_name.to_owned(), amount: Some(js_number(i_amount)), gem: None },
                price: TradePrice { amount: js_number(p_amount), name: p_name.to_owned() },
                tab: None,
            });
        }
    }
    Some(entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_trade_whispers() {
        rpt_data::init("en").unwrap();
        let line = r#"2024/01/01 10:00:00 123 abc [INFO Client 1] @From <GLD> Buyer: Hi, I would like to buy your level 21 20% Vaal Grace listed for 5 chaos in Standard (stash tab "~price"; position: left 3, top 7)"#;
        let e = handle_line(line, "en").unwrap();
        assert_eq!(e.channel, MessageChannel::WhisperFrom);
        assert_eq!(e.guild_tag.as_deref(), Some("GLD"));
        assert_eq!(e.char_name.as_deref(), Some("Buyer"));
        let t = e.trade.unwrap();
        assert_eq!(t.item.name, "Vaal Grace");
        assert_eq!(t.item.gem, Some(TradeGem { level: 21, quality: 20 }));
        assert_eq!(t.price, TradePrice { amount: 5.0, name: "chaos".into() });
        assert_eq!(t.tab, Some(TradeTab { name: "~price".into(), left: 3, top: 7 }));

        let bulk = "2024/01/01 10:00:00 1 a [INFO Client 1] @From Buyer: Hi, I'd like to buy your 30 Chaos Orb for my 1 Divine Orb in Standard.";
        let t = handle_line(bulk, "en").unwrap().trade.unwrap();
        assert_eq!(t.item, TradeItem { name: "Chaos Orb".into(), amount: Some(30.0), gem: None });
        assert_eq!(t.price.name, "Divine Orb");

        let global = "2024/01/01 10:00:00 1 a [INFO Client 1] #Someone: hello";
        assert_eq!(handle_line(global, "en").unwrap().channel, MessageChannel::Global);
        assert!(handle_line("no bracket here", "en").is_none());
    }
}
