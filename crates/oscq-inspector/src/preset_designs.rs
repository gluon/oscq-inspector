//! Three drawings of the capture part of the preset section, for the human to choose from.
//! Compiled only with the `screenshot` feature and reached only through
//! `OSCQ_INSPECTOR_PRESET_DESIGN`, so the shipped window draws exactly what it drew before.
//!
//! All three keep the sixteen square slots, four to a row, the number alone on each: that is the
//! human's decision of 2026-11-17 (PRD 9.9) and it is not what was reported unclear. What they
//! change is the capture around them, which today is on screen nowhere: the word "Capture" is in
//! a right click menu and in the hover text of an empty slot, the scope a capture would take is
//! in that hover text alone, and a plain click captures or recalls depending on a state the slot
//! shows only as a change of border.
//!
//! - [`Design::Bar`]: a capture control above the grid, naming the scope it would take. Armed by
//!   a click, it takes the next slot clicked; a plain click on a slot then always recalls.
//! - [`Design::Line`]: the grid as it is, and one line under it carrying what the slot pointed at
//!   holds, with **Capture** and **Clear** acting on that slot.
//! - [`Design::Marked`]: the slot itself carries its count and its server, so a filled slot can be
//!   told from another without the pointer, with the capture control of [`Design::Bar`] above.
//!
//! These are drawings. They are not wired to the config and nothing they show is kept: the state
//! they draw is the state the window is in, and their controls do nothing.

use egui::Color32;

use crate::presets::Preset;
use crate::style::{self, GRID};
use crate::{glyph, presets};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Design {
    Bar,
    Line,
    Marked,
}

impl Design {
    /// The design `OSCQ_INSPECTOR_PRESET_DESIGN` asks for, or `None` for the shipped section.
    pub fn from_env() -> Option<Self> {
        match std::env::var("OSCQ_INSPECTOR_PRESET_DESIGN").ok()?.as_str() {
            "a" | "bar" => Some(Self::Bar),
            "b" | "line" => Some(Self::Line),
            "c" | "marked" => Some(Self::Marked),
            _ => None,
        }
    }

    /// The room the section takes under its header, in place of the shipped 58 grid units.
    pub fn height(self) -> f32 {
        match self {
            Self::Bar => 66.0 * GRID,
            Self::Line => 72.0 * GRID,
            Self::Marked => 76.0 * GRID,
        }
    }
}

/// The slot the line of [`Design::Line`] speaks of, which a pointer would choose and a
/// screenshot has none to choose with: `OSCQ_INSPECTOR_PRESET_POINTED`, slot 1 by default.
pub fn pointed_from_env() -> usize {
    std::env::var("OSCQ_INSPECTOR_PRESET_POINTED")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|n| (1..=presets::SLOTS).contains(n))
        .unwrap_or(1)
}

/// What the drawing needs of the window: the slots, what a capture here would take, and the slot
/// the line of [`Design::Line`] speaks of.
pub struct Shown<'a> {
    pub presets: &'a std::collections::BTreeMap<usize, Preset>,
    pub recalled: Option<usize>,
    /// How many methods a capture would take now, and whether the scope is the pinned ones.
    pub would_take: usize,
    pub pinned_scope: bool,
    /// The slot the pointer is on, else the one last touched: what the line describes.
    pub pointed: usize,
}

pub fn draw(ui: &mut egui::Ui, design: Design, shown: &Shown) {
    match design {
        Design::Bar => {
            capture_bar(ui, shown);
            ui.add_space(GRID);
            grid(ui, shown, 10.0 * GRID, false);
        }
        Design::Line => {
            grid(ui, shown, 10.0 * GRID, false);
            ui.add_space(2.0 * GRID);
            slot_line(ui, shown);
        }
        Design::Marked => {
            capture_bar(ui, shown);
            ui.add_space(GRID);
            grid(ui, shown, 12.0 * GRID, true);
        }
    }
}

/// The scope a capture would take, in the words the control shows: "12 WRITABLE", "3 PINNED".
fn scope_words(shown: &Shown) -> String {
    let what = if shown.pinned_scope {
        "pinned"
    } else {
        "writable"
    };
    format!("{} {what}", shown.would_take)
}

/// A capture control the width of the pane: the word, the glyph, and the scope it would take.
/// It is what the section has nowhere today.
fn capture_bar(ui: &mut egui::Ui, shown: &Shown) {
    let p = style::palette(ui.visuals());
    let width = ui.available_width();
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, style::CONTROL_H), egui::Sense::click());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let ground = if response.hovered() {
        p.muted
    } else {
        p.surface
    };
    ui.painter().rect(
        rect,
        0.0,
        ground,
        egui::Stroke::new(style::HAIRLINE, p.border),
        egui::StrokeKind::Inside,
    );
    let icon = egui::Rect::from_min_size(
        egui::pos2(rect.left() + GRID, rect.center().y - glyph::SIZE * 0.5),
        egui::Vec2::splat(glyph::SIZE),
    );
    glyph::paint(ui.painter(), icon, glyph::Icon::Capture, p.text);
    let word = style::caps_galley(ui, "Capture", p.text);
    ui.painter().galley(
        egui::pos2(icon.right() + GRID, rect.center().y - word.size().y * 0.5),
        word,
        p.text,
    );
    let scope = style::caps_galley(ui, &scope_words(shown), p.label);
    ui.painter().galley(
        egui::pos2(
            rect.right() - GRID - scope.size().x,
            rect.center().y - scope.size().y * 0.5,
        ),
        scope,
        p.label,
    );
}

/// The sixteen squares. `side` is the square's side; `marked` draws what a filled slot holds
/// inside it, under the number.
fn grid(ui: &mut egui::Ui, shown: &Shown, side: f32, marked: bool) {
    let p = style::palette(ui.visuals());
    let cols = (((ui.available_width() + GRID) / (side + GRID)).floor() as usize).clamp(1, 4);
    for row in 0..presets::SLOTS.div_ceil(cols) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GRID;
            for col in 0..cols {
                let slot = row * cols + col + 1;
                if slot > presets::SLOTS {
                    break;
                }
                let held = shown.presets.get(&slot);
                let last = shown.recalled == Some(slot);
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
                if !ui.is_rect_visible(rect) {
                    continue;
                }
                let (ground, edge, ink) = match (held.is_some(), last) {
                    (true, true) => (p.text, p.text, p.bg),
                    (true, false) => (p.surface, p.text, p.text),
                    (false, _) => (p.bg, p.border, p.dim),
                };
                ui.painter().rect(
                    rect,
                    0.0,
                    ground,
                    egui::Stroke::new(style::HAIRLINE, edge),
                    egui::StrokeKind::Inside,
                );
                match (marked, held) {
                    // The number alone, centred, as the shipped slot draws it.
                    (false, _) | (true, None) => {
                        number(ui, rect.center(), slot, ink);
                    }
                    // The number high in the square, the count and the server under it: a
                    // filled slot is told from another without the pointer.
                    (true, Some(pre)) => {
                        number(ui, rect.center() - egui::vec2(0.0, 2.5 * GRID), slot, ink);
                        small(
                            ui,
                            rect.center() + egui::vec2(0.0, GRID * 0.5),
                            &pre.values.len().to_string(),
                            ink,
                        );
                        small(
                            ui,
                            rect.center() + egui::vec2(0.0, 3.0 * GRID),
                            &short(&pre.server_name, 5),
                            if last { p.bg } else { p.label },
                        );
                    }
                }
            }
        });
    }
}

/// One line under the grid: what the slot pointed at holds, and the two actions on it, in words.
fn slot_line(ui: &mut egui::Ui, shown: &Shown) {
    let p = style::palette(ui.visuals());
    let slot = shown.pointed;
    let text = match shown.presets.get(&slot) {
        Some(pre) => format!("{} values, {}", pre.values.len(), pre.server_name),
        None => format!("empty, would take {}", scope_words(shown)),
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GRID;
        let number = style::caps_galley(ui, &format!("{slot:02}"), p.text);
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(number.size().x, style::CONTROL_H),
            egui::Sense::hover(),
        );
        ui.painter().galley(
            egui::pos2(rect.left(), rect.center().y - number.size().y * 0.5),
            number,
            p.text,
        );
        let body = style::caps_galley(ui, &text, p.label);
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(body.size().x, style::CONTROL_H),
            egui::Sense::hover(),
        );
        ui.painter().galley(
            egui::pos2(rect.left(), rect.center().y - body.size().y * 0.5),
            body,
            p.label,
        );
    });
    ui.add_space(GRID);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GRID;
        word_button(ui, "Capture", true);
        word_button(ui, "Recall", shown.presets.contains_key(&slot));
        word_button(ui, "Clear", shown.presets.contains_key(&slot));
    });
}

/// A control naming its action in words, dim when it would do nothing.
fn word_button(ui: &mut egui::Ui, text: &str, enabled: bool) {
    let p = style::palette(ui.visuals());
    let ink = if enabled { p.text } else { p.dim };
    let galley = style::caps_galley(ui, text, ink);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(galley.size().x + 3.0 * GRID, style::CONTROL_H),
        egui::Sense::click(),
    );
    if !ui.is_rect_visible(rect) {
        return;
    }
    let ground = if response.hovered() && enabled {
        p.muted
    } else {
        p.surface
    };
    ui.painter().rect(
        rect,
        0.0,
        ground,
        egui::Stroke::new(style::HAIRLINE, if enabled { p.border } else { p.subtle }),
        egui::StrokeKind::Inside,
    );
    ui.painter().galley(
        egui::pos2(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        ink,
    );
}

fn number(ui: &egui::Ui, centre: egui::Pos2, slot: usize, colour: Color32) {
    let galley = ui.ctx().fonts_mut(|f| {
        f.layout_no_wrap(
            format!("{slot:02}"),
            egui::FontId::new(style::DATA, egui::FontFamily::Monospace),
            colour,
        )
    });
    ui.painter()
        .galley(centre - galley.size() * 0.5, galley, colour);
}

fn small(ui: &egui::Ui, centre: egui::Pos2, text: &str, colour: Color32) {
    let galley = ui.ctx().fonts_mut(|f| {
        f.layout_no_wrap(
            text.to_owned(),
            egui::FontId::new(9.0, egui::FontFamily::Monospace),
            colour,
        )
    });
    ui.painter()
        .galley(centre - galley.size() * 0.5, galley, colour);
}

/// A name cut to `n` characters: what fits in a square without leaving it.
fn short(name: &str, n: usize) -> String {
    name.chars().take(n).collect()
}
