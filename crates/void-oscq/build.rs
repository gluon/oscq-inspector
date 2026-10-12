//! On Windows with MSVC, links the application icon into the executable, so that Explorer shows
//! it. The icon is drawn by `src/icon.rs`, the same code as the window icon; nothing is stored.

#[allow(dead_code)]
#[path = "src/icon.rs"]
mod icon;
#[allow(dead_code)]
#[path = "src/winres.rs"]
mod winres;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/icon.rs");
    println!("cargo:rerun-if-changed=src/winres.rs");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if os != "windows" || env != "msvc" {
        return;
    }
    let images: Vec<(u32, Vec<u8>)> = winres::SIZES
        .iter()
        .map(|&s| (s, icon::rgba(s, icon::Layout::Full)))
        .collect();
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    let res = out.join("icon.res");
    std::fs::write(&res, winres::icon_resource(&images)).expect("writing the icon resource");
    // The MSVC linker takes a compiled resource file as an input, as it takes an object file.
    println!("cargo:rustc-link-arg-bins={}", res.display());
}
