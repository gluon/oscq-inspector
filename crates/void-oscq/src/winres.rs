//! The Windows executable's icon as a compiled resource file, `.res`, which the MSVC linker takes
//! as an input: one `RT_ICON` per size and the `RT_GROUP_ICON` 1 that lists them, the layout `rc`
//! gives `1 ICON "app.ico"`. Each image is a 32 bit bitmap with alpha, as in an `.ico` file.

const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
/// `MOVEABLE | DISCARDABLE` for the images, plus `PURE` for the group, as `rc` writes them.
const ICON_FLAGS: u16 = 0x1010;
const GROUP_FLAGS: u16 = 0x1030;
/// English, United States: the language `rc` uses when the script names none.
const LANGUAGE: u16 = 0x0409;

/// The sizes Windows picks from, for Explorer, the taskbar and the title bar at every scale.
pub const SIZES: [u32; 6] = [16, 24, 32, 48, 64, 256];

/// `images` are square, `(size, rgba)`, as [`crate::icon::rgba`] draws them, smallest first.
pub fn icon_resource(images: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    // A .res file opens with an empty entry, which marks it as 32 bit.
    entry(&mut out, 0, 0, 0, 0, &[]);
    let bitmaps: Vec<Vec<u8>> = images.iter().map(|(s, px)| bitmap(*s, px)).collect();
    let mut group = Vec::new();
    push16(&mut group, 0);
    push16(&mut group, 1);
    push16(&mut group, images.len() as u16);
    for (i, ((size, _), bmp)) in images.iter().zip(&bitmaps).enumerate() {
        let id = i as u16 + 1;
        entry(&mut out, RT_ICON, id, ICON_FLAGS, LANGUAGE, bmp);
        directory_entry(&mut group, *size, bmp.len());
        push16(&mut group, id);
    }
    entry(&mut out, RT_GROUP_ICON, 1, GROUP_FLAGS, LANGUAGE, &group);
    out
}

/// The same images as an `.ico` file.
pub fn ico(images: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let bitmaps: Vec<Vec<u8>> = images.iter().map(|(s, px)| bitmap(*s, px)).collect();
    let mut out = Vec::new();
    push16(&mut out, 0);
    push16(&mut out, 1);
    push16(&mut out, images.len() as u16);
    let mut offset = 6 + 16 * images.len();
    for ((size, _), bmp) in images.iter().zip(&bitmaps) {
        directory_entry(&mut out, *size, bmp.len());
        push32(&mut out, offset as u32);
        offset += bmp.len();
    }
    for bmp in bitmaps {
        out.extend(bmp);
    }
    out
}

/// Width, height, colours, reserved, planes, bits, bytes: the part common to both directories.
fn directory_entry(out: &mut Vec<u8>, size: u32, len: usize) {
    // 256 is written 0.
    out.push(size as u8);
    out.push(size as u8);
    out.push(0);
    out.push(0);
    push16(out, 1);
    push16(out, 32);
    push32(out, len as u32);
}

/// A `BITMAPINFOHEADER` at twice the height, the BGRA rows from the bottom, then the 1 bit mask,
/// set where the image is transparent.
fn bitmap(size: u32, rgba: &[u8]) -> Vec<u8> {
    let mask_row = size.div_ceil(32) * 4;
    let mut out = Vec::with_capacity((40 + size * size * 4 + mask_row * size) as usize);
    push32(&mut out, 40);
    push32(&mut out, size);
    push32(&mut out, size * 2);
    push16(&mut out, 1);
    push16(&mut out, 32);
    for _ in 0..6 {
        push32(&mut out, 0);
    }
    let row = |y: u32| &rgba[(y * size * 4) as usize..((y + 1) * size * 4) as usize];
    for y in (0..size).rev() {
        for p in row(y).chunks(4) {
            out.extend([p[2], p[1], p[0], p[3]]);
        }
    }
    for y in (0..size).rev() {
        let mut bits = vec![0u8; mask_row as usize];
        for (x, p) in row(y).chunks(4).enumerate() {
            if p[3] == 0 {
                bits[x / 8] |= 0x80 >> (x % 8);
            }
        }
        out.extend(bits);
    }
    out
}

/// One resource: its header, with the type and name as numbers, then the data padded to 4 bytes.
fn entry(out: &mut Vec<u8>, kind: u16, name: u16, flags: u16, language: u16, data: &[u8]) {
    push32(out, data.len() as u32);
    push32(out, 32);
    push16(out, 0xFFFF);
    push16(out, kind);
    push16(out, 0xFFFF);
    push16(out, name);
    push32(out, 0);
    push16(out, flags);
    push16(out, language);
    push32(out, 0);
    push32(out, 0);
    out.extend(data);
    out.resize(out.len().next_multiple_of(4), 0);
}

fn push16(out: &mut Vec<u8>, v: u16) {
    out.extend(v.to_le_bytes());
}

fn push32(out: &mut Vec<u8>, v: u32) {
    out.extend(v.to_le_bytes());
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn u16_at(b: &[u8], i: usize) -> u16 {
        u16::from_le_bytes(b[i..i + 2].try_into().unwrap())
    }

    fn u32_at(b: &[u8], i: usize) -> u32 {
        u32::from_le_bytes(b[i..i + 4].try_into().unwrap())
    }

    fn images() -> Vec<(u32, Vec<u8>)> {
        SIZES
            .map(|s| (s, crate::icon::rgba(s, crate::icon::Layout::Full)))
            .to_vec()
    }

    /// Walks the resources: the empty entry, then one image per size, then the group naming
    /// them by id, each with the size of its image.
    #[test]
    fn the_resource_file_holds_each_image_and_the_group_listing_them() {
        let res = icon_resource(&images());
        let mut entries = Vec::new();
        let mut i = 0;
        while i < res.len() {
            let len = u32_at(&res, i) as usize;
            assert_eq!(u32_at(&res, i + 4), 32);
            let kind = u16_at(&res, i + 10);
            let name = u16_at(&res, i + 14);
            entries.push((kind, name, res[i + 32..i + 32 + len].to_vec()));
            i = (i + 32 + len).next_multiple_of(4);
        }
        assert_eq!(i, res.len());
        let kinds: Vec<(u16, u16)> = entries.iter().map(|e| (e.0, e.1)).collect();
        let mut want = vec![(0, 0)];
        want.extend((1..=SIZES.len() as u16).map(|id| (RT_ICON, id)));
        want.push((RT_GROUP_ICON, 1));
        assert_eq!(kinds, want);
        let group = &entries[SIZES.len() + 1].2;
        assert_eq!(
            (u16_at(group, 2), u16_at(group, 4)),
            (1, SIZES.len() as u16)
        );
        for (n, size) in SIZES.into_iter().enumerate() {
            let image = &entries[n + 1].2;
            let e = 6 + 14 * n;
            assert_eq!(group[e], size as u8, "256 is written 0");
            assert_eq!(u32_at(group, e + 8) as usize, image.len());
            assert_eq!(u16_at(group, e + 12), n as u16 + 1);
            assert_eq!(u32_at(image, 0), 40, "a BITMAPINFOHEADER");
            assert_eq!(u32_at(image, 4), size);
        }
    }

    /// The resource file is the one the resource compiler makes of the same icon: `llvm-rc`
    /// compiling `1 ICON "app.ico"`. Needs `llvm-rc` on the PATH, fails without it:
    /// cargo test -p void-oscq -- --ignored same_resource_as_llvm_rc
    #[test]
    #[ignore = "needs llvm-rc"]
    fn same_resource_as_llvm_rc() {
        let dir = std::env::temp_dir().join(format!("void-oscq-rc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("app.ico"), ico(&images())).unwrap();
        std::fs::write(dir.join("app.rc"), "1 ICON \"app.ico\"\n").unwrap();
        let status = std::process::Command::new("llvm-rc")
            .current_dir(&dir)
            .args(["/fo", "app.res", "app.rc"])
            .status()
            .expect("llvm-rc on the PATH");
        assert!(status.success());
        let reference = std::fs::read(dir.join("app.res")).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        let ours = icon_resource(&images());
        assert_eq!(reference.len(), ours.len());
        assert!(reference == ours, "same bytes as llvm-rc");
    }

    #[test]
    fn a_bitmap_is_bottom_up_bgra_with_its_mask() {
        let size = 16;
        let rgba = crate::icon::rgba(size, crate::icon::Layout::Full);
        let bmp = bitmap(size, &rgba);
        assert_eq!(bmp.len(), 40 + 16 * 16 * 4 + 4 * 16);
        assert_eq!(u32_at(&bmp, 8), 32, "height doubled for the mask");
        // The bottom row comes first: its middle pixel, swapped to BGRA.
        let src = ((15 * 16 + 8) * 4) as usize;
        let dst = 40 + 8 * 4;
        assert_eq!(
            bmp[dst..dst + 4],
            [rgba[src + 2], rgba[src + 1], rgba[src], rgba[src + 3]]
        );
        // The top left corner is transparent: its mask bit is set, in the last mask row.
        let last_mask_row = 40 + 16 * 16 * 4 + 4 * 15;
        assert_eq!(bmp[last_mask_row] & 0x80, 0x80);
    }
}
