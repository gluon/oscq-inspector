//! The icon study of 2026-11-19: the icon that ships and the three directions beside it, at the
//! sizes a Dock and a window really draw, on a light Dock ground and on a dark one.
//!
//! Needs the `screenshot` feature, which carries the directions (`src/icon.rs`):
//!
//! ```text
//! cargo run -p oscq-inspector --features screenshot --example icon_study -- OUT_DIR
//! ```
//!
//! What it writes into OUT_DIR:
//!
//! - `NAME-1024.png`, `NAME-256.png`, `NAME-32.png`: the artwork alone, transparent around it,
//!   for `current`, `bars`, `stem` and `quad`.
//! - `NAME-1024-dark.png` and `NAME-1024-light.png`: the master on each Dock ground.
//! - `dock-dark.png` and `dock-light.png`: one sheet per ground, the four icons in rows in the
//!   order above, at 256, 128, 64 and 32 across, each at its true size. Nothing is scaled: a
//!   32 pixel icon on these sheets is 32 pixels.

#[allow(dead_code)]
#[path = "../src/icon.rs"]
mod icon;

use std::path::Path;

use icon::{Direction, Layout};

/// The Dock's ground, dark desktop and light desktop: what an icon is judged against.
const DARK_DOCK: [u8; 3] = [38, 38, 38];
const LIGHT_DOCK: [u8; 3] = [232, 232, 232];

/// The four, in the order the sheets draw them in rows.
fn artwork(name: &str, size: u32) -> Vec<u8> {
    match name {
        "current" => icon::rgba(size, Layout::MacOs),
        "bars" => icon::rgba_of(size, Layout::MacOs, Direction::Bars),
        "stem" => icon::rgba_of(size, Layout::MacOs, Direction::Stem),
        "quad" => icon::rgba_of(size, Layout::MacOs, Direction::Quad),
        other => panic!("no artwork named {other}"),
    }
}

const NAMES: [&str; 4] = ["current", "bars", "stem", "quad"];
const SIZES: [u32; 4] = [256, 128, 64, 32];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).ok_or("usage: icon_study OUT_DIR")?;
    let dir = Path::new(&out);
    std::fs::create_dir_all(dir)?;

    for name in NAMES {
        for size in [1024, 256, 32] {
            let img = artwork(name, size);
            write_png(&dir.join(format!("{name}-{size}.png")), size, size, &img)?;
        }
        let master = artwork(name, 1024);
        for (ground, label) in [(DARK_DOCK, "dark"), (LIGHT_DOCK, "light")] {
            let (w, h, img) = on_ground(&master, 1024, ground, 64);
            write_png(&dir.join(format!("{name}-1024-{label}.png")), w, h, &img)?;
        }
    }

    for (ground, label) in [(DARK_DOCK, "dark"), (LIGHT_DOCK, "light")] {
        let (w, h, img) = sheet(ground);
        write_png(&dir.join(format!("dock-{label}.png")), w, h, &img)?;
    }
    println!("{out}");
    Ok(())
}

/// One icon on a Dock ground, with `pad` pixels around it. RGB, opaque.
fn on_ground(icon: &[u8], size: u32, ground: [u8; 3], pad: u32) -> (u32, u32, Vec<u8>) {
    let side = size + 2 * pad;
    let mut out = vec![0u8; (side * side * 3) as usize];
    for y in 0..side {
        for x in 0..side {
            let at = ((y * side + x) * 3) as usize;
            out[at..at + 3].copy_from_slice(&ground);
        }
    }
    blit(&mut out, side, icon, size, pad, pad);
    (side, side, out)
}

/// The sheet: one row per artwork, the sizes across, every icon at its true size. The cell is as
/// wide and as tall as the largest icon plus its margin, so nothing is scaled and the rows line
/// up on their centres.
fn sheet(ground: [u8; 3]) -> (u32, u32, Vec<u8>) {
    let cell = SIZES[0] + 48;
    let w = cell * SIZES.len() as u32;
    let h = cell * NAMES.len() as u32;
    let mut out = vec![0u8; (w * h * 3) as usize];
    for y in 0..h {
        for x in 0..w {
            let at = ((y * w + x) * 3) as usize;
            out[at..at + 3].copy_from_slice(&ground);
        }
    }
    for (row, name) in NAMES.iter().enumerate() {
        for (col, size) in SIZES.iter().enumerate() {
            let img = artwork(name, *size);
            let x = col as u32 * cell + (cell - size) / 2;
            let y = row as u32 * cell + (cell - size) / 2;
            blit(&mut out, w, &img, *size, x, y);
        }
    }
    (w, h, out)
}

/// Draws RGBA `icon` over RGB `out` at `x`, `y`, source over.
fn blit(out: &mut [u8], out_w: u32, icon: &[u8], size: u32, x: u32, y: u32) {
    for iy in 0..size {
        for ix in 0..size {
            let s = ((iy * size + ix) * 4) as usize;
            let a = f32::from(icon[s + 3]) / 255.0;
            if a <= 0.0 {
                continue;
            }
            let d = (((y + iy) * out_w + x + ix) * 3) as usize;
            for c in 0..3 {
                let over = f32::from(icon[s + c]);
                let under = f32::from(out[d + c]);
                out[d + c] = (under + (over - under) * a).round() as u8;
            }
        }
    }
}

fn write_png(path: &Path, w: u32, h: u32, data: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let colour = match data.len() as u32 / (w * h) {
        4 => png::ColorType::Rgba,
        _ => png::ColorType::Rgb,
    };
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut encoder = png::Encoder::new(file, w, h);
    encoder.set_color(colour);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::High);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(data)?;
    writer.finish()?;
    Ok(())
}
