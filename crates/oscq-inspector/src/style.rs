//! The visual system (PRD 9.1): a 4 px grid, one type scale, monospace for data and proportional
//! for the interface around it, groups apart by space rather than by rules.
//!
//! Since the human's decision of 2026-11-17 the palette, the shape and the typefaces are those
//! of the Structure Void design system: a greyscale ground with one signal colour for everything
//! live or needing attention, square corners, no shadows, 1 px borders, Space Grotesk for the
//! interface and Space Mono for data. Both typefaces are bundled under the SIL Open Font
//! License; their license texts are in `licenses/fonts` and checked by `ci/check-licenses.sh`.

use egui::{Color32, CornerRadius, FontFamily, FontId, RichText, Shadow, Stroke, TextStyle};

/// The base grid. Every size, margin and gap below is a multiple of it.
pub const GRID: f32 = 4.0;
/// A row of the tree, the pinned panel and the server list.
pub const ROW_H: f32 = 6.0 * GRID;
/// The height of a control inside a row: a 2 px margin above and below.
pub const CONTROL_H: f32 = 5.0 * GRID;
/// One level of the tree.
pub const INDENT: f32 = 4.0 * GRID;
/// Between groups.
pub const GROUP_GAP: f32 = 4.0 * GRID;
/// Inside a group.
pub const ITEM_GAP: f32 = 2.0 * GRID;
/// The metadata column: the "sent" mark, type tags and access flags.
pub const META_W: f32 = 30.0 * GRID;
/// Sliders, text fields and dropdowns in the control column.
pub const CONTROL_W: f32 = 40.0 * GRID;
/// The least width the central pane keeps, whatever the window and however the side panes were
/// dragged: its toolbar fits on wrapped lines, and a tree row keeps its widget and access flags.
pub const CENTRE_MIN: f32 = 56.0 * GRID;
/// The server pane narrows to this first when the window does.
pub const SERVERS_MIN: f32 = 32.0 * GRID;
/// The work pane: four preset slots to a row at its default width, three at its least.
pub const WORK_DEFAULT: f32 = 76.0 * GRID;
pub const WORK_MIN: f32 = 72.0 * GRID;
/// The tree's name column before the metadata column gives way, and the metadata column once it
/// has, the access flags alone.
pub const NAME_FLOOR: f32 = 24.0 * GRID;
pub const FLAGS_W: f32 = 6.0 * GRID;

/// The type scale: 11, 13 and 15 px for the interface, 12 px monospace for data.
pub const SMALL: f32 = 11.0;
pub const BODY: f32 = 13.0;
pub const HEADING: f32 = 15.0;
pub const DATA: f32 = 12.0;

/// The tracking of a capitalised label, about 0.14 em at [`SMALL`].
pub const TRACKING: f32 = 1.5;

/// A hairline and a border: one physical pixel at any scale, since `egui` paints in points and
/// the window is drawn at the system's scale.
pub const HAIRLINE: f32 = 1.0;

/// The one signal colour, in both themes: everything live or needing attention, and nothing
/// else. A state that has merely been observed is grey (CLAUDE.md law 1).
pub const SIGNAL: Color32 = Color32::from_rgb(0xFF, 0x7A, 0x1A);

/// The greys of one theme, darkest ground to brightest text in the dark theme and the same
/// system inverted in the light one.
pub struct Palette {
    /// The window and its panels.
    pub bg: Color32,
    /// A surface above the ground: a modal, a menu, a striped row.
    pub raised: Color32,
    /// A control's own ground: a button, a field, a filled preset slot, a selected tree row.
    pub surface: Color32,
    /// A control's ground under the pointer.
    pub muted: Color32,
    /// A rule between zones, and a border that must not be noticed.
    pub subtle: Color32,
    /// The default 1 px border, and the rail of a slider.
    pub border: Color32,
    /// A control's own text, and the brightest text there is.
    pub text: Color32,
    /// Running text.
    pub body: Color32,
    /// A section label, a unit, a hint.
    pub label: Color32,
    /// Disabled, and nothing else at this size.
    pub dim: Color32,
}

/// The dark theme, the one the system is designed in.
pub const DARK: Palette = Palette {
    bg: Color32::from_rgb(0x00, 0x00, 0x00),
    raised: Color32::from_rgb(0x0A, 0x0A, 0x0A),
    surface: Color32::from_rgb(0x11, 0x11, 0x11),
    muted: Color32::from_rgb(0x1A, 0x1A, 0x1A),
    subtle: Color32::from_rgb(0x22, 0x22, 0x22),
    border: Color32::from_rgb(0x33, 0x33, 0x33),
    text: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    body: Color32::from_rgb(0xBB, 0xBB, 0xBB),
    label: Color32::from_rgb(0x99, 0x99, 0x99),
    dim: Color32::from_rgb(0x66, 0x66, 0x66),
};

/// The same system inverted. Its greys are not the dark ones subtracted from white: each is
/// chosen so that the hierarchy reads at least as strongly as in the dark theme, which PRD 9.1
/// requires and `the_light_theme_contrasts_as_the_dark_one_does` measures. In particular a
/// control's ground stays close to white, because the dark theme's white on `#111111` is a
/// contrast of 18.9 that a grey ground could not match with black text.
pub const LIGHT: Palette = Palette {
    bg: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    raised: Color32::from_rgb(0xFA, 0xFA, 0xFA),
    surface: Color32::from_rgb(0xF7, 0xF7, 0xF7),
    muted: Color32::from_rgb(0xEC, 0xEC, 0xEC),
    subtle: Color32::from_rgb(0xDA, 0xDA, 0xDA),
    border: Color32::from_rgb(0xA8, 0xA8, 0xA8),
    text: Color32::from_rgb(0x00, 0x00, 0x00),
    body: Color32::from_rgb(0x2E, 0x2E, 0x2E),
    label: Color32::from_rgb(0x4F, 0x4F, 0x4F),
    dim: Color32::from_rgb(0x8C, 0x8C, 0x8C),
};

/// The palette of the theme a `Ui` is drawing in.
pub fn palette(visuals: &egui::Visuals) -> &'static Palette {
    if visuals.dark_mode { &DARK } else { &LIGHT }
}

/// Applies the fonts, the grid and the type scale to every theme. Animations stay off
/// (PRD 6.2): a fade asks for a frame on every frame until it ends.
pub fn apply(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    ctx.all_styles_mut(|s| {
        s.text_styles = [
            (
                TextStyle::Small,
                FontId::new(SMALL, FontFamily::Proportional),
            ),
            (TextStyle::Body, FontId::new(BODY, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(BODY, FontFamily::Proportional),
            ),
            (
                TextStyle::Heading,
                FontId::new(HEADING, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(DATA, FontFamily::Monospace),
            ),
        ]
        .into();
        let sp = &mut s.spacing;
        sp.item_spacing = egui::vec2(ITEM_GAP, GRID);
        sp.button_padding = egui::vec2(ITEM_GAP, 0.0);
        sp.interact_size = egui::vec2(10.0 * GRID, CONTROL_H);
        sp.indent = INDENT;
        sp.slider_width = CONTROL_W - 18.0 * GRID;
        // A 2 px track, with the thumb drawn as a 2 px vertical bar: `HandleShape::Rect` takes
        // its width from the handle's radius, which is a fraction of the row's height.
        sp.slider_rail_height = 2.0;
        sp.combo_width = CONTROL_W;
        sp.text_edit_width = CONTROL_W;
        sp.icon_width = 3.0 * GRID;
        sp.icon_spacing = GRID;
        sp.window_margin = egui::Margin::same(3 * GRID as i8);
        sp.menu_margin = egui::Margin::same(2 * GRID as i8);
        s.animation_time = 0.0;
        s.interaction.show_tooltips_only_when_still = false;
    });
    ctx.set_visuals_of(egui::Theme::Dark, visuals_of(&DARK, true));
    ctx.set_visuals_of(egui::Theme::Light, visuals_of(&LIGHT, false));
}

/// Space Grotesk for the interface, Space Mono for data, each ahead of the fonts `epaint`
/// ships: those stay as the fallbacks that carry the glyphs `egui` itself draws, the collapsing
/// triangles and the resize handles among them, and their license texts stay required.
pub fn fonts() -> egui::FontDefinitions {
    let mut f = egui::FontDefinitions::default();
    f.font_data.insert(
        "SpaceGrotesk".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/SpaceGrotesk-Regular.ttf"
        ))),
    );
    f.font_data.insert(
        "SpaceMono".to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../assets/fonts/SpaceMono-Regular.ttf"
        ))),
    );
    for (family, name) in [
        (FontFamily::Proportional, "SpaceGrotesk"),
        (FontFamily::Monospace, "SpaceMono"),
    ] {
        f.families
            .entry(family)
            .or_default()
            .insert(0, name.to_owned());
    }
    f
}

/// One theme from one palette. Square corners, no shadow, 1 px borders, and a selection that
/// inverts: the ground takes the colour of the text and the text that of the ground.
pub fn visuals_of(p: &Palette, dark: bool) -> egui::Visuals {
    let mut v = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.dark_mode = dark;
    v.panel_fill = p.bg;
    v.window_fill = p.raised;
    v.window_stroke = Stroke::new(HAIRLINE, p.border);
    v.window_corner_radius = CornerRadius::ZERO;
    v.menu_corner_radius = CornerRadius::ZERO;
    v.window_shadow = Shadow::NONE;
    v.popup_shadow = Shadow::NONE;
    v.extreme_bg_color = p.surface;
    v.faint_bg_color = p.raised;
    v.code_bg_color = p.surface;
    v.weak_text_color = Some(p.label);
    // One signal colour: a warning and an error share it, and each says in words what it is.
    v.warn_fg_color = SIGNAL;
    v.error_fg_color = SIGNAL;
    v.hyperlink_color = SIGNAL;
    v.selection = egui::style::Selection {
        bg_fill: p.text,
        stroke: Stroke::new(HAIRLINE, p.bg),
    };
    v.handle_shape = egui::style::HandleShape::Rect {
        // The handle's half height is `ROW_H / 2.5`; a ratio of its inverse gives a 2 px bar.
        aspect_ratio: 1.0 / (ROW_H / 2.5),
    };
    v.slider_trailing_fill = false;
    v.striped = false;
    v.button_frame = true;
    // Disabled text lands on the palette's dimmest grey and goes no further.
    v.disabled_alpha = 0.55;
    let square =
        |weak_bg: Color32, bg: Color32, border: Color32, fg: Color32| egui::style::WidgetVisuals {
            bg_fill: bg,
            weak_bg_fill: weak_bg,
            bg_stroke: Stroke::new(HAIRLINE, border),
            corner_radius: CornerRadius::ZERO,
            fg_stroke: Stroke::new(HAIRLINE, fg),
            expansion: 0.0,
        };
    let w = &mut v.widgets;
    // Text, separators and guides: no ground of their own, a subtle rule, running text.
    w.noninteractive = square(p.bg, p.bg, p.subtle, p.body);
    // A control at rest: its own surface, a default border, its text at full strength. The
    // `bg_fill` is the rail of a slider and the box of a check box.
    w.inactive = square(p.surface, p.border, p.border, p.text);
    w.hovered = square(p.muted, p.dim, p.text, p.text);
    w.active = square(p.subtle, p.label, p.text, p.text);
    w.open = square(p.surface, p.border, p.dim, p.text);
    v
}

/// Data: a path, a value, type tags, flags.
pub fn data(text: impl Into<String>) -> RichText {
    RichText::new(text).monospace()
}

/// A label of the interface's own vocabulary: a section, a tab, a small button. Capitals at
/// [`SMALL`], tracked, so that it reads as a label and not as a value.
pub fn caps(text: &str) -> RichText {
    RichText::new(text.to_uppercase())
        .size(SMALL)
        .extra_letter_spacing(TRACKING)
}

/// A capitalised, tracked label laid out as one line, for painting by hand: the segmented bar
/// measures its cells with it and then paints with it, so the two never disagree.
pub fn caps_galley(ui: &egui::Ui, text: &str, colour: Color32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_uppercase(),
        egui::TextFormat {
            font_id: FontId::new(SMALL, FontFamily::Proportional),
            color: colour,
            extra_letter_spacing: TRACKING,
            ..Default::default()
        },
    );
    job.wrap.max_width = f32::INFINITY;
    ui.ctx().fonts_mut(|f| f.layout_job(job))
}

/// The quiet label above a group.
pub fn section(ui: &mut egui::Ui, text: &str) {
    let colour = ui.visuals().weak_text_color();
    ui.label(caps(text).color(colour));
}

/// One segmented bar: the choices side by side inside a single 1 px border, split by 1 px
/// rules, the chosen one inverted. The whole bar is allocated as one rectangle, so a wrapping
/// toolbar never breaks it across two lines.
///
/// `segments` gives each choice its value, its label and its hover text. Returns true when the
/// choice changed.
pub fn segmented<T: PartialEq + Copy>(
    ui: &mut egui::Ui,
    current: &mut T,
    segments: &[(T, &str, &str)],
) -> bool {
    let p = palette(ui.visuals());
    // Each cell takes its label's width and the same padding, so the bar reads as one object.
    let widths: Vec<f32> = segments
        .iter()
        .map(|(_, label, _)| (caps_galley(ui, label, p.text).size().x + 3.0 * GRID).ceil())
        .collect();
    let total = widths.iter().sum::<f32>() + HAIRLINE * (segments.len() as f32 + 1.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(total, CONTROL_H), egui::Sense::hover());
    let mut changed = false;
    let mut x = rect.min.x + HAIRLINE;
    for ((value, label, hover), width) in segments.iter().zip(&widths) {
        let cell = egui::Rect::from_min_size(
            egui::pos2(x, rect.min.y + HAIRLINE),
            egui::vec2(*width, rect.height() - 2.0 * HAIRLINE),
        );
        let id = ui.id().with(("segment", label));
        let response = ui.interact(cell, id, egui::Sense::click());
        let chosen = *current == *value;
        if ui.is_rect_visible(cell) {
            let (ground, ink) = match (chosen, response.hovered()) {
                (true, _) => (p.text, p.bg),
                (false, true) => (p.muted, p.text),
                (false, false) => (p.bg, p.label),
            };
            ui.painter().rect_filled(cell, 0.0, ground);
            let galley = caps_galley(ui, label, ink);
            let at = cell.center() - galley.size() * 0.5;
            ui.painter().galley(at, galley, ink);
        }
        if response.clicked() && !chosen {
            *current = *value;
            changed = true;
        }
        if !hover.is_empty() {
            response.on_hover_text(*hover);
        }
        x += width + HAIRLINE;
    }
    // The border around the group, and the rules between the cells, drawn over the grounds.
    let stroke = Stroke::new(HAIRLINE, p.border);
    ui.painter()
        .rect_stroke(rect, 0.0, stroke, egui::StrokeKind::Inside);
    let mut x = rect.min.x + HAIRLINE;
    for width in widths.iter().take(segments.len().saturating_sub(1)) {
        x += width + HAIRLINE;
        ui.painter().vline(
            x - HAIRLINE * 0.5,
            egui::Rangef::new(rect.min.y, rect.max.y),
            stroke,
        );
    }
    changed
}

/// The separator between two zones of one line: a 1 px rule down the middle of a gap of
/// [`GROUP_GAP`]. It takes the width the gap between two zones took before there was a rule, so
/// that adding rules to a line of zones does not make the line wider. Its caller sets the
/// layout's item spacing to zero and gives each zone its gap through this.
pub fn divider(ui: &mut egui::Ui, height: f32) {
    let colour = palette(ui.visuals()).subtle;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(GROUP_GAP, height), egui::Sense::hover());
    ui.painter().vline(
        rect.center().x.round() + 0.5,
        rect.y_range(),
        Stroke::new(HAIRLINE, colour),
    );
}
