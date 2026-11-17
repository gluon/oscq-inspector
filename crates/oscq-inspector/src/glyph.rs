//! Thin monoline icons, drawn with `egui`'s painter rather than taken from an icon font or an
//! icon crate: PRD 5 forbids adding a dependency without a reason recorded, and a handful of
//! strokes needs none. Each icon is drawn inside the square it is given, on the 4 px grid, with
//! one stroke width, so that a row of them reads as one set.
//!
//! An icon button carries no text, so a test cannot find it by its label. Each takes a stable
//! `egui::Id`, which `icon_button` returns in its response: a test reads the response back from
//! the context and looks for the strokes painted inside it.

use egui::{Color32, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2};

use crate::style;

/// The stroke of every icon: one hairline, as every border is.
pub const WEIGHT: f32 = 1.25;

/// The side of the square an icon is drawn in, inside a control of [`style::CONTROL_H`].
pub const SIZE: f32 = 4.0 * style::GRID;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    /// Remove, unpin, close: two strokes crossing.
    Close,
    /// Read again: a circle open at the top right, with an arrow head on its end.
    Refresh,
    /// A group that is open, and one that is folded.
    ChevronDown,
    ChevronRight,
    /// Copy: two squares, one behind the other.
    Copy,
    /// Save to a file: a tray with an arrow coming down into it.
    Save,
}

/// Draws `icon` inside `rect`, in `colour`. The strokes are laid out in a unit square and
/// scaled, so an icon keeps its proportions at any size.
pub fn paint(painter: &egui::Painter, rect: Rect, icon: Icon, colour: Color32) {
    // The drawing box: the square inside `rect`, inset so that a stroke stays inside it.
    let side = rect.width().min(rect.height());
    let box_ = Rect::from_center_size(rect.center(), Vec2::splat(side));
    let p = |x: f32, y: f32| Pos2::new(box_.min.x + x * side, box_.min.y + y * side);
    let stroke = Stroke::new(WEIGHT, colour);
    let line = |a: Pos2, b: Pos2| painter.line_segment([a, b], stroke);
    match icon {
        Icon::Close => {
            line(p(0.22, 0.22), p(0.78, 0.78));
            line(p(0.78, 0.22), p(0.22, 0.78));
        }
        Icon::Refresh => {
            // An arc of about 290 degrees, open at the top right, then the arrow head.
            let centre = p(0.5, 0.52);
            let radius = 0.30 * side;
            let from = -120.0_f32.to_radians();
            let to = 150.0_f32.to_radians();
            let steps = 20;
            let at = |a: f32| centre + Vec2::new(a.cos(), a.sin()) * radius;
            let points: Vec<Pos2> = (0..=steps)
                .map(|i| at(from + (to - from) * i as f32 / steps as f32))
                .collect();
            painter.add(egui::Shape::line(points, stroke));
            // The head on the end of the arc, two short strokes back along it.
            let end = at(from);
            line(end, end + Vec2::new(-0.16, 0.02) * side);
            line(end, end + Vec2::new(0.02, 0.17) * side);
        }
        Icon::ChevronDown => {
            line(p(0.26, 0.40), p(0.50, 0.64));
            line(p(0.74, 0.40), p(0.50, 0.64));
        }
        Icon::ChevronRight => {
            line(p(0.40, 0.26), p(0.64, 0.50));
            line(p(0.40, 0.74), p(0.64, 0.50));
        }
        Icon::Copy => {
            painter.rect_stroke(
                Rect::from_min_max(p(0.30, 0.30), p(0.80, 0.80)),
                0.0,
                stroke,
                egui::StrokeKind::Inside,
            );
            // The sheet behind: two sides only, so the two squares read apart.
            line(p(0.20, 0.70), p(0.20, 0.20));
            line(p(0.20, 0.20), p(0.70, 0.20));
        }
        Icon::Save => {
            line(p(0.50, 0.18), p(0.50, 0.58));
            line(p(0.34, 0.44), p(0.50, 0.60));
            line(p(0.66, 0.44), p(0.50, 0.60));
            line(p(0.20, 0.74), p(0.20, 0.82));
            line(p(0.20, 0.82), p(0.80, 0.82));
            line(p(0.80, 0.82), p(0.80, 0.74));
        }
    }
}

/// The `Id` of the icon button whose salt is `salt`. It does not hang off the `Ui` it is drawn
/// in: an icon button carries no text, so this is the only handle a test has on it, and a
/// whole id keeps that handle independent of where the button sits. Every call site gives a
/// salt of its own, a path or an address included where there is one button per row.
pub fn id_of(salt: &str) -> egui::Id {
    egui::Id::new(("oscq icon", salt))
}

/// An icon button: a square control with a 1 px border, its icon in the middle, and nothing
/// written on it. `id_salt` makes its `Id` through [`id_of`].
pub fn icon_button(ui: &mut Ui, icon: Icon, id_salt: &str) -> Response {
    button_sized(ui, icon, id_salt, Vec2::splat(style::CONTROL_H), true)
}

/// An icon button of a given size, with or without its frame.
pub fn button_sized(ui: &mut Ui, icon: Icon, id_salt: &str, size: Vec2, frame: bool) -> Response {
    let id = id_of(id_salt);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    // Interacted with our own id, not the one `allocate_exact_size` makes up: a test reads the
    // button's place back from the context by that id, since it carries no text to find.
    let response = ui.interact(rect, id, Sense::click());
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact(&response);
        if frame {
            ui.painter().rect(
                rect,
                0.0,
                visuals.weak_bg_fill,
                visuals.bg_stroke,
                egui::StrokeKind::Inside,
            );
        }
        let inner = Rect::from_center_size(rect.center(), Vec2::splat(SIZE));
        paint(ui.painter(), inner, icon, visuals.fg_stroke.color);
    }
    response
}

/// A small icon button with no frame, for the end of a row: removing a manual entry, unpinning
/// a node. It keeps the full control height so that the row's other cells line up with it.
pub fn row_button(ui: &mut Ui, icon: Icon, id_salt: &str, width: f32) -> Response {
    button_sized(ui, icon, id_salt, Vec2::new(width, style::CONTROL_H), false)
}
