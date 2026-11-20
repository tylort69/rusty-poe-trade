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

/// Sellers to include by default.
#[derive(Clone, Copy, PartialEq)]
enum Sellers {
    InstantBuyout,
    Online,
    Offline,
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

const CURRENCY: [(Option<&str>, &str); 4] = [
    (None, "Any currency"),
    (Some("chaos_divine"), "Chaos or Divine"),
    (Some("chaos"), "Chaos only"),
    (Some("divine"), "Divine only"),
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
        let mut sellers = if pc.default_offline {
            Sellers::Offline
        } else if pc.merchant_only {
            Sellers::InstantBuyout
        } else {
            Sellers::Online
        };
        block(
            ui,
            "Sellers",
            "Instant buyout: merchant tab listings you can buy by travelling to the hideout. \
             Online: also players who sell in person (whisper them). Include offline: every listing, mostly stale prices.",
            |ui| {
                if choice(
                    ui,
                    &mut sellers,
                    &[(Sellers::InstantBuyout, "Instant buyout only"), (Sellers::Online, "Online"), (Sellers::Offline, "Include offline")],
                ) {
                    pc.merchant_only = sellers == Sellers::InstantBuyout;
                    pc.default_offline = sellers == Sellers::Offline;
                }
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
            "Which currencies sellers must ask for. Chaos or Divine skips listings priced in other currencies, which are often mispriced.",
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
    fn auto_search_roundtrip() {
        for v in [AutoSearch::Off, AutoSearch::Easy, AutoSearch::Always] {
            let (mut on, mut always) = (true, true);
            auto_set(v, &mut on, &mut always);
            assert!(auto_get(on, always) == v);
        }
    }
}
