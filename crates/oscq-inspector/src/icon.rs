//! The application icon, drawn in code: the one source of the window icon, of the macOS `.icns`,
//! of the Windows executable's icon and of the Linux `.png`. A round magnifier on a black square:
//! a white ring, a black void, an orange core, and a white handle. It follows the charter of the
//! Structure Void application icons: a black ground, one white mass, one element in the signal
//! colour, drawn on a 16 by 16 grid whose strokes and gaps are 2 units or more.

/// Where the artwork sits on its square canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// The macOS icon grid: the body takes 824 of 1024, centred, as every application icon does in
    /// the Finder and the Dock.
    MacOs,
    /// The body is the whole canvas, a square with no margin and no rounding: Windows and Linux
    /// draw an icon as it is given, and the charter wants it square wherever the system allows.
    Full,
}

const BODY: [f32; 3] = [0.0, 0.0, 0.0];
const INK: [f32; 3] = [1.0, 1.0, 1.0];
/// `#FF7A1A`, the signal colour of the application, `style::SIGNAL`.
const LIVE: [f32; 3] = [1.0, 0.478, 0.102];

/// The glyph on the 16 unit grid of the body: the lens, its void and its core share one centre;
/// the handle runs from inside the ring, so that no gap parts it from the lens, to the corner.
const LENS: (f32, f32) = (6.5, 6.5);
const RING_OUTER: f32 = 4.75;
const RING_INNER: f32 = 2.75;
const CORE: f32 = 1.6;
const HANDLE_FROM: (f32, f32) = (9.5, 9.5);
const HANDLE_TO: (f32, f32) = (14.0, 14.0);
const HANDLE_HALF_WIDTH: f32 = std::f32::consts::SQRT_2;

/// The icon at `size` by `size` pixels, RGBA, 8 bits per channel, not premultiplied, rows from
/// the top.
pub fn rgba(size: u32, layout: Layout) -> Vec<u8> {
    let px = size as f32;
    let margin = match layout {
        Layout::MacOs => px * 100.0 / 1024.0,
        Layout::Full => 0.0,
    };
    let body = px - 2.0 * margin;
    // macOS 26 shrinks an icon that leaves its rounded shape onto a grey one, so there it keeps
    // the rounding; everywhere else the square is the icon.
    let corner = match layout {
        Layout::MacOs => body * 0.225,
        Layout::Full => 0.0,
    };
    let unit = body / 16.0;
    let at = |(u, v): (f32, f32)| (margin + u * unit, margin + v * unit);
    let lens = at(LENS);
    let handle_from = at(HANDLE_FROM);
    let handle_to = at(HANDLE_TO);

    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let p = (x as f32 + 0.5, y as f32 + 0.5);
            let body_d = rounded_square(p, px / 2.0, body / 2.0, corner);
            let ring_d =
                circle(p, lens, RING_OUTER * unit).max(-circle(p, lens, RING_INNER * unit));
            let handle_d = bar(p, handle_from, handle_to, HANDLE_HALF_WIDTH * unit);
            let core_d = circle(p, lens, CORE * unit);
            let mut rgb = BODY;
            mix(&mut rgb, INK, coverage(ring_d.min(handle_d)));
            mix(&mut rgb, LIVE, coverage(core_d));
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

/// A bar of half width `half` from `a` to `b`, its ends cut square.
fn bar(p: (f32, f32), a: (f32, f32), b: (f32, f32), half: f32) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = length(dx, dy);
    let (ux, uy) = (dx / len, dy / len);
    let (mx, my) = ((a.0 + b.0) / 2.0 - p.0, (a.1 + b.1) / 2.0 - p.1);
    let along = (mx * ux + my * uy).abs() - len / 2.0;
    let across = (mx * uy - my * ux).abs() - half;
    length(along.max(0.0), across.max(0.0)) + along.max(across).min(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(img: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * size + x) * 4) as usize;
        [img[i], img[i + 1], img[i + 2], img[i + 3]]
    }

    #[test]
    fn every_size_is_drawn_with_its_body_mass_and_core() {
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
                assert!(count(rgb(INK)) > 0, "{layout:?} {size}: ring and handle");
                assert!(count(rgb(LIVE)) > 0, "{layout:?} {size}: core");
                let corner = pixel(&img, size, 0, 0);
                match layout {
                    Layout::MacOs => assert_eq!(corner[3], 0, "macOS corners are transparent"),
                    Layout::Full => assert_eq!(corner, [0, 0, 0, 255], "the full icon is square"),
                }
            }
        }
    }

    /// The pixel under a point of the 16 unit grid, at 1024 on the full layout.
    fn grid_pixel(img: &[u8], (u, v): (f32, f32)) -> [u8; 3] {
        let unit = 1024.0 / 16.0;
        let x = (u * unit) as u32;
        let y = (v * unit) as u32;
        let p = pixel(img, 1024, x, y);
        [p[0], p[1], p[2]]
    }

    #[test]
    fn the_lens_reads_ring_void_core_from_its_centre_out() {
        let img = rgba(1024, Layout::Full);
        let rgb = |c: [f32; 3]| c.map(channel);
        let (cx, cy) = LENS;
        assert_eq!(grid_pixel(&img, (cx, cy)), rgb(LIVE), "core");
        assert_eq!(grid_pixel(&img, (cx + 2.2, cy)), rgb(BODY), "void");
        assert_eq!(grid_pixel(&img, (cx + 3.75, cy)), rgb(INK), "ring");
        assert_eq!(
            grid_pixel(&img, (cx + 5.5, cy)),
            rgb(BODY),
            "outside the ring"
        );
        assert_eq!(grid_pixel(&img, (13.0, 13.0)), rgb(INK), "handle");
    }

    #[test]
    fn no_gap_parts_the_handle_from_the_ring() {
        let img = rgba(1024, Layout::Full);
        let ink = INK.map(channel);
        // Along the handle's axis, from the inside of the ring to the handle's end.
        let (cx, cy) = LENS;
        let step = std::f32::consts::FRAC_1_SQRT_2;
        let mut r = RING_INNER + 0.25;
        while r < (HANDLE_TO.0 - cx) / step - 0.25 {
            let at = (cx + r * step, cy + r * step);
            assert_eq!(
                grid_pixel(&img, at),
                ink,
                "gap at {r:.2} units from the centre"
            );
            r += 0.05;
        }
    }

    #[test]
    fn the_macos_grid_leaves_its_margin_and_full_does_not() {
        let mac = rgba(1024, Layout::MacOs);
        let full = rgba(1024, Layout::Full);
        assert_eq!(pixel(&mac, 1024, 512, 90)[3], 0);
        assert_eq!(pixel(&mac, 1024, 512, 110)[3], 255);
        assert_eq!(pixel(&full, 1024, 512, 0)[3], 255);
    }
}
