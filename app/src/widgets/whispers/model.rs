//! Trade whisper bookkeeping (no UI): cards built from `@From` / `@To` trade
//! whispers, updated by area join/leave and trade-accepted system lines.

use crate::widgets::item_check::client_log::{self, MessageChannel, TradeGem, TradePrice, TradeTab};
use crate::widgets::mapping::parse::{LogEvent, LogSecs, log_timestamp, parse_line};

/// Maximum number of cards kept.
pub const CAP: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    /// `@From`: someone wants to buy from us.
    Incoming,
    /// `@To`: we want to buy from someone.
    Outgoing,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Whisper {
    pub id: u64,
    pub dir: Dir,
    pub player: String,
    pub guild: Option<String>,
    pub item: String,
    pub quantity: Option<f64>,
    pub gem: Option<TradeGem>,
    pub price: TradePrice,
    pub league: String,
    pub tab: Option<TradeTab>,
    /// First whisper (log time).
    pub received: LogSecs,
    /// Latest repeat (log time).
    pub last: LogSecs,
    /// Number of identical whispers grouped into this card.
    pub count: u32,
    /// Latest original message body.
    pub raw: String,
    pub seen: bool,
    /// We sent `/invite` (incoming) or `/hideout` (outgoing).
    pub invited: bool,
    /// Player is currently in our area.
    pub in_area: bool,
    /// Player joined and left again.
    pub left: bool,
    pub completed: bool,
    /// When "Trade accepted." was logged for this card (0 = not traded).
    pub completed_at: LogSecs,
}

impl Whisper {
    /// Item label with quantity / gem level+quality.
    pub fn item_label(&self) -> String {
        let mut s = String::new();
        if let Some(q) = self.quantity.filter(|q| q.is_finite()) {
            s.push_str(&format!("{}× ", rpt_m3::fmt_number(q)));
        }
        if let Some(g) = &self.gem {
            s.push_str(&format!("L{} Q{}% ", g.level, g.quality));
        }
        s.push_str(&self.item);
        s
    }

    pub fn price_label(&self) -> String {
        if self.price.amount.is_finite() {
            format!("{} {}", rpt_m3::fmt_number(self.price.amount), self.price.name).trim().to_owned()
        } else {
            self.price.name.clone()
        }
    }

    fn same_request(&self, dir: Dir, player: &str, item: &str, gem: &Option<TradeGem>) -> bool {
        self.dir == dir && self.player == player && self.item == item && &self.gem == gem && !self.completed
    }
}

/// Things the widget reacts to after feeding lines.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    NewIncoming { id: u64 },
    Completed { id: u64, dir: Dir, player: String },
}

#[derive(Debug, Default)]
pub struct WhisperBook {
    /// Newest first.
    pub list: Vec<Whisper>,
    next_id: u64,
    /// Player we last sent `/tradewith` to (claims the next "Trade accepted.").
    trade_target: Option<String>,
}

impl WhisperBook {
    /// Feed one raw Client.txt line. `language` is the app/game language.
    pub fn feed(&mut self, line: &str, language: &str) -> Vec<Effect> {
        let mut out = Vec::new();
        if let Some((ts, ev)) = parse_line(line) {
            match ev {
                LogEvent::Joined { name } => self.for_player(&name, |w| {
                    w.in_area = true;
                    w.left = false;
                }),
                LogEvent::Left { name } => self.for_player(&name, |w| {
                    if w.in_area {
                        w.left = true;
                    }
                    w.in_area = false;
                }),
                LogEvent::TradeAccepted => {
                    if let Some(i) = self.trade_card() {
                        let w = &mut self.list[i];
                        w.completed = true;
                        w.completed_at = ts;
                        out.push(Effect::Completed { id: w.id, dir: w.dir, player: w.player.clone() });
                    }
                    self.trade_target = None;
                }
                LogEvent::TradeCancelled => self.trade_target = None,
                _ => {}
            }
            return out;
        }
        let Some(entry) = client_log::handle_line(line, language) else { return out };
        let dir = match entry.channel {
            MessageChannel::WhisperFrom => Dir::Incoming,
            MessageChannel::WhisperTo => Dir::Outgoing,
            _ => return out,
        };
        let (Some(trade), Some(player)) = (entry.trade, entry.char_name) else { return out };
        let ts = log_timestamp(line).unwrap_or_default();
        if let Some(i) = self.list.iter().position(|w| w.same_request(dir, &player, &trade.item.name, &trade.item.gem)) {
            let mut w = self.list.remove(i);
            w.count += 1;
            w.last = ts;
            w.raw = entry.body;
            w.seen = false;
            w.price = trade.price;
            w.quantity = trade.item.amount;
            let id = w.id;
            self.list.insert(0, w);
            if dir == Dir::Incoming {
                out.push(Effect::NewIncoming { id });
            }
        } else {
            self.next_id += 1;
            let id = self.next_id;
            self.list.insert(
                0,
                Whisper {
                    id,
                    dir,
                    player,
                    guild: entry.guild_tag,
                    item: trade.item.name,
                    quantity: trade.item.amount,
                    gem: trade.item.gem,
                    price: trade.price,
                    league: trade.league,
                    tab: trade.tab,
                    received: ts,
                    last: ts,
                    count: 1,
                    raw: entry.body,
                    // our own outgoing whispers need no attention
                    seen: dir == Dir::Outgoing,
                    invited: false,
                    in_area: false,
                    left: false,
                    completed: false,
                    completed_at: 0,
                },
            );
            if dir == Dir::Incoming {
                out.push(Effect::NewIncoming { id });
            }
        }
        self.list.truncate(CAP);
        out
    }

    fn for_player(&mut self, name: &str, f: impl Fn(&mut Whisper)) {
        for w in self.list.iter_mut().filter(|w| w.player == name) {
            f(w);
        }
    }

    /// Which card a "Trade accepted." belongs to:
    /// 1. the newest open card of the player we last sent `/tradewith` to;
    /// 2. the newest open incoming card whose buyer is in our area;
    /// 3. the newest open outgoing card whose seller's hideout we visited.
    fn trade_card(&self) -> Option<usize> {
        let open = |w: &&Whisper| !w.completed;
        if let Some(t) = &self.trade_target {
            if let Some(i) = self.list.iter().position(|w| open(&w) && &w.player == t) {
                return Some(i);
            }
        }
        self.list
            .iter()
            .position(|w| open(&w) && w.dir == Dir::Incoming && w.in_area)
            .or_else(|| self.list.iter().position(|w| open(&w) && w.dir == Dir::Outgoing && w.invited))
    }

    /// Quick action "Invite": the newest open incoming whisper, preferring one
    /// not invited yet.
    pub fn invite_target(&self) -> Option<&Whisper> {
        let open = || self.list.iter().filter(|w| w.dir == Dir::Incoming && !w.completed);
        open().find(|w| !w.invited).or_else(|| open().next())
    }

    /// Quick action "Trade": the newest open incoming buyer in your area,
    /// else the newest one you invited.
    pub fn trade_target(&self) -> Option<&Whisper> {
        let open = || self.list.iter().filter(|w| w.dir == Dir::Incoming && !w.completed);
        open().find(|w| w.in_area).or_else(|| open().find(|w| w.invited))
    }

    /// Quick actions "Thanks" / "Kick": the player you traded with most
    /// recently, else the buyer in your area / party right now.
    pub fn party_target(&self) -> Option<&Whisper> {
        self.list
            .iter()
            .filter(|w| w.completed)
            .max_by_key(|w| w.completed_at)
            .or_else(|| self.trade_target())
    }

    pub fn get(&self, id: u64) -> Option<&Whisper> {
        self.list.iter().find(|w| w.id == id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Whisper> {
        self.list.iter_mut().find(|w| w.id == id)
    }

    pub fn dismiss(&mut self, id: u64) {
        self.list.retain(|w| w.id != id);
    }

    pub fn dismiss_completed(&mut self) {
        self.list.retain(|w| !w.completed);
    }

    pub fn set_trade_target(&mut self, player: &str) {
        self.trade_target = Some(player.to_owned());
    }

    pub fn unseen(&self) -> usize {
        self.list.iter().filter(|w| !w.seen).count()
    }

    pub fn mark_all_seen(&mut self) {
        for w in &mut self.list {
            w.seen = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(hms: &str, msg: &str) -> String {
        format!("2024/01/01 {hms} 123456789 cffb0719 [INFO Client 1234] {msg}")
    }

    const BUY: &str = r#"@From <GLD> Buyer: Hi, I would like to buy your Tabula Rasa Simple Robe listed for 10 chaos in Settlers (stash tab "~price 10 chaos"; position: left 3, top 7)"#;

    #[test]
    fn incoming_lifecycle() {
        rpt_data::init("en").unwrap();
        let mut b = WhisperBook::default();
        let fx = b.feed(&l("12:00:00", BUY), "en");
        assert_eq!(fx, vec![Effect::NewIncoming { id: 1 }]);
        let w = &b.list[0];
        assert_eq!((w.dir, w.player.as_str(), w.guild.as_deref()), (Dir::Incoming, "Buyer", Some("GLD")));
        assert_eq!(w.item, "Tabula Rasa Simple Robe");
        assert_eq!(w.price_label(), "10 chaos");
        assert_eq!(w.league, "Settlers");
        assert_eq!(w.tab, Some(TradeTab { name: "~price 10 chaos".into(), left: 3, top: 7 }));
        assert_eq!(b.unseen(), 1);

        // repeated whisper groups into the same card
        b.feed(&l("12:00:30", BUY), "en");
        assert_eq!(b.list.len(), 1);
        assert_eq!(b.list[0].count, 2);
        assert_eq!(b.list[0].last - b.list[0].received, 30);

        b.feed(&l("12:01:00", ": Buyer has joined the area."), "en");
        assert!(b.list[0].in_area);
        b.feed(&l("12:01:10", ": Somebody has joined the area."), "en");
        let fx = b.feed(&l("12:01:30", ": Trade accepted."), "en");
        assert_eq!(fx, vec![Effect::Completed { id: 1, dir: Dir::Incoming, player: "Buyer".into() }]);
        assert!(b.list[0].completed);
        b.feed(&l("12:01:40", ": Buyer has left the area."), "en");
        assert!(!b.list[0].in_area);

        // a new whisper after completion is a new card
        b.feed(&l("12:05:00", BUY), "en");
        assert_eq!(b.list.len(), 2);
        assert!(!b.list[0].completed);
        b.dismiss_completed();
        assert_eq!(b.list.len(), 1);
    }

    #[test]
    fn join_leave_and_trade_target() {
        rpt_data::init("en").unwrap();
        let mut b = WhisperBook::default();
        b.feed(&l("12:00:00", BUY), "en");
        b.feed(&l("12:00:01", "@From Other: Hi, I'd like to buy your 30 Chaos Orb for my 1 Divine Orb in Settlers."), "en");
        assert_eq!(b.list[0].item_label(), "30× Chaos Orb");
        assert_eq!(b.list[0].league, "Settlers");
        b.feed(&l("12:00:05", ": Buyer has joined the area."), "en");
        b.feed(&l("12:00:06", ": Buyer has left the area."), "en");
        let buyer = b.list.iter().find(|w| w.player == "Buyer").unwrap();
        assert!(buyer.left && !buyer.in_area);
        // trade target wins over "in area"
        b.feed(&l("12:00:07", ": Other has joined the area."), "en");
        b.set_trade_target("Buyer");
        b.feed(&l("12:00:08", ": Trade cancelled."), "en");
        assert!(b.feed(&l("12:00:09", ": Trade accepted."), "en").iter().any(|e| matches!(e, Effect::Completed { player, .. } if player == "Other")));
        // nobody left in the area / targeted → unclaimed
        assert!(b.feed(&l("12:00:10", ": Trade accepted."), "en").is_empty());
    }

    #[test]
    fn outgoing_and_cap() {
        rpt_data::init("en").unwrap();
        let mut b = WhisperBook::default();
        let to = r#"@To Seller: Hi, I would like to buy your level 21 20% Vaal Grace listed for 5 chaos in Standard (stash tab "Gems"; position: left 1, top 2)"#;
        assert!(b.feed(&l("12:00:00", to), "en").is_empty());
        let w = &b.list[0];
        assert_eq!((w.dir, w.seen), (Dir::Outgoing, true));
        assert_eq!(w.item_label(), "L21 Q20% Vaal Grace");
        // non-trade chat is ignored
        b.feed(&l("12:00:01", "@From Friend: hey"), "en");
        b.feed(&l("12:00:02", "#Global: hi"), "en");
        assert_eq!(b.list.len(), 1);
        for i in 0..60 {
            b.feed(&l("12:01:00", &format!("@From P{i}: Hi, I'd like to buy your 1 Chaos Orb for my 1 Divine Orb in Standard.")), "en");
        }
        assert_eq!(b.list.len(), CAP);
        assert_eq!(b.list[0].player, "P59");
    }
}
