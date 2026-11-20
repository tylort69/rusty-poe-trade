//! Drawing of the PoB viewer: source sidebar and the five tabs.

use std::path::{Path, PathBuf};

use egui::{Align, Color32, Layout, RichText, ScrollArea, Ui, text::LayoutJob, vec2};
use rpt_m3::{Button, CardKind, Chip, IconButton, TypeRole, icons, theme};

use super::items::{self, ModKind};
use super::model::{self, Build};
use super::paths;
use super::tree;
use super::{PobState, Source, Tab, widget_cfg};
use crate::host::HostCommand;
use crate::widgets::WCtx;
use crate::widgets::price_check::ui::harmonized;

/// Extra Material Icons Round codepoints (not in `rpt_m3::icons`).
const ICON_ACCOUNT_TREE: &str = "\u{e97a}";
const ICON_CLOUD_DOWNLOAD: &str = "\u{e2c0}";
const ICON_DESCRIPTION: &str = "\u{e873}";

const SIDEBAR_W: f32 = 260.0;
const CARD_W: f32 = 228.0;

pub(super) fn body(ui: &mut Ui, w: &mut WCtx<'_>, st: &mut PobState, wm_id: u32, size: egui::Vec2) {
    let th = theme(ui.ctx());
    let c = th.colors;
    let show_sidebar = widget_cfg(w, wm_id).map(|p| p.sidebar).unwrap_or(true);

    // Title row.
    ui.horizontal(|ui| {
        ui.label(rpt_m3::icon_text(ICON_ACCOUNT_TREE, 22.0).color(c.primary));
        ui.label(RichText::new("Path of Building").font(th.font(TypeRole::TitleLarge)).color(c.on_surface));
        if let Some(l) = &st.loaded {
            ui.label(RichText::new(l.build.title()).font(th.font(TypeRole::TitleSmall)).color(c.on_surface_variant));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .add(IconButton::new(icons::FOLDER_OPEN).selected(show_sidebar).size(32.0).tooltip(if show_sidebar { "Hide sources" } else { "Show sources" }))
                .clicked()
            {
                if let Some(p) = widget_cfg(w, wm_id) {
                    p.sidebar = !p.sidebar;
                    w.save_requested = true;
                }
            }
        });
    });
    ui.add_space(4.0);

    let h = (size.y - 56.0).max(200.0);
    ui.horizontal_top(|ui| {
        if show_sidebar {
            ui.allocate_ui_with_layout(vec2(SIDEBAR_W, h), Layout::top_down(Align::Min), |ui| {
                ui.set_width(SIDEBAR_W);
                ui.set_min_height(h);
                sidebar(ui, w, st, wm_id);
            });
            ui.separator();
        }
        let main_w = ui.available_width();
        ui.allocate_ui_with_layout(vec2(main_w, h), Layout::top_down(Align::Min), |ui| {
            ui.set_width(main_w);
            ui.set_min_height(h);
            main_panel(ui, w, st, wm_id, h);
        });
    });
}

// ---------------------------------------------------------------------------
// Sidebar

fn short_label(p: &Path) -> String {
    let s = p.to_string_lossy();
    if s.contains(super::paths::FLATPAK_ID) {
        "Flatpak".into()
    } else if let Some(i) = s.find("compatdata/") {
        let id: String = s[i + 11..].chars().take_while(|c| *c != '/').collect();
        if s.contains("com.valvesoftware.Steam") { format!("Steam Flatpak {id}") } else { format!("Proton {id}") }
    } else if s.contains(".wine") {
        "Wine".into()
    } else if s.contains("pobfrontend") {
        "pobfrontend".into()
    } else if s.contains("OneDrive") {
        "OneDrive".into()
    } else {
        "Documents".into()
    }
}

fn ago(t: std::time::SystemTime) -> String {
    let s = std::time::SystemTime::now().duration_since(t).map(|d| d.as_secs()).unwrap_or(0);
    match s {
        0..60 => "now".into(),
        60..3600 => format!("{}m", s / 60),
        3600..86400 => format!("{}h", s / 3600),
        86400..2_592_000 => format!("{}d", s / 86400),
        _ => format!("{}mo", s / 2_592_000),
    }
}

fn sidebar(ui: &mut Ui, w: &mut WCtx<'_>, st: &mut PobState, wm_id: u32) {
    let th = theme(ui.ctx());
    let c = th.colors;
    rpt_m3::segmented(ui, &mut st.source, &[(Source::Folder, "Folder"), (Source::Import, "Import")]);
    ui.add_space(8.0);
    match st.source {
        Source::Folder => folder_source(ui, w, st, wm_id),
        Source::Import => {
            rpt_m3::section_header(ui, "Build code or link");
            let resp = rpt_m3::text_field(ui, &mut st.import_input, "Paste code, pobb.in or pastebin link");
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.add_space(8.0);
            let busy = st.fetch.is_some();
            ui.horizontal(|ui| {
                let label = if model::import_url(&st.import_input).is_some() { "Fetch & open" } else { "Open" };
                if ui.add(Button::filled(label).icon(ICON_CLOUD_DOWNLOAD).small().enabled(!busy && !st.import_input.trim().is_empty())).clicked() || (enter && !busy) {
                    st.start_import(w);
                }
                if busy {
                    rpt_m3::circular_progress(ui, 20.0);
                }
            });
            ui.add_space(4.0);
            ui.label(
                RichText::new("Links are downloaded from pobb.in or pastebin.com only when you press the button.")
                    .font(th.font(TypeRole::BodySmall))
                    .color(c.on_surface_variant),
            );
        }
    }
    if let Some(e) = &st.error {
        ui.add_space(8.0);
        rpt_m3::banner(ui, icons::ERROR, e.clone(), true);
    }
}

fn folder_source(ui: &mut Ui, w: &mut WCtx<'_>, st: &mut PobState, wm_id: u32) {
    let th = theme(ui.ctx());
    let c = th.colors;
    let candidates = st.candidates.get_or_insert_with(|| paths::detect(&paths::Env::current())).clone();
    let configured = widget_cfg(w, wm_id).and_then(|p| p.builds_dir.clone());
    let current: Option<PathBuf> = configured.clone().map(PathBuf::from).or_else(|| candidates.first().cloned());
    let dir_input = st.dir_input.get_or_insert_with(|| current.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default());

    rpt_m3::section_header(ui, "Builds folder");
    let mut set_dir: Option<Option<String>> = None;
    if !candidates.is_empty() {
        ui.horizontal_wrapped(|ui| {
            for cand in &candidates {
                let sel = current.as_ref() == Some(cand);
                let r = ui.add(Chip::filter(short_label(cand), sel)).on_hover_text(cand.to_string_lossy());
                if r.clicked() {
                    set_dir = Some(Some(cand.to_string_lossy().into_owned()));
                }
            }
        });
        ui.add_space(4.0);
    }
    let resp = rpt_m3::text_field(ui, dir_input, "…/Path of Building/Builds");
    if resp.lost_focus() {
        let t = dir_input.trim().to_owned();
        if Some(&t) != configured.as_ref() && current.as_ref().map(|p| p.to_string_lossy().into_owned()) != Some(t.clone()) {
            set_dir = Some((!t.is_empty()).then_some(t));
        }
    }
    ui.add_space(4.0);
    let mut refresh = false;
    ui.horizontal(|ui| {
        if ui.add(Button::text("Rescan").icon(icons::REFRESH).small()).clicked() {
            st.candidates = None;
            refresh = true;
        }
        if configured.is_some() && ui.add(Button::text("Auto-detect").small()).clicked() {
            set_dir = Some(None);
        }
    });
    if let Some(d) = set_dir {
        st.dir_input = Some(d.clone().unwrap_or_default());
        if let Some(p) = widget_cfg(w, wm_id) {
            p.builds_dir = d;
            w.save_requested = true;
        }
        st.listing = None;
        return;
    }
    if refresh {
        st.listing = None;
    }

    let Some(dir) = current else {
        ui.add_space(8.0);
        ui.label(
            RichText::new("No Path of Building folder found. Type the path of your Builds folder above.")
                .font(th.font(TypeRole::BodySmall))
                .color(c.on_surface_variant),
        );
        return;
    };
    if st.listing.as_ref().map(|(d, _)| d != &dir).unwrap_or(true) {
        st.listing = Some((dir.clone(), paths::list_builds(&dir)));
    }
    let last = widget_cfg(w, wm_id).and_then(|p| p.last_build.clone());
    let files = st.listing.as_ref().map(|(_, f)| f.clone()).unwrap_or_default();
    ui.add_space(8.0);
    if !dir.is_dir() {
        rpt_m3::banner(ui, icons::WARNING, "Folder does not exist", true);
        return;
    }
    if files.is_empty() {
        ui.label(RichText::new("No .xml builds in this folder.").color(c.on_surface_variant));
        return;
    }
    let mut open: Option<PathBuf> = None;
    ScrollArea::vertical().id_salt("pob-builds").auto_shrink([false, false]).show(ui, |ui| {
        let mut group: Option<&str> = None;
        for f in &files {
            if group != Some(f.group.as_str()) {
                group = Some(f.group.as_str());
                if !f.group.is_empty() {
                    ui.add_space(4.0);
                    ui.label(rpt_m3::icon_text(icons::FOLDER_OPEN, 14.0).color(c.primary));
                    ui.label(RichText::new(&f.group).font(th.font(TypeRole::LabelMedium)).color(c.primary));
                }
            }
            let sel = last.as_deref() == Some(&*f.path.to_string_lossy());
            ui.horizontal(|ui| {
                let r = ui.add_sized(
                    vec2(ui.available_width() - 36.0, 24.0),
                    egui::Button::selectable(sel, RichText::new(&f.name).color(if sel { c.primary } else { c.on_surface })),
                );
                if r.clicked() {
                    open = Some(f.path.clone());
                }
                ui.label(RichText::new(ago(f.modified)).font(th.font(TypeRole::LabelSmall)).color(c.on_surface_variant));
            });
        }
    });
    if let Some(p) = open {
        if st.load_file(&p) {
            if let Some(cfg) = widget_cfg(w, wm_id) {
                cfg.last_build = Some(p.to_string_lossy().into_owned());
                w.save_requested = true;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Main panel

fn main_panel(ui: &mut Ui, w: &mut WCtx<'_>, st: &mut PobState, wm_id: u32, h: f32) {
    let th = theme(ui.ctx());
    let c = th.colors;
    if st.loaded.is_none() {
        ui.add_space(h * 0.3);
        ui.vertical_centered(|ui| {
            ui.label(rpt_m3::icon_text(ICON_DESCRIPTION, 48.0).color(c.on_surface_variant));
            ui.label(RichText::new("No build loaded").font(th.font(TypeRole::TitleMedium)).color(c.on_surface));
            ui.label(
                RichText::new("Pick a build from your PoB folder or paste an export code.").color(c.on_surface_variant),
            );
        });
        return;
    }
    rpt_m3::segmented(
        ui,
        &mut st.tab,
        &[(Tab::Overview, "Overview"), (Tab::Skills, "Skills"), (Tab::Items, "Items"), (Tab::Tree, "Tree"), (Tab::Notes, "Notes")],
    );
    ui.add_space(8.0);
    ScrollArea::vertical().id_salt(("pob-tab", st.tab as u8)).auto_shrink([false, false]).show(ui, |ui| match st.tab {
        Tab::Overview => overview(ui, st),
        Tab::Skills => skills(ui, st),
        Tab::Items => items_tab(ui, st),
        Tab::Tree => tree_tab(ui, w, st),
        Tab::Notes => notes(ui, w, st, wm_id),
    });
}

fn card_title(ui: &mut Ui, s: &str) {
    let th = theme(ui.ctx());
    ui.label(RichText::new(s).font(th.font(TypeRole::TitleSmall)).color(th.colors.primary));
    ui.add_space(4.0);
}

fn stat_row(ui: &mut Ui, label: &str, value: String, color: Option<Color32>) {
    let th = theme(ui.ctx());
    let c = th.colors;
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).color(c.on_surface_variant));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(value).font(th.font(TypeRole::LabelLarge)).color(color.unwrap_or(c.on_surface)));
        });
    });
}

/// A fixed-width filled card in a wrapping row.
fn stat_card(ui: &mut Ui, title: &str, body: impl FnOnce(&mut Ui)) {
    ui.allocate_ui(vec2(CARD_W + 24.0, 0.0), |ui| {
        rpt_m3::card(ui, CardKind::Filled).show(ui, |ui| {
            ui.set_width(CARD_W);
            card_title(ui, title);
            body(ui);
        });
    });
}

fn wrap_row(ui: &mut Ui, body: impl FnOnce(&mut Ui)) {
    ui.with_layout(Layout::left_to_right(Align::TOP).with_main_wrap(true), |ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        body(ui);
    });
}

// --- Overview ---------------------------------------------------------------

const OFFENCE: &[(&str, &str)] = &[
    ("CombinedDPS", "Combined DPS"),
    ("TotalDPS", "Hit DPS"),
    ("TotalDotDPS", "DoT DPS"),
    ("BleedDPS", "Bleed DPS"),
    ("IgniteDPS", "Ignite DPS"),
    ("PoisonDPS", "Poison DPS"),
    ("AverageHit", "Average hit"),
    ("Speed", "Attack/cast rate"),
    ("HitChance", "Hit chance"),
    ("CritChance", "Crit chance"),
    ("CritMultiplier", "Crit multiplier"),
];
const DEFENCE: &[(&str, &str)] = &[
    ("Life", "Life"),
    ("LifeUnreserved", "Unreserved life"),
    ("EnergyShield", "Energy shield"),
    ("Mana", "Mana"),
    ("ManaUnreserved", "Unreserved mana"),
    ("Ward", "Ward"),
    ("Armour", "Armour"),
    ("Evasion", "Evasion"),
    ("PhysicalDamageReduction", "Phys. reduction"),
    ("BlockChance", "Block"),
    ("SpellBlockChance", "Spell block"),
    ("EffectiveSpellSuppressionChance", "Spell suppression"),
    ("SpellSuppressionChance", "Spell suppression"),
    ("TotalEHP", "Effective HP"),
    ("LifeRegenRecovery", "Life recovery/s"),
];
const RESISTS: &[(&str, &str, [u8; 3])] = &[
    ("FireResist", "Fire", [0xE0, 0x50, 0x30]),
    ("ColdResist", "Cold", [0x50, 0x80, 0xE0]),
    ("LightningResist", "Lightning", [0xE0, 0xC0, 0x30]),
    ("ChaosResist", "Chaos", [0xC0, 0x50, 0xC0]),
];

fn overview(ui: &mut Ui, st: &PobState) {
    let Some(l) = &st.loaded else { return };
    let b = &l.build;
    let th = theme(ui.ctx());
    let c = th.colors;
    rpt_m3::card(ui, CardKind::Elevated).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(b.title()).font(th.font(TypeRole::HeadlineSmall)).color(c.on_surface));
        ui.horizontal_wrapped(|ui| {
            if !b.class_name.is_empty() {
                ui.add(Chip::assist(format!("Class: {}", b.class_name)));
            }
            if let Some(band) = &b.bandit {
                ui.add(Chip::assist(format!("Bandit: {band}")));
            }
            if let Some(g) = main_group(b) {
                ui.add(Chip::assist(format!("Main skill: {}", g.title())).leading(icons::STAR));
            }
            if let Some(v) = b.spec().and_then(|s| s.tree_version.clone()) {
                ui.add(Chip::assist(format!("Tree {}", v.replace('_', "."))));
            }
        });
        ui.label(RichText::new(&l.origin).font(th.font(TypeRole::BodySmall)).color(c.on_surface_variant));
    });
    ui.add_space(8.0);
    wrap_row(ui, |ui| {
        let off: Vec<_> = OFFENCE.iter().filter_map(|(k, n)| b.stat(k).map(|v| (*k, *n, v))).filter(|(k, _, v)| !(k.ends_with("DPS") && *v == 0.0)).collect();
        let minion_dps = b.minion_stats.iter().find(|(k, _)| k == "TotalDPS").map(|(_, v)| *v).filter(|v| *v > 0.0);
        if !off.is_empty() || minion_dps.is_some() {
            stat_card(ui, "Offence", |ui| {
                for (k, n, v) in off {
                    stat_row(ui, n, model::fmt_stat(k, v), (k == "CombinedDPS").then_some(c.primary));
                }
                if let Some(v) = minion_dps {
                    stat_row(ui, "Minion DPS", model::fmt_big(v), None);
                }
            });
        }
        let mut seen_supp = false;
        let def: Vec<_> = DEFENCE
            .iter()
            .filter_map(|(k, n)| b.stat(k).map(|v| (*k, *n, v)))
            .filter(|(k, _, _)| {
                if k.contains("SpellSuppression") {
                    !std::mem::replace(&mut seen_supp, true)
                } else {
                    true
                }
            })
            .collect();
        if !def.is_empty() {
            stat_card(ui, "Defence", |ui| {
                for (k, n, v) in def {
                    let v = if matches!(k, "BlockChance" | "SpellBlockChance" | "PhysicalDamageReduction") || k.contains("Suppression") {
                        format!("{}%", model::fmt_big(v))
                    } else {
                        model::fmt_stat(k, v)
                    };
                    stat_row(ui, n, v, None);
                }
            });
        }
        let res: Vec<_> = RESISTS.iter().filter_map(|(k, n, col)| b.stat(k).map(|v| (*k, *n, *col, v))).collect();
        if !res.is_empty() {
            stat_card(ui, "Resistances", |ui| {
                for (k, n, col, v) in res {
                    let over = b.stat(&format!("{k}OverCap")).filter(|o| *o > 0.0);
                    let txt = match over {
                        Some(o) => format!("{}%  (+{} over cap)", model::fmt_big(v), model::fmt_big(o)),
                        None => format!("{}%", model::fmt_big(v)),
                    };
                    let tint = harmonized(ui, Color32::from_rgb(col[0], col[1], col[2])).accent;
                    let warn = (k != "ChaosResist" && v < 75.0).then_some(c.error);
                    stat_row(ui, n, txt, Some(warn.unwrap_or(tint)));
                }
            });
        }
        let attrs: Vec<_> = [("Str", "Strength"), ("Dex", "Dexterity"), ("Int", "Intelligence")].iter().filter_map(|(k, n)| b.stat(k).map(|v| (*n, v))).collect();
        if !attrs.is_empty() {
            stat_card(ui, "Attributes", |ui| {
                for (n, v) in attrs {
                    stat_row(ui, n, model::fmt_big(v), None);
                }
            });
        }
    });
    if b.stats.is_empty() {
        ui.label(RichText::new("This build has no saved stats (open and save it in Path of Building to compute them).").color(c.on_surface_variant));
    }
    if let Some(cs) = b.config_sets.get(b.active_config_set).filter(|c| !c.inputs.is_empty()) {
        ui.add_space(8.0);
        egui::CollapsingHeader::new(format!("Configuration{}", if cs.title.is_empty() { String::new() } else { format!(" — {}", cs.title) }))
            .id_salt("pob-config")
            .show(ui, |ui| {
                egui::Grid::new("pob-config-grid").num_columns(2).striped(true).show(ui, |ui| {
                    for (k, v) in &cs.inputs {
                        ui.label(RichText::new(k).color(c.on_surface_variant));
                        ui.label(v);
                        ui.end_row();
                    }
                });
            });
    }
}

fn main_group(b: &Build) -> Option<&model::SkillGroup> {
    let set = b.skill_set()?;
    set.groups.get(b.main_socket_group?.checked_sub(1)?)
}

// --- Skills -----------------------------------------------------------------

fn alt_quality(q: &str) -> &str {
    match q {
        "Alternate1" => "Anomalous",
        "Alternate2" => "Divergent",
        "Alternate3" => "Phantasmal",
        q => q,
    }
}

fn skills(ui: &mut Ui, st: &mut PobState) {
    let Some(l) = &mut st.loaded else { return };
    let th = theme(ui.ctx());
    let c = th.colors;
    let b = &mut l.build;
    if b.skill_sets.len() > 1 {
        ui.horizontal_wrapped(|ui| {
            for (i, s) in b.skill_sets.iter().enumerate() {
                let title = if s.title.is_empty() { format!("Set {}", i + 1) } else { s.title.clone() };
                if ui.add(Chip::filter(title, i == b.active_skill_set)).clicked() {
                    b.active_skill_set = i;
                }
            }
        });
        ui.add_space(8.0);
    }
    let main_idx = b.main_socket_group.and_then(|m| m.checked_sub(1));
    let Some(set) = b.skill_set() else {
        ui.label(RichText::new("No skills in this build.").color(c.on_surface_variant));
        return;
    };
    if set.groups.is_empty() {
        ui.label(RichText::new("No socket groups in this skill set.").color(c.on_surface_variant));
    }
    for (i, g) in set.groups.iter().enumerate() {
        let is_main = Some(i) == main_idx;
        let frame = if is_main {
            rpt_m3::card(ui, CardKind::Filled).fill(c.secondary_container).stroke(egui::Stroke::new(1.5, c.primary))
        } else {
            rpt_m3::card(ui, CardKind::Outlined)
        };
        frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            let fg = if g.enabled { c.on_surface } else { rpt_m3::with_alpha(c.on_surface, 0.45) };
            ui.horizontal_wrapped(|ui| {
                if is_main {
                    ui.label(rpt_m3::icon_text(icons::STAR, 18.0).color(c.primary));
                }
                ui.label(RichText::new(g.title()).font(th.font(TypeRole::TitleSmall)).color(fg));
                if let Some(slot) = &g.slot {
                    rpt_m3::tonal_badge(ui, slot, c.surface_container_highest, c.on_surface_variant);
                }
                if g.source.is_some() {
                    rpt_m3::tonal_badge(ui, "from item", c.tertiary_container, c.on_tertiary_container);
                }
                if g.include_in_full_dps {
                    rpt_m3::tonal_badge(ui, "Full DPS", c.primary_container, c.on_primary_container);
                }
                if !g.enabled {
                    rpt_m3::tonal_badge(ui, "Disabled", c.error_container, c.on_error_container);
                }
            });
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                let mut active_n = 0;
                for gem in &g.gems {
                    let lvl_q = match (gem.level, gem.quality) {
                        (Some(l), Some(q)) if q > 0 => format!("{l}/{q}"),
                        (Some(l), _) => format!("{l}"),
                        (None, Some(q)) if q > 0 => format!("q{q}"),
                        _ => String::new(),
                    };
                    let mut label = gem.name.clone();
                    if let Some(q) = &gem.quality_id {
                        label = format!("{} {label}", alt_quality(q));
                    }
                    if !lvl_q.is_empty() {
                        label = format!("{label}  {lvl_q}");
                    }
                    let enabled = gem.enabled && g.enabled;
                    let (fill, fg, stroke) = if gem.support {
                        (Color32::TRANSPARENT, c.on_surface_variant, egui::Stroke::new(1.0, c.outline_variant))
                    } else {
                        active_n += 1;
                        if is_main && g.main_active_skill == Some(active_n) {
                            (c.primary, c.on_primary, egui::Stroke::NONE)
                        } else {
                            (c.primary_container, c.on_primary_container, egui::Stroke::NONE)
                        }
                    };
                    let tip = format!(
                        "{}{}{}",
                        if gem.support { "Support gem" } else { "Active skill" },
                        if gem.enabled { "" } else { " (disabled)" },
                        gem.skill_id.as_deref().map(|s| format!("\n{s}")).unwrap_or_default()
                    );
                    gem_chip(ui, &label, fill, fg, stroke, enabled).on_hover_text(tip);
                }
            });
        });
        ui.add_space(6.0);
    }
}

/// Chip-shaped gem label: filled for active skills, outlined for supports, dimmed when disabled.
fn gem_chip(ui: &mut Ui, label: &str, fill: Color32, fg: Color32, stroke: egui::Stroke, enabled: bool) -> egui::Response {
    let th = theme(ui.ctx());
    let a = if enabled { 1.0 } else { 0.38 };
    egui::Frame::new()
        .fill(rpt_m3::with_alpha(fill, if fill == Color32::TRANSPARENT { 0.0 } else { a }))
        .stroke(egui::Stroke::new(stroke.width, rpt_m3::with_alpha(stroke.color, a)))
        .corner_radius(egui::CornerRadius::same(rpt_m3::shape::SMALL))
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            let mut t = RichText::new(label).font(th.font(TypeRole::LabelLarge)).color(rpt_m3::with_alpha(fg, a));
            if !enabled {
                t = t.strikethrough();
            }
            ui.label(t);
        })
        .response
}

// --- Items ------------------------------------------------------------------

const SLOT_ORDER: &[&str] = &[
    "Weapon 1", "Weapon 2", "Helmet", "Body Armour", "Gloves", "Boots", "Amulet", "Ring 1", "Ring 2", "Belt", "Flask 1", "Flask 2", "Flask 3",
    "Flask 4", "Flask 5",
];

/// (slot label, item id) in display order: main slots, other slots, tree jewels.
fn ordered_slots(b: &Build) -> Vec<(String, u32)> {
    let Some(set) = b.item_set() else { return vec![] };
    let mut out: Vec<(String, u32)> = SLOT_ORDER.iter().filter_map(|s| set.slots.iter().find(|(n, _)| n == s).cloned()).collect();
    for s in &set.slots {
        if !SLOT_ORDER.contains(&s.0.as_str()) {
            out.push(s.clone());
        }
    }
    if let Some(spec) = b.spec() {
        for (node, id) in &spec.sockets {
            out.push((format!("Jewel socket {node}"), *id));
        }
    }
    out
}

fn mod_color(ui: &Ui, m: &items::ModLine) -> Color32 {
    let c = theme(ui.ctx()).colors;
    if m.crafted {
        harmonized(ui, Color32::from_rgb(0xB4, 0xB4, 0xFF)).accent
    } else if m.fractured {
        harmonized(ui, Color32::from_rgb(0xA2, 0x91, 0x62)).accent
    } else if m.kind == ModKind::Enchant {
        harmonized(ui, Color32::from_rgb(0xB8, 0xDA, 0xF2)).accent
    } else {
        c.on_surface
    }
}

fn items_tab(ui: &mut Ui, st: &mut PobState) {
    let Some(l) = &mut st.loaded else { return };
    let th = theme(ui.ctx());
    let c = th.colors;
    if l.build.item_sets.len() > 1 {
        ui.horizontal_wrapped(|ui| {
            for (i, s) in l.build.item_sets.iter().enumerate() {
                let title = if s.title.is_empty() { format!("Set {}", i + 1) } else { s.title.clone() };
                if ui.add(Chip::filter(title, i == l.build.active_item_set)).clicked() {
                    l.build.active_item_set = i;
                }
            }
        });
        ui.add_space(6.0);
    }

    // Compare bar.
    rpt_m3::card(ui, CardKind::Outlined).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            ui.label(rpt_m3::icon_text(icons::COMPARE_ARROWS, 20.0).color(c.primary));
            match &st.last_checked {
                None => {
                    ui.label(RichText::new("Compare with an item: price-check or item-check an item in game first.").color(c.on_surface_variant));
                    ui.add(Button::tonal("Compare").small().enabled(false));
                }
                Some(lc) => {
                    let tint = harmonized(ui, lc.item.rarity.design_color()).accent;
                    ui.label(RichText::new("Last checked:").color(c.on_surface_variant));
                    ui.label(RichText::new(lc.item.title()).color(tint).strong());
                    if lc.slots.is_empty() {
                        ui.label(RichText::new("(no matching equipment slot)").color(c.on_surface_variant));
                    }
                    for s in &lc.slots {
                        let sel = st.compare_slot.as_deref() == Some(s.as_str());
                        if ui.add(Button::tonal(format!("Compare with {s}")).small().selected(sel)).clicked() {
                            st.compare_slot = if sel { None } else { Some(s.clone()) };
                        }
                    }
                }
            }
        });
    });
    ui.add_space(8.0);

    let slots = ordered_slots(&l.build);
    if slots.is_empty() {
        ui.label(RichText::new("No equipped items in this item set.").color(c.on_surface_variant));
    }
    wrap_row(ui, |ui| {
        for (slot, id) in &slots {
            let Some(it) = l.items.get(id) else { continue };
            let sel = st.selected_slot.as_deref() == Some(slot.as_str());
            let tint = harmonized(ui, it.rarity.design_color()).accent;
            let frame = if sel {
                rpt_m3::card(ui, CardKind::Filled).fill(c.secondary_container).inner_margin(egui::Margin::same(8))
            } else {
                rpt_m3::card(ui, CardKind::Filled).inner_margin(egui::Margin::same(8))
            };
            let r = ui
                .allocate_ui(vec2(196.0, 0.0), |ui| {
                    frame
                        .show(ui, |ui| {
                            ui.set_width(180.0);
                            ui.label(RichText::new(slot).font(th.font(TypeRole::LabelSmall)).color(c.on_surface_variant));
                            ui.add(egui::Label::new(RichText::new(if it.name.is_empty() { "?" } else { &it.name }).color(tint).strong()).truncate());
                            if let Some(base) = &it.base {
                                ui.add(egui::Label::new(RichText::new(base).font(th.font(TypeRole::BodySmall)).color(c.on_surface_variant)).truncate());
                            }
                        })
                        .response
                })
                .inner;
            let r = ui.interact(r.rect, ui.scope_id().with(("pob-slot", slot)), egui::Sense::click());
            if r.clicked() {
                st.selected_slot = if sel { None } else { Some(slot.clone()) };
                st.compare_slot = None;
            }
        }
    });
    ui.add_space(8.0);

    if let Some(slot) = st.compare_slot.clone() {
        let pob = slots.iter().find(|(s, _)| *s == slot).and_then(|(_, id)| l.items.get(id));
        if let Some(lc) = &st.last_checked {
            compare_view(ui, &lc.item, pob, &slot);
        }
    } else if let Some(slot) = &st.selected_slot {
        if let Some((_, id)) = slots.iter().find(|(s, _)| s == slot) {
            if let (Some(it), Some(raw)) = (l.items.get(id), l.build.item(*id)) {
                item_detail(ui, slot, it, &raw.text);
            }
        }
    }
}

fn item_detail(ui: &mut Ui, slot: &str, it: &items::ItemText, raw: &str) {
    let th = theme(ui.ctx());
    let c = th.colors;
    rpt_m3::card(ui, CardKind::Elevated).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let tint = harmonized(ui, it.rarity.design_color()).accent;
        ui.label(RichText::new(slot).font(th.font(TypeRole::LabelMedium)).color(c.on_surface_variant));
        ui.label(RichText::new(&it.name).font(th.font(TypeRole::TitleMedium)).color(tint));
        if let Some(b) = &it.base {
            ui.label(RichText::new(b).color(tint));
        }
        ui.add_space(4.0);
        let mut last_kind: Option<ModKind> = None;
        for m in &it.mods {
            if last_kind.is_some_and(|k| k != m.kind) {
                rpt_m3::divider(ui);
            }
            last_kind = Some(m.kind);
            ui.label(RichText::new(&m.text).color(mod_color(ui, m)));
        }
        if it.corrupted {
            ui.label(RichText::new("Corrupted").color(c.error));
        }
        ui.add_space(4.0);
        egui::CollapsingHeader::new("Full PoB item text").id_salt(("pob-raw", slot)).show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(raw).monospace().color(c.on_surface_variant)).wrap());
            if ui.add(Button::text("Copy").icon(icons::CONTENT_COPY).small()).clicked() {
                ui.ctx().copy_text(raw.to_owned());
            }
        });
    });
}

fn compare_view(ui: &mut Ui, checked: &items::ItemText, pob: Option<&items::ItemText>, slot: &str) {
    let th = theme(ui.ctx());
    let c = th.colors;
    rpt_m3::card(ui, CardKind::Elevated).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(format!("Checked item vs PoB {slot}")).font(th.font(TypeRole::TitleSmall)).color(c.on_surface));
            rpt_m3::tonal_badge(ui, "mod comparison, not a PoB stat calculation", c.tertiary_container, c.on_tertiary_container);
        });
        let Some(pob) = pob else {
            ui.label(RichText::new(format!("The build has no item in {slot}; every mod is new.")).color(c.on_surface_variant));
            for m in &checked.mods {
                ui.label(RichText::new(format!("+ {}", m.text)).color(c.success));
            }
            return;
        };
        let a_tint = harmonized(ui, checked.rarity.design_color()).accent;
        let b_tint = harmonized(ui, pob.rarity.design_color()).accent;
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("A:").color(c.on_surface_variant));
            ui.label(RichText::new(checked.title()).color(a_tint));
            ui.label(RichText::new("  B (PoB):").color(c.on_surface_variant));
            ui.label(RichText::new(pob.title()).color(b_tint));
        });
        ui.add_space(4.0);
        let rows = items::compare(&checked.mods, &pob.mods);
        let both: Vec<_> = rows.iter().filter(|r| r.a.is_some() && r.b.is_some()).collect();
        let only_a: Vec<_> = rows.iter().filter(|r| r.only_a()).collect();
        let only_b: Vec<_> = rows.iter().filter(|r| r.only_b()).collect();
        if !both.is_empty() {
            rpt_m3::section_header(ui, "In both");
            egui::Grid::new("pob-cmp-both").num_columns(3).spacing(vec2(16.0, 4.0)).striped(true).show(ui, |ui| {
                for r in both {
                    ui.label(r.a.as_deref().unwrap_or_default());
                    ui.label(RichText::new(r.b.as_deref().unwrap_or_default()).color(c.on_surface_variant));
                    match &r.deltas {
                        Some(d) if d.iter().any(|x| x.abs() > 1e-9) => {
                            let txt = d.iter().map(|x| format!("{}{}", if *x > 0.0 { "+" } else { "" }, items::fmt_num(*x))).collect::<Vec<_>>().join(" / ");
                            let col = if d.iter().all(|x| *x >= 0.0) { c.success } else if d.iter().all(|x| *x <= 0.0) { c.error } else { c.on_surface };
                            ui.label(RichText::new(txt).color(col).strong());
                        }
                        _ => {
                            ui.label(RichText::new("=").color(c.on_surface_variant));
                        }
                    }
                    ui.end_row();
                }
            });
        }
        if !only_a.is_empty() {
            rpt_m3::section_header(ui, "Only on the checked item (A)");
            for r in only_a {
                ui.label(RichText::new(format!("+ {}", r.a.as_deref().unwrap_or_default())).color(c.success));
            }
        }
        if !only_b.is_empty() {
            rpt_m3::section_header(ui, "Only on the PoB item (B)");
            for r in only_b {
                ui.label(RichText::new(format!("− {}", r.b.as_deref().unwrap_or_default())).color(c.error));
            }
        }
        ui.add_space(4.0);
        ui.label(
            RichText::new("Differences are A − B per value; lines with the same stat are summed. Whether higher is better depends on the mod.")
                .font(th.font(TypeRole::BodySmall))
                .color(c.on_surface_variant),
        );
    });
}

// --- Tree -------------------------------------------------------------------

fn tree_tab(ui: &mut Ui, w: &mut WCtx<'_>, st: &mut PobState) {
    let th = theme(ui.ctx());
    let c = th.colors;
    let Some(l) = &mut st.loaded else { return };
    if l.build.specs.len() > 1 {
        ui.horizontal_wrapped(|ui| {
            for (i, s) in l.build.specs.iter().enumerate() {
                let title = if s.title.is_empty() { format!("Tree {}", i + 1) } else { s.title.clone() };
                if ui.add(Chip::filter(title, i == l.build.active_spec)).clicked() {
                    l.build.active_spec = i;
                }
            }
        });
        ui.add_space(8.0);
    }
    let Some(spec) = l.build.spec().cloned() else {
        ui.label(RichText::new("No passive tree in this build.").color(c.on_surface_variant));
        return;
    };
    let ascend_name = l.build.ascend_class_name.clone();
    let tag = spec.tree_version.as_deref().and_then(tree::export_tag);
    if let Some(t) = &tag {
        st.ensure_tree_cache(w, t);
    }
    let data = tag.as_ref().and_then(|t| st.tree_data.get(t)).and_then(|d| d.as_ref());

    let class = spec
        .class_id
        .and_then(|ci| data.and_then(|d| d.classes.get(ci as usize)).map(|c| c.0.clone()).or_else(|| tree::class_fallback(ci).map(str::to_owned)));
    let asc = match (spec.class_id, spec.ascend_class_id) {
        (Some(ci), Some(ai)) if ai > 0 => data
            .and_then(|d| d.classes.get(ci as usize))
            .and_then(|c| c.1.get(ai as usize - 1).cloned())
            .or_else(|| tree::ascendancy_fallback(ci, ai).map(str::to_owned)),
        _ => None,
    }
    .or_else(|| (!ascend_name.is_empty() && ascend_name != "None").then_some(ascend_name));

    let (main_nodes, asc_nodes) = match data {
        Some(d) => {
            let a = spec.nodes.iter().filter(|n| d.nodes.get(n).is_some_and(|x| x.ascendancy.is_some())).count();
            (spec.nodes.len() - a, Some(a))
        }
        None => (spec.nodes.len(), None),
    };

    rpt_m3::card(ui, CardKind::Elevated).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(if spec.title.is_empty() { "Passive tree" } else { &spec.title }).font(th.font(TypeRole::TitleMedium)).color(c.on_surface));
        ui.horizontal_wrapped(|ui| {
            if let Some(cl) = &class {
                ui.add(Chip::assist(format!("Class: {cl}")));
            }
            if let Some(a) = &asc {
                ui.add(Chip::assist(format!("Ascendancy: {a}")));
            }
            ui.add(Chip::assist(match asc_nodes {
                Some(a) => format!("{main_nodes} passives + {a} ascendancy"),
                None => format!("{main_nodes} nodes allocated"),
            }));
            if !spec.mastery_effects.is_empty() {
                ui.add(Chip::assist(format!("{} masteries", spec.mastery_effects.len())));
            }
            if !spec.sockets.is_empty() {
                ui.add(Chip::assist(format!("{} jewels", spec.sockets.len())));
            }
            if let Some(v) = &spec.tree_version {
                ui.add(Chip::assist(format!("Version {}", v.replace('_', "."))));
            }
        });
        if let Some(url) = &spec.url {
            ui.horizontal(|ui| {
                if ui.add(Button::tonal("Open in browser").icon(icons::OPEN_IN_NEW).small()).clicked() {
                    w.host.push(HostCommand::OpenUrl(url.clone()));
                }
                if ui.add(Button::text("Copy link").icon(icons::LINK).small()).clicked() {
                    ui.ctx().copy_text(url.clone());
                }
            });
        }
    });
    ui.add_space(8.0);

    match (data, &tag) {
        (Some(d), _) => {
            let named = |kind: tree::NodeKind, asc: bool| -> Vec<String> {
                let mut v: Vec<String> = spec
                    .nodes
                    .iter()
                    .filter_map(|n| d.nodes.get(n))
                    .filter(|n| n.kind == kind && n.ascendancy.is_some() == asc && !n.name.is_empty())
                    .map(|n| n.name.clone())
                    .collect();
                v.sort();
                v.dedup();
                v
            };
            let keystones = named(tree::NodeKind::Keystone, false);
            let notables = named(tree::NodeKind::Notable, false);
            let asc_notables = named(tree::NodeKind::Notable, true);
            for (title, list) in [("Keystones", keystones), ("Ascendancy", asc_notables), ("Notables", notables)] {
                if list.is_empty() {
                    continue;
                }
                rpt_m3::section_header(ui, format!("{title} ({})", list.len()));
                ui.horizontal_wrapped(|ui| {
                    for n in list {
                        let mut chip = Chip::assist(n.clone());
                        if title == "Keystones" {
                            chip = chip.leading(icons::DIAMOND);
                        }
                        let stats = d.nodes.values().find(|x| x.name == n).map(|x| x.stats.join("\n")).unwrap_or_default();
                        let r = ui.add(chip);
                        if !stats.is_empty() {
                            r.on_hover_text(stats);
                        }
                    }
                });
            }
            ui.add_space(8.0);
            ui.label(RichText::new("Allocated nodes (scroll to zoom, drag to pan, double-click to reset)").font(th.font(TypeRole::BodySmall)).color(c.on_surface_variant));
            let size = vec2(ui.available_width(), 360.0);
            tree::draw(ui, d, &spec.nodes, &mut st.tree_view, size, c.primary, c.on_surface_variant);
        }
        (None, Some(t)) => {
            let busy = st.tree_task.is_some();
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Keystones, notables and the tree view need GGG's passive tree data.").color(c.on_surface_variant));
                if ui.add(Button::tonal(format!("Download {t} tree data (~5 MB)")).icon(ICON_CLOUD_DOWNLOAD).small().enabled(!busy)).clicked() {
                    let t = t.clone();
                    st.download_tree(w, &t);
                }
                if busy {
                    rpt_m3::circular_progress(ui, 20.0);
                }
            });
            if let Some(e) = &st.tree_error {
                rpt_m3::banner(ui, icons::ERROR, e.clone(), true);
            }
        }
        (None, None) => {
            ui.label(RichText::new("Tree data is not available for this tree version.").color(c.on_surface_variant));
        }
    }
}

// --- Notes ------------------------------------------------------------------

fn notes(ui: &mut Ui, w: &mut WCtx<'_>, st: &PobState, wm_id: u32) {
    let Some(l) = &st.loaded else { return };
    let th = theme(ui.ctx());
    let c = th.colors;
    let mut colored = widget_cfg(w, wm_id).map(|p| p.note_colors).unwrap_or(true);
    if rpt_m3::switch_row(ui, "Show PoB colors", &mut colored).changed() {
        if let Some(p) = widget_cfg(w, wm_id) {
            p.note_colors = colored;
            w.save_requested = true;
        }
    }
    ui.add_space(6.0);
    if l.build.notes.trim().is_empty() {
        ui.label(RichText::new("This build has no notes.").color(c.on_surface_variant));
        return;
    }
    rpt_m3::card(ui, CardKind::Filled).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let font = th.font(TypeRole::BodyMedium);
        if colored {
            let mut job = LayoutJob::default();
            for (col, text) in model::color_spans(&l.build.notes) {
                let color = match col {
                    Some([r, g, b]) => harmonized(ui, Color32::from_rgb(r, g, b)).accent,
                    None => c.on_surface,
                };
                job.append(&text, 0.0, egui::TextFormat { font_id: font.clone(), color, ..Default::default() });
            }
            job.wrap.max_width = ui.available_width();
            ui.label(job);
        } else {
            ui.label(RichText::new(model::strip_colors(&l.build.notes)).font(font).color(c.on_surface));
        }
    });
}
