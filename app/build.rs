//! Turns assets/icon.txt (16x16 pixel art, perk tile with a floppy disk) into a multi-size .ico and embeds it with version info.

use std::io::Write;

include!("assets/icon_art.rs");

/// One 32-bit BMP icon image (BGRA, bottom-up, plus the 1-bit AND mask).
fn bmp(rows: &[Vec<[u8; 4]>], size: usize) -> Vec<u8> {
    let scale = size / rows.len();
    let mut out = Vec::new();
    for v in [40u32, size as u32, (size * 2) as u32] {
        out.extend(v.to_le_bytes());
    }
    out.extend(1u16.to_le_bytes());
    out.extend(32u16.to_le_bytes());
    out.extend([0u8; 24]);
    for y in (0..size).rev() {
        for x in 0..size {
            let [r, g, b, a] = rows[y / scale][x / scale];
            out.extend([b, g, r, a]);
        }
    }
    let mask_row = size.div_ceil(32) * 4;
    out.extend(vec![0u8; mask_row * size]);
    out
}

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.txt");
    println!("cargo:rerun-if-changed=assets/icon_art.rs");
    let art = std::fs::read_to_string("assets/icon.txt").unwrap();
    let rows = icon_pixels(&art);

    let sizes = [16usize, 32, 48, 64, 128, 256];
    let images: Vec<Vec<u8>> = sizes.iter().map(|&s| bmp(&rows, s)).collect();
    let mut ico = Vec::new();
    ico.extend([0u8, 0, 1, 0]);
    ico.extend((sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len();
    for (s, img) in sizes.iter().zip(&images) {
        let dim = if *s >= 256 { 0 } else { *s as u8 };
        ico.extend([dim, dim, 0, 0]);
        ico.extend(1u16.to_le_bytes());
        ico.extend(32u16.to_le_bytes());
        ico.extend((img.len() as u32).to_le_bytes());
        ico.extend((offset as u32).to_le_bytes());
        offset += img.len();
    }
    for img in &images {
        ico.extend(img);
    }
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("app.ico");
    std::fs::File::create(&out).unwrap().write_all(&ico).unwrap();
    // The installer uses the same icon.
    let _ = std::fs::create_dir_all("../installer");
    let _ = std::fs::write("../installer/app.ico", &ico);

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon(out.to_str().unwrap());
        res.set("FileDescription", "Noita Save Selector");
        res.set("ProductName", "Noita Save Selector");
        res.compile().unwrap();
    }
}
