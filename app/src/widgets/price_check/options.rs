//! Price check options, drawn both by the cog in the price check window and by
//! Settings → Price check. Every option says in plain words what it does.

use egui::{RichText, Ui};
use rpt_m3::{CardKind, Chip, TypeRole, icons, theme};

use crate::config::{PriceCheckWidget, ShowSeller};
use crate::i18n::I18n;

/// What happens when the window opens with a hotkey.
#[derive(Clone, Copy, PartialEq)]
enum AutoSearch {
    Off,
    Easy,
    Always,
}

fn auto_get(on: bool, always: bool) -> AutoSearch {
    match (on, always) {
        (false, _) => AutoSearch::Off,
        (true, false) => AutoSearch::Easy,
        (true, true) => AutoSearch::Always,
    }
}

fn auto_set(v: AutoSearch, on: &mut bool, always: &mut bool) {
    *on = v != AutoSearch::Off;
    *always = v == AutoSearch::Always;
}

/// Seller choices as three independent picks. The trade site has only four
/// seller modes, so this maps the picks onto the nearest one.
pub struct SellerPicks<'a> {
    /// "Instant buyout only" on the trade site.
    pub merchant_only: &'a mut bool,
    /// "In person (online)" only.
    pub in_person_only: &'a mut bool,
    /// Offline sellers too ("Any").
    pub offline: &'a mut bool,
}

/// Draw the seller pickers: sale type (instant buyout / in person, either or
/// both) and, for in-person sellers, online only or offline too. Returns true
/// if something changed.
pub fn sellers(ui: &mut Ui, p: SellerPicks<'_>) -> bool {
    let th = theme(ui.ctx());
    let c = th.colors;
    let mut instant = !*p.in_person_only;
    let mut in_person = !*p.merchant_only;
    let mut changed = false;

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        // never allow both off: clicking the only selected one does nothing
        if ui.add(Chip::filter("Instant buyout", instant)).on_hover_text("Merchant tab listings: travel to their hideout and buy, no whisper needed.").clicked() && in_person {
            instant = !instant;
            changed = true;
        }
        if ui.add(Chip::filter("In person", in_person)).on_hover_text("Whisper the seller and trade face to face. You can negotiate.").clicked() && instant {
            in_person = !in_person;
            changed = true;
        }
    });
    if changed {
        *p.merchant_only = instant && !in_person;
        *p.in_person_only = in_person && !instant;
        if !in_person {
            *p.offline = false;
        }
    }

    if in_person {
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
            ui.label(RichText::new("In-person sellers:").color(c.on_surface_variant));
            let mut off = *p.offline;
            if choice(ui, &mut off, &[(false, "Online only"), (true, "Online and offline")]) {
                *p.offline = off;
                changed = true;
            }
        });
        if *p.offline && !instant {
            ui.add(egui::Label::new(
                RichText::new("The trade site can't leave out instant buyout once offline sellers are included, so those listings show too.")
                    .font(th.font(TypeRole::BodySmall))
                    .color(c.on_surface_variant),
            ).wrap());
        }
    }
    changed
}

/// Short summary for the filter button, e.g. "In person, online".
pub fn sellers_label(merchant_only: bool, in_person_only: bool, offline: bool) -> &'static str {
    match (offline, merchant_only, in_person_only) {
        (true, _, _) => "Any seller, offline too",
        (false, true, _) => "Instant buyout",
        (false, false, true) => "In person, online",
        (false, false, false) => "Instant buyout + in person",
    }
}

const LISTED: [(Option<&str>, &str); 7] = [
    (None, "Any time"),
    (Some("1day"), "1 day"),
    (Some("3days"), "3 days"),
    (Some("1week"), "1 week"),
    (Some("2weeks"), "2 weeks"),
    (Some("1month"), "1 month"),
    (Some("2months"), "2 months"),
];

/// Asking-price currencies (trade site `price.option` ids).
pub const CURRENCY: [(Option<&str>, &str); 14] = [
    (None, "Any currency"),
    (Some("chaos_divine"), "Chaos or Divine"),
    (Some("chaos"), "Chaos"),
    (Some("divine"), "Divine"),
    (Some("exalted"), "Exalted"),
    (Some("alt"), "Alteration"),
    (Some("alch"), "Alchemy"),
    (Some("fusing"), "Fusing"),
    (Some("chrome"), "Chromatic"),
    (Some("jew"), "Jeweller's"),
    (Some("chance"), "Chance"),
    (Some("scour"), "Scouring"),
    (Some("vaal"), "Vaal"),
    (Some("mirror"), "Mirror"),
];

const RANGE_PRESETS: [f64; 5] = [0.0, 5.0, 10.0, 15.0, 20.0];

/// A titled block: headline, explanation, then its control(s) underneath.
fn block(ui: &mut Ui, title: &str, help: &str, body: impl FnOnce(&mut Ui)) {
    let th = theme(ui.ctx());
    let c = th.colors;
    ui.add_space(6.0);
    ui.label(RichText::new(title).font(th.font(TypeRole::BodyLarge)).color(c.on_surface));
    if !help.is_empty() {
        ui.add(egui::Label::new(RichText::new(help).font(th.font(TypeRole::BodySmall)).color(c.on_surface_variant)).wrap());
    }
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        body(ui);
    });
    ui.add_space(6.0);
}

/// Single-select row of filter chips. Returns true if the value changed.
fn choice<T: PartialEq + Clone>(ui: &mut Ui, cur: &mut T, opts: &[(T, &str)]) -> bool {
    let mut changed = false;
    for (v, label) in opts {
        let sel = *cur == *v;
        if ui.add(Chip::filter(*label, sel)).clicked() && !sel {
            *cur = v.clone();
            changed = true;
        }
    }
    changed
}

fn toggle(ui: &mut Ui, title: &str, help: &str, value: &mut bool) {
    rpt_m3::setting_row(ui, None, title, (!help.is_empty()).then_some(help), |ui| rpt_m3::switch(ui, value));
}

fn section(ui: &mut Ui, title: &str, body: impl FnOnce(&mut Ui)) {
    ui.add_space(8.0);
    rpt_m3::card(ui, CardKind::Outlined).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let th = theme(ui.ctx());
        ui.label(RichText::new(title).font(th.font(TypeRole::TitleSmall)).color(th.colors.primary));
        body(ui);
    });
}

/// Draw every price check option. Returns true when something changed.
pub fn options(ui: &mut Ui, pc: &mut PriceCheckWidget, i18n: &I18n) -> bool {
    let before = pc.clone();

    section(ui, "When the window opens", |ui| {
        let easy_help = "\"Easy items\" are ones the default filters price well: uniques, currency and other bulk items, \
                         heist contracts and blueprints, sanctum relics, charms, idols. For anything else you pick mods and press Search.";
        if let Some(k) = pc.hotkey.clone() {
            let mut v = auto_get(pc.smart_initial_search, pc.smart_initial_always);
            let title = format!("Search automatically with {} + {} (hold)", pc.hotkey_hold, k);
            block(ui, &title, easy_help, |ui| {
                if choice(ui, &mut v, &[(AutoSearch::Off, "Never"), (AutoSearch::Easy, "Easy items only"), (AutoSearch::Always, "Every item")]) {
                    auto_set(v, &mut pc.smart_initial_search, &mut pc.smart_initial_always);
                }
            });
        }
        if let Some(k) = pc.hotkey_locked.clone() {
            let mut v = auto_get(pc.locked_initial_search, pc.locked_initial_always);
            let title = format!("Search automatically with {k} (stays open)");
            let help = if pc.hotkey.is_some() { "" } else { easy_help };
            block(ui, &title, help, |ui| {
                if choice(ui, &mut v, &[(AutoSearch::Off, "Never"), (AutoSearch::Easy, "Easy items only"), (AutoSearch::Always, "Every item")]) {
                    auto_set(v, &mut pc.locked_initial_search, &mut pc.locked_initial_always);
                }
            });
        }
        toggle(ui, "Mark the item you checked", "Draws a circle over the item in your inventory or stash.", &mut pc.show_cursor);
    });

    section(ui, "Mod filters", |ui| {
        block(
            ui,
            "Roll range",
            "How far the pre-filled mod values may be from your item's rolls. Rares: percent of the value. \
             Uniques: percent of the mod's possible range. Exact searches for your rolls or better.",
            |ui| {
                for p in RANGE_PRESETS {
                    let sel = (pc.search_stat_range - p).abs() < f64::EPSILON;
                    let label = if p == 0.0 { "Exact".to_owned() } else { format!("±{p}%") };
                    if ui.add(Chip::filter(label, sel)).clicked() {
                        pc.search_stat_range = p;
                    }
                }
                ui.label("Custom");
                rpt_m3::number_field(ui, &mut pc.search_stat_range, 0.0..=50.0, 0.2, 0, " %");
            },
        );
        block(
            ui,
            "Fill in",
            "Which ends of each mod's range get a value. \"Minimum only\" finds items at least as good as yours; \
             \"Minimum and maximum\" finds items rolled close to yours.",
            |ui| {
                choice(ui, &mut pc.fill_roll_max, &[(false, "Minimum only"), (true, "Minimum and maximum")]);
            },
        );
        toggle(
            ui,
            "Search stacks at least as big as yours",
            "For stackable items (currency, fragments, …). Off searches single items.",
            &mut pc.activate_stock_filter,
        );
    });

    section(ui, "Listings", |ui| {
        block(
            ui,
            "Sellers",
            "Pick one or both sale types. Instant buyout only: buy right away. In person, online only: sellers you can whisper and haggle with now. \
             Both, online and offline: everything, to see whether the item exists at all.",
            |ui| {
                ui.vertical(|ui| {
                    sellers(ui, SellerPicks { merchant_only: &mut pc.merchant_only, in_person_only: &mut pc.in_person_only, offline: &mut pc.default_offline });
                });
            },
        );
        block(ui, "Listed within", "Shorter windows give fresher prices but fewer results.", |ui| {
            let mut cur = pc.default_listed.clone();
            let opts: Vec<(Option<String>, &str)> = LISTED.iter().map(|(v, l)| (v.map(str::to_owned), *l)).collect();
            if choice(ui, &mut cur, &opts) {
                pc.default_listed = cur;
            }
        });
        block(
            ui,
            "Asking price in",
            "Only show listings priced in this currency. Any currency compares everything by its chaos value; picking one (e.g. Alteration) shows only sellers asking for it.",
            |ui| {
                let mut cur = pc.default_currency.clone();
                let opts: Vec<(Option<String>, &str)> = CURRENCY.iter().map(|(v, l)| (v.map(str::to_owned), *l)).collect();
                if choice(ui, &mut cur, &opts) {
                    pc.default_currency = cur;
                }
            },
        );
        block(
            ui,
            "Group a seller's repeat listings",
            "By the trade site is fast. By this app is exact, but uses more of the trade site's rate limit.",
            |ui| {
                choice(ui, &mut pc.collapse_listings, &[("api".to_owned(), "By the trade site"), ("app".to_owned(), "By this app")]);
            },
        );
    });

    section(ui, "Results", |ui| {
        let mut seller = match &pc.show_seller {
            ShowSeller::Mode(m) => m.clone(),
            ShowSeller::Off(_) => String::new(),
        };
        block(ui, "Seller column", "What to show for each listing's seller. Your own listings are always highlighted.", |ui| {
            if choice(ui, &mut seller, &[(String::new(), "Hide"), ("account".into(), "Account name"), ("ign".into(), "Character name")]) {
                pc.show_seller = if seller.is_empty() { ShowSeller::Off(false) } else { ShowSeller::Mode(seller.clone()) };
            }
        });
        toggle(ui, "Show the rate-limit meter", "How much of the trade site's request allowance is left.", &mut pc.show_rate_limit_state);
        toggle(
            ui,
            "Price prediction for rares",
            "Asks poeprices.info for an estimate. English game client and popular leagues only.",
            &mut pc.request_price_prediction,
        );
    });

    section(ui, "Advanced", |ui| {
        rpt_m3::setting_row(
            ui,
            None,
            "Extra wait between searches",
            Some("Added to each trade request. Raise it if you get rate-limit errors."),
            |ui| rpt_m3::number_field(ui, &mut pc.api_latency_seconds, 0.5..=10.0, 0.05, 1, " s"),
        );
        toggle(ui, "Open trade links in the built-in browser", "Instead of your system browser.", &mut pc.builtin_browser);
        if pc.builtin_browser {
            rpt_m3::banner(ui, icons::WARNING, i18n.t("price_check.builtin_browser_warning"), true);
        }
    });

    *pc != before
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seller_labels_match_site_modes() {
        assert_eq!(sellers_label(true, false, false), "Instant buyout");
        assert_eq!(sellers_label(false, true, false), "In person, online");
        assert_eq!(sellers_label(false, false, false), "Instant buyout + in person");
        assert_eq!(sellers_label(false, true, true), "Any seller, offline too");
    }

    #[test]
    fn auto_search_roundtrip() {
        for v in [AutoSearch::Off, AutoSearch::Easy, AutoSearch::Always] {
            let (mut on, mut always) = (true, true);
            auto_set(v, &mut on, &mut always);
            assert!(auto_get(on, always) == v);
        }
    }
}
