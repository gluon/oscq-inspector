//! The application icon, drawn in code: the one source of the window icon, of the macOS `.icns`,
//! of the Windows executable's icon and of the Linux `.png`. An address space tree, a root and two
//! leaves, one of them lit as a live value, on a plain rounded square.

/// Where the artwork sits on its square canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// The macOS icon grid: the body takes 824 of 1024, centred, as every application icon does in
    /// the Finder and the Dock.
    MacOs,
    /// The body fills the canvas but for a pixel: Windows and Linux icons have no margin.
    Full,
}

const BODY: [f32; 3] = [0.122, 0.227, 0.373];
const INK: [f32; 3] = [0.957, 0.965, 0.973];
const LIVE: [f32; 3] = [1.0, 0.690, 0.125];

/// The icon at `size` by `size` pixels, RGBA, 8 bits per channel, not premultiplied, rows from
/// the top.
pub fn rgba(size: u32, layout: Layout) -> Vec<u8> {
    let px = size as f32;
    let margin = match layout {
        Layout::MacOs => px * 100.0 / 1024.0,
        Layout::Full => px / 32.0,
    };
    let body = px - 2.0 * margin;
    let corner = body * 0.225;
    // Glyph coordinates are fractions of the body. Below 32 pixels the strokes and nodes keep a
    // minimum size in pixels, so the tree stays readable at 16.
    let at = |u: f32, v: f32| (margin + u * body, margin + v * body);
    let stroke = (0.045 * body).max(0.7);
    let root_r = (0.09 * body).max(1.7);
    let leaf_r = (0.085 * body).max(1.6);
    let root = at(0.33, 0.28);
    let fork = at(0.33, 0.50);
    let foot = at(0.33, 0.72);
    let leaf_a = at(0.67, 0.50);
    let leaf_b = at(0.67, 0.72);
    let branch_end = |l: (f32, f32)| (l.0 - leaf_r, l.1);

    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let p = (x as f32 + 0.5, y as f32 + 0.5);
            let body_d = rounded_square(p, px / 2.0, body / 2.0, corner);
            let ink_d = [
                segment(p, root, foot) - stroke,
                segment(p, fork, branch_end(leaf_a)) - stroke,
                segment(p, foot, branch_end(leaf_b)) - stroke,
                circle(p, root, root_r),
                circle(p, leaf_a, leaf_r),
            ]
            .into_iter()
            .fold(f32::MAX, f32::min);
            let live_d = circle(p, leaf_b, leaf_r);
            let mut rgb = BODY;
            mix(&mut rgb, INK, coverage(ink_d));
            mix(&mut rgb, LIVE, coverage(live_d));
            for c in rgb {
                out.push(channel(c));
            }
            out.push(channel(coverage(body_d)));
        }
    }
    out
}

/// Area of the pixel inside a shape whose signed distance, in pixels, is `d` at its centre.
fn coverage(d: f32) -> f32 {
    (0.5 - d).clamp(0.0, 1.0)
}

fn mix(rgb: &mut [f32; 3], over: [f32; 3], a: f32) {
    for (c, o) in rgb.iter_mut().zip(over) {
        *c += (o - *c) * a;
    }
}

fn channel(c: f32) -> u8 {
    (c * 255.0).round() as u8
}

fn length(x: f32, y: f32) -> f32 {
    (x * x + y * y).sqrt()
}

fn rounded_square(p: (f32, f32), centre: f32, half: f32, corner: f32) -> f32 {
    let qx = (p.0 - centre).abs() - (half - corner);
    let qy = (p.1 - centre).abs() - (half - corner);
    length(qx.max(0.0), qy.max(0.0)) + qx.max(qy).min(0.0) - corner
}

fn circle(p: (f32, f32), c: (f32, f32), r: f32) -> f32 {
    length(p.0 - c.0, p.1 - c.1) - r
}

fn segment(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (bx, by) = (b.0 - a.0, b.1 - a.1);
    let (px, py) = (p.0 - a.0, p.1 - a.1);
    let t = ((px * bx + py * by) / (bx * bx + by * by)).clamp(0.0, 1.0);
    length(px - bx * t, py - by * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(img: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * size + x) * 4) as usize;
        [img[i], img[i + 1], img[i + 2], img[i + 3]]
    }

    #[test]
    fn every_size_is_drawn_with_its_body_glyph_and_live_node() {
        for layout in [Layout::MacOs, Layout::Full] {
            for size in [16, 32, 48, 256, 1024] {
                let img = rgba(size, layout);
                assert_eq!(img.len(), (size * size * 4) as usize);
                let count = |want: [u8; 3]| {
                    img.chunks(4)
                        .filter(|p| p[3] == 255 && p[..3] == want)
                        .count()
                };
                let rgb = |c: [f32; 3]| c.map(channel);
                assert!(count(rgb(BODY)) > 0, "{layout:?} {size}: body");
                assert!(count(rgb(INK)) > 0, "{layout:?} {size}: tree");
                assert!(count(rgb(LIVE)) > 0, "{layout:?} {size}: live node");
                assert_eq!(pixel(&img, size, 0, 0)[3], 0, "corners are transparent");
            }
        }
    }

    #[test]
    fn the_macos_grid_leaves_its_margin_and_full_does_not() {
        let mac = rgba(1024, Layout::MacOs);
        let full = rgba(1024, Layout::Full);
        assert_eq!(pixel(&mac, 1024, 512, 90)[3], 0);
        assert_eq!(pixel(&mac, 1024, 512, 110)[3], 255);
        assert_eq!(pixel(&full, 1024, 512, 40)[3], 255);
    }
}
