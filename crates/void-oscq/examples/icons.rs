//! Writes the icon files the release packaging needs, drawn by the application's own icon code:
//! `AppIcon.iconset/`, the PNG set `iconutil` turns into the macOS `.icns`, and `void-oscq.png`,
//! 256 pixels, for Linux.
//!
//! cargo run -p void-oscq --example icons -- OUT_DIR

#[allow(dead_code)]
#[path = "../src/icon.rs"]
mod icon;

use std::path::Path;

use icon::Layout;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).ok_or("usage: icons OUT_DIR")?;
    let iconset = Path::new(&out).join("AppIcon.iconset");
    std::fs::create_dir_all(&iconset)?;
    // The names iconutil expects: each point size at 1x and 2x.
    for points in [16, 32, 128, 256, 512] {
        write_png(
            &iconset.join(format!("icon_{points}x{points}.png")),
            points,
            Layout::MacOs,
        )?;
        write_png(
            &iconset.join(format!("icon_{points}x{points}@2x.png")),
            points * 2,
            Layout::MacOs,
        )?;
    }
    write_png(&Path::new(&out).join("void-oscq.png"), 256, Layout::Full)?;
    println!("{out}");
    Ok(())
}

fn write_png(path: &Path, size: u32, layout: Layout) -> Result<(), Box<dyn std::error::Error>> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut encoder = png::Encoder::new(file, size, size);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::High);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&icon::rgba(size, layout))?;
    writer.finish()?;
    Ok(())
}
