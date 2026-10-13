//! The visual system (PRD 9.1): a 4 px grid, one type scale, monospace for data and proportional
//! for the interface around it, groups apart by space rather than by rules.

use egui::{FontFamily, FontId, RichText, TextStyle};

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

/// Applies the grid and the type scale to every theme. Animations stay off (PRD 6.2): a fade asks
/// for a frame on every frame until it ends.
pub fn apply(ctx: &egui::Context) {
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
        sp.combo_width = CONTROL_W;
        sp.text_edit_width = CONTROL_W;
        sp.icon_width = 3.0 * GRID;
        sp.icon_spacing = GRID;
        sp.window_margin = egui::Margin::same(3 * GRID as i8);
        sp.menu_margin = egui::Margin::same(2 * GRID as i8);
        s.animation_time = 0.0;
        s.interaction.show_tooltips_only_when_still = false;
    });
}

/// Data: a path, a value, type tags, flags.
pub fn data(text: impl Into<String>) -> RichText {
    RichText::new(text).monospace()
}

/// The quiet label above a group.
pub fn section(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text.to_uppercase()).small().weak());
}
