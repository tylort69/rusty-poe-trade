//! Port of upstream `trends/PriceTrend.vue`, `price-prediction/*`,
//! `related-items/RelatedItems.vue`, `unidentified-resolver/*`,
//! `expected-value/*` and `BackgroundInfo.vue`.

use egui::{Align, Color32, Layout, RichText, Sense, Ui, vec2};
use rpt_data::BaseType;
use rpt_m3::{Button, TypeRole, icons, theme};
use rpt_parser::ParsedItem;
use rpt_trade::filters::ItemFilters;
use rpt_trade::poeprices::FeedbackOption;
use rpt_trade::trends::{Forecast, is_valuable_basetype, price_trend, show_trend_fallback};

use super::Env;
use super::session::PredictionState;
use super::ui::{QuickPrice, Slowdown, bundled, harmonized, quick_price};

fn base_cells(item: &ParsedItem) -> (u32, u32) {
    let b = item.unique_base.as_ref().unwrap_or(&item.info);
    (b.w.unwrap_or(1), b.h.unwrap_or(1))
}

fn centered(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        ui.horizontal(|ui| add(ui));
    });
}

// ---------------------------------------------------------------------------
// PriceTrend.vue

pub fn price_trend_view(ui: &mut Ui, env: &Env<'_>, item: &ParsedItem, filters: &ItemFilters, slowdown: &Slowdown) {
    let th = theme(ui.ctx());
    let c = th.colors;
    let trend = price_trend(item, &env.sv.prices);
    let valuable = is_valuable_basetype(item);
    match trend {
        Some(trend) => {
            ui.horizontal(|ui| {
                ui.set_min_height(48.0);
                if !valuable && !slowdown.is_ready(ui.ctx(), 800) {
                    ui.add_space((ui.available_width() / 2.0 - 120.0).max(0.0));
                    rpt_m3::circular_progress(ui, 20.0);
                    ui.label(env.i18n.tf("trade_result.getting_price", &[""]));
                    ui.label(RichText::new(env.t("trade_result.getting_price_from")).color(c.outline));
                    return;
                }
                let qp = QuickPrice {
                    item_img: Some(&item.info.icon),
                    item_cells: base_cells(item),
                    item_label: valuable.then(|| env.t("trade_result.base_item")),
                    fraction: filters.stack_size.is_some(),
                    ..Default::default()
                };
                let trend_w = if trend.change.is_some() { 140.0 } else { 0.0 };
                ui.allocate_ui_with_layout(vec2((ui.available_width() - trend_w).max(100.0), 40.0), Layout::top_down(Align::Center), |ui| {
                    ui.horizontal(|ui| quick_price(ui, Some(&trend.price), &qp));
                });
                if let Some(ch) = &trend.change {
                    let resp = egui::Frame::new()
                        .corner_radius(egui::CornerRadius::same(rpt_m3::shape::MEDIUM))
                        .inner_margin(egui::Margin::symmetric(8, 4))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 2.0;
                                        match ch.forecast {
                                            Forecast::Down => {
                                                ui.label(rpt_m3::icon_text(icons::KEYBOARD_DOUBLE_ARROW_DOWN, 16.0).color(c.error));
                                            }
                                            Forecast::Up => {
                                                ui.label(rpt_m3::icon_text(icons::KEYBOARD_DOUBLE_ARROW_UP, 16.0).color(c.success));
                                            }
                                            Forecast::Const => {
                                                ui.label(RichText::new("±").color(c.outline));
                                            }
                                        }
                                        ui.label(RichText::new(&ch.text).font(th.font(TypeRole::LabelLarge)));
                                    });
                                    ui.label(RichText::new(env.t("trade_result.graph_7d")).font(th.font(TypeRole::LabelSmall)).color(c.on_surface_variant));
                                });
                                rpt_m3::sparkline(
                                    ui,
                                    &ch.graph.points,
                                    ch.graph.draw_min,
                                    ch.graph.draw_max,
                                    vec2(48.0, 32.0),
                                    c.on_surface_variant,
                                    rpt_m3::with_alpha(c.secondary_container, 0.9),
                                );
                            });
                        })
                        .response
                        .interact(Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(resp.rect, egui::CornerRadius::same(rpt_m3::shape::MEDIUM), rpt_m3::with_alpha(c.on_surface, 0.08));
                    }
                    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("poe.ninja").clicked() {
                        env.open_url(trend.url.clone());
                    }
                }
            });
            ui.add_space(8.0);
        }
        None if show_trend_fallback(item) => {
            ui.horizontal(|ui| {
                ui.set_min_height(48.0);
                centered(ui, |ui| {
                    quick_price(ui, None, &QuickPrice { item_img: Some(&item.info.icon), item_cells: base_cells(item), currency_text: true, ..Default::default() });
                });
            });
            ui.add_space(8.0);
        }
        None => {}
    }
}

// ---------------------------------------------------------------------------
// PricePrediction.vue + FeedbackOption.vue

pub fn price_prediction(ui: &mut Ui, env: &Env<'_>, item: &ParsedItem, st: &mut PredictionState, slowdown: &Slowdown, league: &str) {
    let th = theme(ui.ctx());
    let c = th.colors;
    if st.loading() || !slowdown.is_ready(ui.ctx(), 800) {
        ui.horizontal(|ui| {
            ui.set_min_height(64.0);
            ui.add_space((ui.available_width() / 2.0 - 110.0).max(0.0));
            rpt_m3::circular_progress(ui, 20.0);
            ui.vertical(|ui| {
                ui.label("Getting price prediction...");
                ui.label(RichText::new("Powered by poeprices.info").color(c.outline));
            });
        });
        return;
    }
    if let Some(price) = st.price.clone() {
        ui.horizontal(|ui| {
            let cv = rpt_trade::prices::CurrencyValue { min: price.min, max: price.max, currency: price.currency };
            ui.allocate_ui_with_layout(vec2((ui.available_width() - 90.0).max(100.0), 40.0), Layout::top_down(Align::Center), |ui| {
                ui.horizontal(|ui| {
                    quick_price(ui, Some(&cv), &QuickPrice { item_img: Some(&item.info.icon), item_cells: base_cells(item), approx: true, ..Default::default() });
                });
            });
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.horizontal(|ui| {
                    if price.confidence < 78.0 {
                        ui.label(rpt_m3::icon_text(icons::WARNING, 16.0).color(harmonized(ui, Color32::from_rgb(0xF6, 0xAD, 0x55)).accent));
                    }
                    ui.label(RichText::new(format!("{}\u{2009}%", rpt_trade::js::num_to_string(price.confidence))).font(th.font(TypeRole::LabelLarge)));
                });
                ui.label(RichText::new("Confidence").font(th.font(TypeRole::LabelSmall)).color(c.on_surface_variant));
            });
        });
        ui.add_space(8.0);
        if !st.show_contrib {
            ui.horizontal(|ui| {
                if ui.add(Button::tonal("Contribution to predicted price").icon(icons::EXPAND_MORE).small()).clicked() {
                    st.show_contrib = true;
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let link = rpt_trade::poeprices::get_external_link(&item.raw_text, league);
                    if !st.feedback_sent && price.confidence < 83.0 {
                        for opt in [FeedbackOption::High, FeedbackOption::Fair, FeedbackOption::Low] {
                            if feedback_option(ui, env, item, st, &price, opt, league) {
                                st.feedback_sent = true;
                            }
                        }
                        if ui.add(rpt_m3::IconButton::new(icons::OPEN_IN_NEW).size(32.0).tooltip("poeprices.info")).clicked() {
                            env.open_url(link);
                        }
                    } else if ui.add(Button::text("by poeprices.info").small()).clicked() {
                        env.open_url(link);
                    }
                });
            });
        } else {
            if ui.add(Button::text("Contribution to predicted price").icon(icons::EXPAND_LESS).small()).clicked() {
                st.show_contrib = false;
            }
            egui::Grid::new(ui.scope_id().with("contrib")).spacing(vec2(8.0, 2.0)).show(ui, |ui| {
                for e in &price.explanation {
                    ui.label(RichText::new(format!("{}\u{a0}%", rpt_trade::js::num_to_string(e.contrib))).color(c.on_surface_variant));
                    ui.add(egui::Label::new(&e.name).truncate());
                    ui.end_row();
                }
            });
        }
        ui.add_space(8.0);
    } else if let Some(err) = &st.error {
        rpt_m3::banner(ui, icons::ERROR, format!("Failed to get price prediction\nError: {err}"), true);
        ui.add_space(8.0);
    }
}

/// One feedback button with its popover form. Returns true when sent.
fn feedback_option(
    ui: &mut Ui,
    env: &Env<'_>,
    item: &ParsedItem,
    st: &mut PredictionState,
    prediction: &rpt_trade::poeprices::RareItemPrice,
    opt: FeedbackOption,
    league: &str,
) -> bool {
    let (label, text) = match opt {
        FeedbackOption::Low => ("low", "Predicted price is too low."),
        FeedbackOption::High => ("high", "Predicted price is too high."),
        FeedbackOption::Fair => ("fair", "Predicted price is fair."),
    };
    let resp = ui.add(Button::outlined(label).small());
    let mut sent = false;
    rpt_m3::popover(&resp, |ui| {
        ui.set_width(256.0);
        ui.label(text);
        if opt != FeedbackOption::Fair {
            ui.add(egui::TextEdit::multiline(&mut st.feedback_text).hint_text("Why do you think so? (Not required)").desired_rows(5).desired_width(f32::INFINITY));
        }
        ui.horizontal(|ui| {
            if ui.add(Button::filled("Send feedback").small()).clicked() {
                let (trade, txt, pred, raw, league) =
                    (env.sv.trade.clone(), st.feedback_text.clone(), prediction.clone(), item.raw_text.clone(), league.to_owned());
                env.sv.rt.spawn(async move {
                    if let Err(e) = trade.send_poeprices_feedback(&txt, opt, &pred, &raw, &league).await {
                        tracing::warn!("poeprices feedback failed: {e}");
                    }
                });
                sent = true;
                ui.close();
            }
            ui.label("to poeprices.info");
        });
    });
    sent
}

// ---------------------------------------------------------------------------
// RelatedItems.vue

pub fn related_items(ui: &mut Ui, env: &Env<'_>, item: &ParsedItem) {
    let th = theme(ui.ctx());
    let c = th.colors;
    let res = rpt_trade::extras::related_items(item, &env.sv.prices);
    let res = match res {
        Ok(Some(r)) => Ok(r),
        Ok(None) => return,
        Err(e) => Err(e),
    };
    egui::Frame::new()
        .fill(c.surface_container)
        .corner_radius(egui::CornerRadius::same(rpt_m3::shape::LARGE))
        .inner_margin(egui::Margin::same(8))
        .shadow(egui::Shadow { offset: [0, 2], blur: 10, spread: 0, color: rpt_m3::with_alpha(c.shadow, 0.35) })
        .show(ui, |ui| {
            ui.set_max_width(384.0);
            match res {
                Err(e) => {
                    ui.label(e);
                }
                Ok(r) => {
                    ui.horizontal_top(|ui| {
                        let col = |ui: &mut Ui, list: &[rpt_trade::extras::RelatedItem]| {
                            ui.vertical(|ui| {
                                ui.set_width(180.0);
                                for it in list {
                                    let fill = if it.highlight { c.secondary_container } else { Color32::TRANSPARENT };
                                    egui::Frame::new().fill(fill).corner_radius(egui::CornerRadius::same(rpt_m3::shape::SMALL)).inner_margin(egui::Margin::symmetric(4, 2)).show(ui, |ui| {
                                        quick_price(ui, it.price.as_ref(), &QuickPrice { item_img: Some(&it.icon), currency_text: true, fraction: true, ..Default::default() });
                                        ui.add(egui::Label::new(RichText::new(&it.name).font(th.font(TypeRole::BodySmall)).color(c.on_surface_variant)).truncate());
                                    });
                                }
                            });
                        };
                        col(ui, &r.related);
                        if !r.items.is_empty() {
                            col(ui, &r.items);
                        }
                    });
                }
            }
        });
    let _ = env;
}

// ---------------------------------------------------------------------------
// UnidentifiedResolver.vue

/// Returns the identified item when the user picked (or there is one) variant.
pub fn unidentified_resolver(ui: &mut Ui, env: &Env<'_>, item: &ParsedItem) -> Option<ParsedItem> {
    if !rpt_trade::extras::needs_unidentified_resolver(item) {
        return None;
    }
    let variants: Vec<BaseType> = rpt_trade::extras::identified_variants(item);
    if variants.len() == 1 {
        return Some(rpt_trade::extras::identify_unique(&variants[0], item));
    }
    let c = theme(ui.ctx()).colors;
    let mut picked = None;
    rpt_m3::banner(ui, icons::HELP_OUTLINE, env.i18n.tf("item.identification", &[&item.info.name]), false);
    ui.add_space(8.0);
    egui::Grid::new(ui.scope_id().with("unid")).num_columns(2).spacing(vec2(8.0, 8.0)).show(ui, |ui| {
        for (i, v) in variants.iter().enumerate() {
            let resp = egui::Frame::new()
                .fill(c.surface_container_highest)
                .corner_radius(egui::CornerRadius::same(rpt_m3::shape::MEDIUM))
                .inner_margin(egui::Margin::same(8))
                .show(ui, |ui| {
                    ui.set_width(180.0);
                    ui.horizontal(|ui| {
                        super::ui::icon(ui, &v.icon, vec2(48.0, 48.0));
                        ui.add(egui::Label::new(&v.name).wrap());
                    });
                })
                .response
                .interact(Sense::click());
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                picked = Some(rpt_trade::extras::identify_unique(v, item));
            }
            if i % 2 == 1 {
                ui.end_row();
            }
        }
    });
    picked
}

// ---------------------------------------------------------------------------
// StackValue.vue / DustValue.vue

fn dashed_card(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    let c = theme(ui.ctx()).colors;
    ui.add_space(8.0);
    egui::Frame::new()
        .stroke(egui::Stroke::new(1.0, c.outline_variant))
        .corner_radius(egui::CornerRadius::same(rpt_m3::shape::SMALL))
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        });
}

pub fn stack_value(ui: &mut Ui, env: &Env<'_>, item: &ParsedItem, filters: &ItemFilters) {
    let Some(v) = rpt_trade::extras::stack_value(item, filters, &env.sv.prices) else { return };
    let c = theme(ui.ctx()).colors;
    dashed_card(ui, |ui| {
        ui.columns(2, |cols| {
            let line = |ui: &mut Ui, label: String, amount: f64, price: &str| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.label(RichText::new(format!("{label} × {}", rpt_trade::js::num_to_string(amount))).color(c.on_surface_variant));
                    ui.label(rpt_m3::icon_text(icons::ARROW_FORWARD, 14.0).color(c.outline));
                    ui.label(price);
                });
            };
            line(&mut cols[0], env.t("trade_result.you_have"), v.have.amount, &v.have.price);
            if let Some(s) = &v.one_stack {
                line(&mut cols[1], env.t("trade_result.stack"), s.amount, &s.price);
            }
        });
    });
}

pub fn dust_value(ui: &mut Ui, env: &Env<'_>, item: &ParsedItem) {
    let Some(d) = item.dust_equivalent else { return };
    let c = theme(ui.ctx()).colors;
    dashed_card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(RichText::new(env.t("item.disenchanting")).color(c.on_surface_variant));
            ui.label(rpt_m3::icon_text(icons::ARROW_FORWARD, 14.0).color(c.outline));
            ui.label(format!("{} ×", locale_number(d)));
            ui.add(egui::Image::new(bundled("dust.png")).fit_to_exact_size(vec2(20.0, 20.0)));
        });
    });
}

/// `Number.toLocaleString()` (en): thousands separators.
pub fn locale_number(v: f64) -> String {
    let neg = v < 0.0;
    let r = v.abs().round() as u64;
    let s = r.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    if neg { format!("-{out}") } else { out }
}

// ---------------------------------------------------------------------------
// BackgroundInfo.vue

pub fn background_info(ui: &mut Ui, env: &Env<'_>) {
    let c = theme(ui.ctx()).colors;
    if env.sv.leagues.is_loading() {
        ui.horizontal(|ui| {
            ui.label(rpt_m3::icon_text(icons::INFO, 18.0).color(c.outline));
            ui.label(env.t("app.leagues_loading"));
        });
        ui.add_space(8.0);
    } else if env.sv.leagues.error().is_some() {
        rpt_m3::banner(ui, icons::ERROR, format!("{}\n{}", env.t("app.leagues_failed"), env.t("app.leagues_failed_help")), true);
        ui.horizontal(|ui| {
            if ui.add(Button::tonal(env.t("Retry")).icon(icons::REFRESH).small()).clicked() {
                env.sv.reload_leagues();
            }
            if ui.add(Button::text(env.t("Browser")).icon(icons::OPEN_IN_NEW).small()).clicked() {
                env.open_url(format!("https://{}/api/leagues?type=main&realm=pc", env.sv.trade.trade_endpoint()));
            }
        });
        ui.add_space(8.0);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn locale() {
        assert_eq!(super::locale_number(1234567.0), "1,234,567");
        assert_eq!(super::locale_number(950.0), "950");
        assert_eq!(super::locale_number(1000.0), "1,000");
    }
}
