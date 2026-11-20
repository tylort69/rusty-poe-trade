//! Material Design 3 components for egui.
//!
//! Every component reads colors from the installed [`crate::M3Theme`] and draws
//! M3 state layers (hover 8%, pressed 12%) with short emphasized animations.

use egui::{
    Align2, Color32, CornerRadius, FontId, Frame, Id, Margin, Pos2, Rect, Response, RichText, Sense, Shadow, Stroke,
    StrokeKind, Ui, Vec2, Widget, WidgetInfo, WidgetText, Role, emath::easing, vec2,
};

use crate::scheme::{ColorScheme, state_layer, with_alpha};
use crate::theme::{ICONS, TypeRole, shape, theme};

const ANIM: f32 = 0.15;

fn interaction_opacity(resp: &Response) -> f32 {
    if resp.is_pointer_button_down_on() {
        0.12
    } else if resp.hovered() || resp.has_focus() {
        0.08
    } else {
        0.0
    }
}

/// `RichText` that renders a Material icon.
pub fn icon_text(icon: &str, size: f32) -> RichText {
    RichText::new(icon).font(FontId::new(size, egui::FontFamily::Name(ICONS.into())))
}

/// Text in an M3 type role.
pub fn text(ui: &Ui, s: impl Into<String>, role: TypeRole) -> RichText {
    RichText::new(s).font(theme(ui.ctx()).font(role))
}

// ---------------------------------------------------------------------------
// Buttons

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Filled,
    Tonal,
    Elevated,
    Outlined,
    Text,
    /// Filled with `error` colors (destructive actions).
    Danger,
}

/// M3 common button (40dp tall, full-rounded, label-large text, optional leading icon).
pub struct Button<'a> {
    kind: ButtonKind,
    label: String,
    icon: Option<&'a str>,
    enabled: bool,
    small: bool,
    selected: Option<bool>,
}

impl<'a> Button<'a> {
    pub fn new(kind: ButtonKind, label: impl Into<String>) -> Self {
        Self { kind, label: label.into(), icon: None, enabled: true, small: false, selected: None }
    }
    pub fn filled(label: impl Into<String>) -> Self {
        Self::new(ButtonKind::Filled, label)
    }
    pub fn tonal(label: impl Into<String>) -> Self {
        Self::new(ButtonKind::Tonal, label)
    }
    pub fn outlined(label: impl Into<String>) -> Self {
        Self::new(ButtonKind::Outlined, label)
    }
    pub fn text(label: impl Into<String>) -> Self {
        Self::new(ButtonKind::Text, label)
    }
    pub fn elevated(label: impl Into<String>) -> Self {
        Self::new(ButtonKind::Elevated, label)
    }
    pub fn icon(mut self, icon: &'a str) -> Self {
        self.icon = Some(icon);
        self
    }
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
    /// 32dp compact variant used inside dense overlay widgets.
    pub fn small(mut self) -> Self {
        self.small = true;
        self
    }
    /// Toggle-button semantics (selected → filled, unselected → tonal/outlined).
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = Some(selected);
        self
    }

    fn colors(&self, c: &ColorScheme) -> (Color32, Color32, Stroke, bool) {
        let kind = match (self.selected, self.kind) {
            (Some(true), _) => ButtonKind::Filled,
            (Some(false), ButtonKind::Filled) => ButtonKind::Tonal,
            (_, k) => k,
        };
        match kind {
            ButtonKind::Filled => (c.primary, c.on_primary, Stroke::NONE, false),
            ButtonKind::Tonal => (c.secondary_container, c.on_secondary_container, Stroke::NONE, false),
            ButtonKind::Elevated => (c.surface_container_low, c.primary, Stroke::NONE, true),
            ButtonKind::Outlined => (Color32::TRANSPARENT, c.primary, Stroke::new(1.0, c.outline_variant), false),
            ButtonKind::Text => (Color32::TRANSPARENT, c.primary, Stroke::NONE, false),
            ButtonKind::Danger => (c.error, c.on_error, Stroke::NONE, false),
        }
    }
}

impl Widget for Button<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let th = theme(ui.ctx());
        let c = th.colors;
        let (mut bg, mut fg, stroke, elevated) = self.colors(&c);
        let height = if self.small { 32.0 } else { 40.0 };
        let font = th.font(TypeRole::LabelLarge);
        let icon_size = if self.small { 16.0 } else { 18.0 };

        let galley = ui.painter().layout_no_wrap(self.label.clone(), font, fg);
        let pad_x = if matches!(self.kind, ButtonKind::Text) { 12.0 } else if self.small { 12.0 } else { 24.0 };
        let icon_w = if self.icon.is_some() { icon_size + if self.label.is_empty() { 0.0 } else { 8.0 } } else { 0.0 };
        let lead_pad = if self.icon.is_some() && !self.label.is_empty() { pad_x - 8.0 } else { pad_x };
        let width = (lead_pad + icon_w + galley.size().x + pad_x).max(height);
        let (rect, resp) = ui.allocate_exact_size(vec2(width, height), if self.enabled { Sense::click() } else { Sense::hover() });
        resp.widget_info(|| WidgetInfo::labeled(Role::Button, self.enabled, &self.label));

        if ui.is_rect_visible(rect) {
            if !self.enabled {
                bg = if bg == Color32::TRANSPARENT { bg } else { with_alpha(c.on_surface, 0.12) };
                fg = with_alpha(c.on_surface, 0.38);
            }
            let radius = CornerRadius::same(shape::FULL);
            let p = ui.painter();
            if elevated && self.enabled {
                p.add(Shadow { offset: [0, 1], blur: 3, spread: 0, color: with_alpha(c.shadow, 0.3) }.as_shape(rect, radius));
            }
            p.rect(rect, radius, bg, stroke, StrokeKind::Inside);
            let o = ui.ctx().animate_value_with_time(resp.id, if self.enabled { interaction_opacity(&resp) } else { 0.0 }, ANIM);
            if o > 0.0 {
                p.rect_filled(rect, radius, with_alpha(fg, o));
            }
            let content_w = icon_w + galley.size().x;
            let mut x = rect.center().x - content_w / 2.0;
            if let Some(icon) = self.icon {
                p.text(Pos2::new(x, rect.center().y), Align2::LEFT_CENTER, icon, FontId::new(icon_size, egui::FontFamily::Name(ICONS.into())), fg);
                x += icon_w;
            }
            p.galley_with_override_text_color(Pos2::new(x, rect.center().y - galley.size().y / 2.0), galley, fg);
        }
        if self.enabled {
            resp.on_hover_cursor(egui::CursorIcon::PointingHand)
        } else {
            resp
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconButtonKind {
    Standard,
    Filled,
    Tonal,
    Outlined,
}

/// M3 icon button (40dp target, 24dp icon).
pub struct IconButton<'a> {
    icon: &'a str,
    kind: IconButtonKind,
    selected: Option<bool>,
    size: f32,
    tooltip: Option<String>,
    enabled: bool,
}

impl<'a> IconButton<'a> {
    pub fn new(icon: &'a str) -> Self {
        Self { icon, kind: IconButtonKind::Standard, selected: None, size: 40.0, tooltip: None, enabled: true }
    }
    pub fn kind(mut self, kind: IconButtonKind) -> Self {
        self.kind = kind;
        self
    }
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = Some(selected);
        self
    }
    /// Touch-target size (icon scales to 60%).
    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }
    pub fn tooltip(mut self, t: impl Into<String>) -> Self {
        self.tooltip = Some(t.into());
        self
    }
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

impl Widget for IconButton<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let c = theme(ui.ctx()).colors;
        let (rect, resp) = ui.allocate_exact_size(Vec2::splat(self.size), if self.enabled { Sense::click() } else { Sense::hover() });
        resp.widget_info(|| WidgetInfo::labeled(Role::Button, self.enabled, self.tooltip.as_deref().unwrap_or("")));
        let sel = self.selected.unwrap_or(false);
        let (bg, mut fg, stroke) = match self.kind {
            IconButtonKind::Standard => (Color32::TRANSPARENT, if sel { c.primary } else { c.on_surface_variant }, Stroke::NONE),
            IconButtonKind::Filled => {
                if self.selected == Some(false) {
                    (c.surface_container_highest, c.primary, Stroke::NONE)
                } else {
                    (c.primary, c.on_primary, Stroke::NONE)
                }
            }
            IconButtonKind::Tonal => {
                if sel { (c.secondary_container, c.on_secondary_container, Stroke::NONE) } else { (c.surface_container_highest, c.on_surface_variant, Stroke::NONE) }
            }
            IconButtonKind::Outlined => {
                if sel { (c.inverse_surface, c.inverse_on_surface, Stroke::NONE) } else { (Color32::TRANSPARENT, c.on_surface_variant, Stroke::new(1.0, c.outline)) }
            }
        };
        if !self.enabled {
            fg = with_alpha(c.on_surface, 0.38);
        }
        if ui.is_rect_visible(rect) {
            let r = CornerRadius::same(shape::FULL);
            let p = ui.painter();
            p.rect(rect, r, bg, stroke, StrokeKind::Inside);
            let o = ui.ctx().animate_value_with_time(resp.id, interaction_opacity(&resp), ANIM);
            if o > 0.0 && self.enabled {
                p.rect_filled(rect, r, with_alpha(fg, o));
            }
            p.text(rect.center(), Align2::CENTER_CENTER, self.icon, FontId::new(self.size * 0.6, egui::FontFamily::Name(ICONS.into())), fg);
        }
        match self.tooltip {
            Some(t) => resp.on_hover_text(t),
            None => resp,
        }
    }
}

/// M3 FAB (56dp, large radius, primary container).
pub fn fab(ui: &mut Ui, icon: &str, label: Option<&str>) -> Response {
    let th = theme(ui.ctx());
    let c = th.colors;
    let font = th.font(TypeRole::LabelLarge);
    let galley = label.map(|l| ui.painter().layout_no_wrap(l.to_owned(), font, c.on_primary_container));
    let w = galley.as_ref().map_or(56.0, |g| 16.0 + 24.0 + 12.0 + g.size().x + 20.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 56.0), Sense::click());
    let r = CornerRadius::same(shape::LARGE);
    let p = ui.painter();
    p.add(Shadow { offset: [0, 3], blur: 8, spread: 0, color: with_alpha(c.shadow, 0.3) }.as_shape(rect, r));
    p.rect_filled(rect, r, c.primary_container);
    let o = ui.ctx().animate_value_with_time(resp.id, interaction_opacity(&resp), ANIM);
    p.rect_filled(rect, r, with_alpha(c.on_primary_container, o));
    let icon_x = if galley.is_some() { rect.left() + 16.0 + 12.0 } else { rect.center().x };
    p.text(Pos2::new(icon_x, rect.center().y), Align2::CENTER_CENTER, icon, FontId::new(24.0, egui::FontFamily::Name(ICONS.into())), c.on_primary_container);
    if let Some(g) = galley {
        p.galley(Pos2::new(rect.left() + 52.0, rect.center().y - g.size().y / 2.0), g, c.on_primary_container);
    }
    resp
}

// ---------------------------------------------------------------------------
// Selection controls

/// M3 switch (52x32 track, thumb grows when on). Returns response; toggles `on` on click.
pub fn switch(ui: &mut Ui, on: &mut bool) -> Response {
    let c = theme(ui.ctx()).colors;
    let (rect, mut resp) = ui.allocate_exact_size(vec2(52.0, 32.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    resp.widget_info(|| WidgetInfo::selected(Role::Switch, true, *on, ""));
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_with_time_and_easing(resp.id, *on, ANIM, easing::cubic_out);
        let p = ui.painter();
        let r = CornerRadius::same(shape::FULL);
        let track = lerp_color(c.surface_container_highest, c.primary, t);
        let border = lerp_color(c.outline, c.primary, t);
        p.rect(rect, r, track, Stroke::new(2.0, border), StrokeKind::Inside);
        let pressed = resp.is_pointer_button_down_on();
        let radius = if pressed { 14.0 } else { egui::lerp(8.0..=12.0, t) };
        let cx = egui::lerp((rect.left() + 16.0)..=(rect.right() - 16.0), t);
        let center = Pos2::new(cx, rect.center().y);
        let o = interaction_opacity(&resp);
        if o > 0.0 {
            p.circle_filled(center, 20.0, with_alpha(if *on { c.primary } else { c.on_surface }, o));
        }
        p.circle_filled(center, radius, lerp_color(c.outline, c.on_primary, t));
        if t > 0.5 {
            p.text(center, Align2::CENTER_CENTER, crate::icons::CHECK, FontId::new(16.0, egui::FontFamily::Name(ICONS.into())), with_alpha(c.on_primary_container, (t - 0.5) * 2.0));
        }
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Switch with a leading label in a list-item row.
pub fn switch_row(ui: &mut Ui, label: impl Into<WidgetText>, on: &mut bool) -> Response {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| switch(ui, on)).inner
    })
    .inner
}

/// M3 checkbox (18dp box, 2dp radius).
pub fn checkbox(ui: &mut Ui, checked: &mut bool, label: impl Into<WidgetText>) -> Response {
    let c = theme(ui.ctx()).colors;
    let label: WidgetText = label.into();
    let galley = label.into_galley(ui, None, ui.available_width() - 40.0, egui::TextStyle::Body);
    let size = vec2(40.0 + galley.size().x, 40f32.max(galley.size().y));
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *checked = !*checked;
        resp.mark_changed();
    }
    resp.widget_info(|| WidgetInfo::selected(Role::CheckBox, true, *checked, galley.text()));
    let bx = Rect::from_center_size(Pos2::new(rect.left() + 20.0, rect.center().y), Vec2::splat(18.0));
    let p = ui.painter();
    let o = interaction_opacity(&resp);
    if o > 0.0 {
        p.circle_filled(bx.center(), 20.0, with_alpha(if *checked { c.primary } else { c.on_surface }, o));
    }
    if *checked {
        p.rect_filled(bx, CornerRadius::same(2), c.primary);
        p.text(bx.center(), Align2::CENTER_CENTER, crate::icons::CHECK, FontId::new(16.0, egui::FontFamily::Name(ICONS.into())), c.on_primary);
    } else {
        p.rect_stroke(bx, CornerRadius::same(2), Stroke::new(2.0, c.on_surface_variant), StrokeKind::Inside);
    }
    p.galley(Pos2::new(rect.left() + 40.0, rect.center().y - galley.size().y / 2.0), galley, c.on_surface);
    resp
}

/// M3 radio button.
pub fn radio<T: PartialEq + Clone>(ui: &mut Ui, current: &mut T, value: T, label: impl Into<WidgetText>) -> Response {
    let c = theme(ui.ctx()).colors;
    let label: WidgetText = label.into();
    let galley = label.into_galley(ui, None, ui.available_width() - 40.0, egui::TextStyle::Body);
    let (rect, mut resp) = ui.allocate_exact_size(vec2(40.0 + galley.size().x, 40.0), Sense::click());
    let selected = *current == value;
    if resp.clicked() && !selected {
        *current = value;
        resp.mark_changed();
    }
    resp.widget_info(|| WidgetInfo::selected(Role::RadioButton, true, selected, galley.text()));
    let center = Pos2::new(rect.left() + 20.0, rect.center().y);
    let p = ui.painter();
    let o = interaction_opacity(&resp);
    if o > 0.0 {
        p.circle_filled(center, 20.0, with_alpha(c.on_surface, o));
    }
    let col = if selected { c.primary } else { c.on_surface_variant };
    p.circle_stroke(center, 9.0, Stroke::new(2.0, col));
    if selected {
        p.circle_filled(center, 5.0, col);
    }
    p.galley(Pos2::new(rect.left() + 40.0, rect.center().y - galley.size().y / 2.0), galley, c.on_surface);
    resp
}

// ---------------------------------------------------------------------------
// Chips

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChipKind {
    Assist,
    Filter,
    Input,
    Suggestion,
}

/// M3 chip (32dp, small radius). Filter chips show a check when selected.
pub struct Chip<'a> {
    kind: ChipKind,
    label: WidgetText,
    selected: bool,
    leading: Option<&'a str>,
    trailing: Option<&'a str>,
    enabled: bool,
    color: Option<(Color32, Color32)>,
}

impl<'a> Chip<'a> {
    pub fn new(kind: ChipKind, label: impl Into<WidgetText>) -> Self {
        Self { kind, label: label.into(), selected: false, leading: None, trailing: None, enabled: true, color: None }
    }
    pub fn filter(label: impl Into<WidgetText>, selected: bool) -> Self {
        Self { selected, ..Self::new(ChipKind::Filter, label) }
    }
    pub fn assist(label: impl Into<WidgetText>) -> Self {
        Self::new(ChipKind::Assist, label)
    }
    pub fn input(label: impl Into<WidgetText>) -> Self {
        Self::new(ChipKind::Input, label)
    }
    pub fn leading(mut self, icon: &'a str) -> Self {
        self.leading = Some(icon);
        self
    }
    pub fn trailing(mut self, icon: &'a str) -> Self {
        self.trailing = Some(icon);
        self
    }
    pub fn enabled(mut self, e: bool) -> Self {
        self.enabled = e;
        self
    }
    /// Override (container, content) colors when selected — e.g. influence or rarity tints.
    pub fn colors(mut self, container: Color32, content: Color32) -> Self {
        self.color = Some((container, content));
        self
    }
}

impl Widget for Chip<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let th = theme(ui.ctx());
        let c = th.colors;
        let selected = self.selected && matches!(self.kind, ChipKind::Filter | ChipKind::Input);
        let (bg, fg, stroke) = if selected {
            let (bg, fg) = self.color.unwrap_or((c.secondary_container, c.on_secondary_container));
            (bg, fg, Stroke::NONE)
        } else {
            (Color32::TRANSPARENT, if matches!(self.kind, ChipKind::Assist) { c.on_surface } else { c.on_surface_variant }, Stroke::new(1.0, c.outline_variant))
        };
        let fg = if self.enabled { fg } else { with_alpha(c.on_surface, 0.38) };
        let leading = if self.kind == ChipKind::Filter && selected { Some(crate::icons::CHECK) } else { self.leading };
        let galley = self.label.into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, th.font(TypeRole::LabelLarge));
        let lead_w = if leading.is_some() { 18.0 + 8.0 } else { 0.0 };
        let trail_w = if self.trailing.is_some() { 18.0 + 8.0 } else { 0.0 };
        let pad_l = if leading.is_some() { 8.0 } else { 16.0 };
        let pad_r = if self.trailing.is_some() { 8.0 } else { 16.0 };
        let size = vec2(pad_l + lead_w + galley.size().x + trail_w + pad_r, 32.0);
        let (rect, resp) = ui.allocate_exact_size(size, if self.enabled { Sense::click() } else { Sense::hover() });
        resp.widget_info(|| WidgetInfo::selected(Role::CheckBox, self.enabled, selected, galley.text()));
        if ui.is_rect_visible(rect) {
            let r = CornerRadius::same(shape::SMALL);
            let p = ui.painter();
            p.rect(rect, r, bg, stroke, StrokeKind::Inside);
            let o = ui.ctx().animate_value_with_time(resp.id, interaction_opacity(&resp), ANIM);
            if o > 0.0 && self.enabled {
                p.rect_filled(rect, r, with_alpha(fg, o));
            }
            let mut x = rect.left() + pad_l;
            let icon_font = FontId::new(18.0, egui::FontFamily::Name(ICONS.into()));
            if let Some(i) = leading {
                p.text(Pos2::new(x, rect.center().y), Align2::LEFT_CENTER, i, icon_font.clone(), if selected { fg } else { c.primary });
                x += lead_w;
            }
            p.galley_with_override_text_color(Pos2::new(x, rect.center().y - galley.size().y / 2.0), galley.clone(), fg);
            x += galley.size().x + 8.0;
            if let Some(i) = self.trailing {
                p.text(Pos2::new(x, rect.center().y), Align2::LEFT_CENTER, i, icon_font, fg);
            }
        }
        resp
    }
}

// ---------------------------------------------------------------------------
// Containment

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardKind {
    Elevated,
    Filled,
    Outlined,
}

/// M3 card container (medium radius).
pub fn card(ui: &Ui, kind: CardKind) -> Frame {
    let c = theme(ui.ctx()).colors;
    let base = Frame::new().corner_radius(CornerRadius::same(shape::MEDIUM)).inner_margin(Margin::same(12));
    match kind {
        CardKind::Elevated => base
            .fill(c.surface_container_low)
            .shadow(Shadow { offset: [0, 1], blur: 3, spread: 0, color: with_alpha(c.shadow, 0.25) }),
        CardKind::Filled => base.fill(c.surface_container_highest),
        CardKind::Outlined => base.fill(c.surface).stroke(Stroke::new(1.0, c.outline_variant)),
    }
}

/// A surface frame at the given container tone (for overlay widget windows).
pub fn surface(ui: &Ui, fill: Color32, radius: u8) -> Frame {
    let c = theme(ui.ctx()).colors;
    Frame::new()
        .fill(fill)
        .corner_radius(CornerRadius::same(radius))
        .inner_margin(Margin::same(16))
        .shadow(Shadow { offset: [0, 4], blur: 18, spread: 0, color: with_alpha(c.shadow, 0.4) })
}

/// M3 divider.
pub fn divider(ui: &mut Ui) {
    let c = theme(ui.ctx()).colors;
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(w, 1.0), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.center().y, Stroke::new(1.0, c.outline_variant));
}

/// Section heading in `title-small` + primary color, like M3 settings lists.
pub fn section_header(ui: &mut Ui, s: impl Into<String>) {
    let th = theme(ui.ctx());
    ui.add_space(8.0);
    ui.label(RichText::new(s).font(th.font(TypeRole::TitleSmall)).color(th.colors.primary));
    ui.add_space(4.0);
}

/// M3 one/two-line list item. Returns the row response (clickable).
pub fn list_item(ui: &mut Ui, leading_icon: Option<&str>, headline: &str, supporting: Option<&str>, trailing: Option<&str>) -> Response {
    let th = theme(ui.ctx());
    let c = th.colors;
    let h = if supporting.is_some() { 64.0 } else { 48.0 };
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
    let p = ui.painter();
    let o = ui.ctx().animate_value_with_time(resp.id, interaction_opacity(&resp), ANIM);
    if o > 0.0 {
        p.rect_filled(rect, CornerRadius::same(shape::MEDIUM), with_alpha(c.on_surface, o));
    }
    let mut x = rect.left() + 16.0;
    if let Some(i) = leading_icon {
        p.text(Pos2::new(x, rect.center().y), Align2::LEFT_CENTER, i, FontId::new(24.0, egui::FontFamily::Name(ICONS.into())), c.on_surface_variant);
        x += 40.0;
    }
    match supporting {
        Some(s) => {
            p.text(Pos2::new(x, rect.center().y - 2.0), Align2::LEFT_BOTTOM, headline, th.font(TypeRole::BodyLarge), c.on_surface);
            p.text(Pos2::new(x, rect.center().y + 2.0), Align2::LEFT_TOP, s, th.font(TypeRole::BodyMedium), c.on_surface_variant);
        }
        None => {
            p.text(Pos2::new(x, rect.center().y), Align2::LEFT_CENTER, headline, th.font(TypeRole::BodyLarge), c.on_surface);
        }
    }
    if let Some(t) = trailing {
        p.text(Pos2::new(rect.right() - 16.0, rect.center().y), Align2::RIGHT_CENTER, t, th.font(TypeRole::LabelSmall), c.on_surface_variant);
    }
    resp
}

// ---------------------------------------------------------------------------
// Navigation

pub struct NavItem<'a> {
    pub icon: &'a str,
    pub label: &'a str,
}

/// M3 navigation rail (80dp wide). Returns `Some(index)` when a destination is clicked.
pub fn navigation_rail(ui: &mut Ui, items: &[NavItem<'_>], selected: usize) -> Option<usize> {
    let th = theme(ui.ctx());
    let c = th.colors;
    let mut clicked = None;
    ui.vertical(|ui| {
        ui.set_width(80.0);
        ui.spacing_mut().item_spacing.y = 12.0;
        for (i, item) in items.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(vec2(80.0, 56.0), Sense::click());
            let is_sel = i == selected;
            let pill = Rect::from_center_size(Pos2::new(rect.center().x, rect.top() + 16.0), vec2(56.0, 32.0));
            let t = ui.ctx().animate_bool_with_time_and_easing(resp.id, is_sel, 0.2, easing::cubic_out);
            let p = ui.painter();
            if t > 0.0 {
                let w = 56.0 * t;
                p.rect_filled(Rect::from_center_size(pill.center(), vec2(w, 32.0)), CornerRadius::same(shape::FULL), c.secondary_container);
            }
            let o = interaction_opacity(&resp);
            if o > 0.0 {
                p.rect_filled(pill, CornerRadius::same(shape::FULL), with_alpha(c.on_surface, o));
            }
            let fg = if is_sel { c.on_secondary_container } else { c.on_surface_variant };
            p.text(pill.center(), Align2::CENTER_CENTER, item.icon, FontId::new(24.0, egui::FontFamily::Name(ICONS.into())), fg);
            p.text(
                Pos2::new(rect.center().x, pill.bottom() + 4.0),
                Align2::CENTER_TOP,
                item.label,
                th.font(TypeRole::LabelMedium),
                if is_sel { c.on_surface } else { c.on_surface_variant },
            );
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                clicked = Some(i);
            }
        }
    });
    clicked
}

/// M3 segmented button (single select). Returns true if selection changed.
pub fn segmented<T: PartialEq + Clone>(ui: &mut Ui, current: &mut T, options: &[(T, &str)]) -> bool {
    let th = theme(ui.ctx());
    let c = th.colors;
    let font = th.font(TypeRole::LabelLarge);
    let galleys: Vec<_> = options.iter().map(|(_, l)| ui.painter().layout_no_wrap((*l).to_owned(), font.clone(), c.on_surface)).collect();
    let seg_w: Vec<f32> = galleys.iter().map(|g| g.size().x + 24.0 + 26.0).collect();
    let total: f32 = seg_w.iter().sum();
    let (rect, _) = ui.allocate_exact_size(vec2(total, 40.0), Sense::hover());
    let mut changed = false;
    let mut x = rect.left();
    let n = options.len();
    for (i, ((value, _), g)) in options.iter().zip(galleys).enumerate() {
        let r = Rect::from_min_size(Pos2::new(x, rect.top()), vec2(seg_w[i], 40.0));
        let resp = ui.interact(r, ui.scope_id().with(("seg", i)), Sense::click());
        let sel = *current == *value;
        let radius = CornerRadius {
            nw: if i == 0 { shape::FULL } else { 0 },
            sw: if i == 0 { shape::FULL } else { 0 },
            ne: if i == n - 1 { shape::FULL } else { 0 },
            se: if i == n - 1 { shape::FULL } else { 0 },
        };
        let p = ui.painter();
        p.rect(r, radius, if sel { c.secondary_container } else { Color32::TRANSPARENT }, Stroke::new(1.0, c.outline), StrokeKind::Inside);
        let o = interaction_opacity(&resp);
        if o > 0.0 {
            p.rect_filled(r, radius, with_alpha(c.on_surface, o));
        }
        let fg = if sel { c.on_secondary_container } else { c.on_surface };
        let content_w = g.size().x + if sel { 26.0 } else { 0.0 };
        let mut cx = r.center().x - content_w / 2.0;
        if sel {
            p.text(Pos2::new(cx, r.center().y), Align2::LEFT_CENTER, crate::icons::CHECK, FontId::new(18.0, egui::FontFamily::Name(ICONS.into())), fg);
            cx += 26.0;
        }
        p.galley_with_override_text_color(Pos2::new(cx, r.center().y - g.size().y / 2.0), g, fg);
        if resp.clicked() && !sel {
            *current = value.clone();
            changed = true;
        }
        x += seg_w[i];
    }
    changed
}

// ---------------------------------------------------------------------------
// Feedback

/// M3 badge (small dot when `text` is None).
pub fn badge(ui: &mut Ui, text: Option<&str>) -> Response {
    let th = theme(ui.ctx());
    let c = th.colors;
    match text {
        None => {
            let (rect, resp) = ui.allocate_exact_size(Vec2::splat(6.0), Sense::hover());
            ui.painter().circle_filled(rect.center(), 3.0, c.error);
            resp
        }
        Some(t) => {
            let g = ui.painter().layout_no_wrap(t.to_owned(), th.font(TypeRole::LabelSmall), c.on_error);
            let (rect, resp) = ui.allocate_exact_size(vec2((g.size().x + 8.0).max(16.0), 16.0), Sense::hover());
            ui.painter().rect_filled(rect, CornerRadius::same(shape::FULL), c.error);
            ui.painter().galley(rect.center() - g.size() / 2.0, g, c.on_error);
            resp
        }
    }
}

/// M3 linear progress indicator. `progress = None` → indeterminate.
pub fn linear_progress(ui: &mut Ui, progress: Option<f32>) -> Response {
    let c = theme(ui.ctx()).colors;
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 4.0), Sense::hover());
    let p = ui.painter();
    let r = CornerRadius::same(2);
    p.rect_filled(rect, r, c.secondary_container);
    match progress {
        Some(f) => {
            let fill = Rect::from_min_size(rect.min, vec2(w * f.clamp(0.0, 1.0), 4.0));
            p.rect_filled(fill, r, c.primary);
        }
        None => {
            let t = (ui.input(|i| i.time) as f32 * 0.8).fract();
            let start = rect.left() + w * easing::cubic_in_out(t) - w * 0.3;
            let bar = Rect::from_x_y_ranges(start.max(rect.left())..=(start + w * 0.4).min(rect.right()), rect.y_range());
            if bar.width() > 0.0 {
                p.rect_filled(bar, r, c.primary);
            }
            ui.ctx().request_repaint();
        }
    }
    resp
}

/// M3 circular indeterminate progress (spinner).
pub fn circular_progress(ui: &mut Ui, size: f32) -> Response {
    let c = theme(ui.ctx()).colors;
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let t = ui.input(|i| i.time) as f32;
    let start = t * 4.0;
    let sweep = 1.0 + (t * 2.0).sin().abs() * 3.5;
    let radius = size / 2.0 - 2.0;
    let pts: Vec<Pos2> = (0..=32)
        .map(|i| {
            let a = start + sweep * i as f32 / 32.0;
            rect.center() + vec2(a.cos(), a.sin()) * radius
        })
        .collect();
    ui.painter().add(egui::Shape::line(pts, Stroke::new(3.0, c.primary)));
    ui.ctx().request_repaint();
    resp
}

/// An inline M3 "banner"/error container with icon.
pub fn banner(ui: &mut Ui, icon: &str, message: impl Into<String>, error: bool) {
    let c = theme(ui.ctx()).colors;
    let (bg, fg) = if error { (c.error_container, c.on_error_container) } else { (c.secondary_container, c.on_secondary_container) };
    Frame::new().fill(bg).corner_radius(CornerRadius::same(shape::MEDIUM)).inner_margin(Margin::symmetric(12, 8)).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(icon_text(icon, 20.0).color(fg));
            ui.label(RichText::new(message).color(fg));
        });
    });
}

/// Snackbar state helper: call [`Snackbar::show`] each frame.
#[derive(Default)]
pub struct Snackbar {
    message: Option<(String, f64)>,
}

impl Snackbar {
    pub fn push(&mut self, ctx: &egui::Context, msg: impl Into<String>) {
        self.message = Some((msg.into(), ctx.input(|i| i.time) + 4.0));
    }
    pub fn show(&mut self, ctx: &egui::Context) {
        let Some((msg, until)) = &self.message else { return };
        let now = ctx.input(|i| i.time);
        if now > *until {
            self.message = None;
            return;
        }
        let c = theme(ctx).colors;
        egui::Area::new(Id::unique("m3-snackbar"))
            .anchor(Align2::CENTER_BOTTOM, vec2(0.0, -24.0))
            .order(egui::Order::Tooltip)
            .show(ctx, |ui| {
                Frame::new()
                    .fill(c.inverse_surface)
                    .corner_radius(CornerRadius::same(shape::EXTRA_SMALL))
                    .inner_margin(Margin::symmetric(16, 14))
                    .shadow(Shadow { offset: [0, 3], blur: 8, spread: 0, color: with_alpha(c.shadow, 0.3) })
                    .show(ui, |ui| ui.label(RichText::new(msg.as_str()).color(c.inverse_on_surface)));
            });
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }
}

/// M3 top app bar row: optional nav icon, title, trailing actions.
pub fn top_app_bar(ui: &mut Ui, title: &str, actions: impl FnOnce(&mut Ui)) {
    let th = theme(ui.ctx());
    ui.horizontal(|ui| {
        ui.set_min_height(48.0);
        ui.label(RichText::new(title).font(th.font(TypeRole::TitleLarge)).color(th.colors.on_surface));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), actions);
    });
}

/// A styled single-line text field with M3 "filled" look.
pub fn text_field(ui: &mut Ui, value: &mut String, hint: &str) -> Response {
    let c = theme(ui.ctx()).colors;
    let resp = Frame::new()
        .fill(c.surface_container_highest)
        .corner_radius(CornerRadius { nw: shape::EXTRA_SMALL, ne: shape::EXTRA_SMALL, sw: 0, se: 0 })
        .inner_margin(Margin::symmetric(12, 8))
        .show(ui, |ui| ui.add(egui::TextEdit::singleline(value).hint_text(hint).frame(egui::Frame::NONE).desired_width(f32::INFINITY)).on_hover_cursor(egui::CursorIcon::Text))
        .inner;
    let rect = resp.rect.expand2(vec2(12.0, 8.0));
    let focused = resp.has_focus();
    ui.painter().hline(
        rect.x_range(),
        rect.bottom(),
        Stroke::new(if focused { 2.0 } else { 1.0 }, if focused { c.primary } else { c.on_surface_variant }),
    );
    resp
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    state_layer(a, b, t)
}
