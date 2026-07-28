//! One-off: rasterize `kask/assets/zk-icon.svg` to the icon assets zed
//! expects for the desktop / window / about icons, the Windows `.ico`, and
//! the macOS document-type `.icns`.
//!
//! Outputs (relative to repo root):
//!   crates/zed/resources/app-icon-dev.png         (512x512)
//!   crates/zed/resources/app-icon-dev@2x.png      (1024x1024)
//!   crates/zed/resources/windows/app-icon-dev.ico (multi-size)
//!   crates/zed/resources/Document.icns            (macOS document icon)

use std::fs;
use std::path::{Path, PathBuf};

use resvg::tiny_skia::{Pixmap, PixmapMut, Transform};
use usvg::Tree;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("canonicalize repo root")
}

/// Rasterize the SVG to a square `size`×`size` RGBA pixmap, scaling the
/// tree's natural size to fit.
fn rasterize(svg_path: &Path, size: u32) -> Pixmap {
    let svg_bytes = fs::read(svg_path).unwrap_or_else(|e| panic!("read {svg_path:?}: {e}"));
    let options = usvg::Options::default();
    let tree =
        Tree::from_data(&svg_bytes, &options).unwrap_or_else(|e| panic!("parse {svg_path:?}: {e}"));

    let natural = tree.size();
    let natural_w = natural.width() as f32;
    let natural_h = natural.height() as f32;
    let scale_x = size as f32 / natural_w;
    let scale_y = size as f32 / natural_h;
    let scale = scale_x.min(scale_y);

    let mut pixmap =
        Pixmap::new(size, size).unwrap_or_else(|| panic!("alloc pixmap {size}x{size}"));
    {
        let mut pixmap_mut =
            PixmapMut::from_bytes(pixmap.data_mut(), size, size).expect("borrow pixmap for render");
        // Center the scaled tree inside the square pixmap.
        let dx = (size as f32 - natural_w * scale) / 2.0;
        let dy = (size as f32 - natural_h * scale) / 2.0;
        let transform = Transform::from_scale(scale, scale).post_translate(dx, dy);
        resvg::render(&tree, transform, &mut pixmap_mut);
    }
    pixmap
}

fn write_png(pixmap: &Pixmap, out: &Path) {
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent).expect("create parent dir");
    }
    let png_bytes = pixmap
        .encode_png()
        .unwrap_or_else(|e| panic!("encode png: {e}"));
    fs::write(out, &png_bytes).unwrap_or_else(|e| panic!("write {out:?}: {e}"));
    println!(
        "wrote {} ({}x{})",
        out.display(),
        pixmap.width(),
        pixmap.height()
    );
}

/// Build a multi-size `.ico` from the source SVG. ICO format: ICONDIR header
/// followed by ICONDIRENTRY per image, then image data (PNG-encoded —
/// supported by Windows Vista+ and all modern tooling).
fn write_ico(svg_path: &Path, sizes: &[u32], out: &Path) {
    let mut entries: Vec<(u32, Vec<u8>)> = Vec::new();
    for &size in sizes {
        let pixmap = rasterize(svg_path, size);
        let png = pixmap.encode_png().expect("encode png for ico");
        entries.push((size, png));
    }

    let mut buf: Vec<u8> = Vec::new();
    // ICONDIR
    buf.extend_from_slice(&[0u8, 0]); // reserved
    buf.extend_from_slice(&1u16.to_le_bytes()); // type = 1 (icon)
    buf.extend_from_slice(&(entries.len() as u16).to_le_bytes());

    // ICONDIRENTRY (16 bytes each)
    let mut data_offset = 6 + entries.len() * 16;
    for &(size, ref png) in &entries {
        let w = if size >= 256 { 0 } else { size as u8 };
        let h = w;
        buf.push(w);
        buf.push(h);
        buf.push(0); // palette
        buf.push(0); // reserved
        buf.extend_from_slice(&1u16.to_le_bytes()); // color planes
        buf.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        buf.extend_from_slice(&(png.len() as u32).to_le_bytes());
        buf.extend_from_slice(&(data_offset as u32).to_le_bytes());
        data_offset += png.len();
    }
    for (_, png) in &entries {
        buf.extend_from_slice(png);
    }

    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent).expect("create parent dir");
    }
    fs::write(out, &buf).unwrap_or_else(|e| panic!("write {out:?}: {e}"));
    println!("wrote {} ({} sizes)", out.display(), entries.len());
}

/// Build a macOS `.icns` from the source SVG. We emit the modern PNG-based
/// OSTypes so the icon renders correctly on macOS 10.14+ (the minimum zed
/// targets). Each entry is a PNG payload. The set covers 16..1024 px so
/// Finder, Dock, and Spotlight all resolve a crisp variant.
fn write_icns(svg_path: &Path, out: &Path) {
    // (ostype, pixel size). OSTypes per Apple's icns spec:
    //   ic07=128, ic08=256, ic09=512, ic10=1024,
    //   ic11=32@2x (64px), ic12=64@2x (128px),
    //   ic13=256@2x (512px), ic14=512@2x (1024px),
    //   icp4=16, icp5=32, icp6=64 (legacy small packed).
    let entries: &[(&[u8; 4], u32)] = &[
        (b"icp4", 16),
        (b"icp5", 32),
        (b"icp6", 64),
        (b"ic07", 128),
        (b"ic08", 256),
        (b"ic09", 512),
        (b"ic10", 1024),
        (b"ic11", 64),
        (b"ic12", 128),
        (b"ic13", 512),
        (b"ic14", 1024),
    ];

    let mut payload: Vec<u8> = Vec::new();
    payload.extend_from_slice(b"icns"); // magic
    // total file length (including the 8-byte header), filled after assembly.
    payload.extend_from_slice(&0u32.to_be_bytes());

    for &(ostype, size) in entries {
        let pixmap = rasterize(svg_path, size);
        let png = pixmap.encode_png().expect("encode png for icns");
        payload.extend_from_slice(ostype);
        // entry length includes the 8-byte ostype+length prefix.
        let entry_len = 8u32 + png.len() as u32;
        payload.extend_from_slice(&entry_len.to_be_bytes());
        payload.extend_from_slice(&png);
    }

    // Patch the total file length in the header.
    let total_len = payload.len() as u32;
    payload[4..8].copy_from_slice(&total_len.to_be_bytes());

    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent).expect("create parent dir");
    }
    fs::write(out, &payload).unwrap_or_else(|e| panic!("write {out:?}: {e}"));
    println!("wrote {} ({} entries)", out.display(), entries.len());
}

fn main() {
    let root = repo_root();
    let svg = root.join("kask").join("assets").join("zk-icon.svg");
    assert!(svg.exists(), "source SVG not found: {svg:?}");

    // All release channels share the same zk monogram — there is no
    // per-channel variant of the logo. We rasterize once per channel suffix
    // so every build (stable, dev, nightly, preview) ships the zk icon,
    // not just dev. Previously only dev was refreshed and the other channels
    // silently kept the upstream Zed "K" icon, which surfaced as "still
    // seeing the old icon" when a user built on a non-dev channel.
    let resources = root.join("crates").join("zed").join("resources");
    let windows = resources.join("windows");
    let channels: &[(&str, &str)] = &[
        ("stable", ""),
        ("dev", "-dev"),
        ("nightly", "-nightly"),
        ("preview", "-preview"),
    ];
    for (_name, suffix) in channels {
        let png = resources.join(format!("app-icon{suffix}.png"));
        let png_2x = resources.join(format!("app-icon{suffix}@2x.png"));
        let ico = windows.join(format!("app-icon{suffix}.ico"));
        write_png(&rasterize(&svg, 512), &png);
        write_png(&rasterize(&svg, 1024), &png_2x);
        write_ico(&svg, &[16, 32, 48, 64, 128, 256], &ico);
    }

    // macOS document-type icon. `bundle-mac` copies this into the .app bundle
    // as `Contents/Resources/Document.icns` (referenced by DocumentTypes.plist
    // via CFBundleTypeIconFile "Document"). cargo-bundle builds the app icon
    // itself from the bundle-{channel} PNGs above, so we only need to refresh
    // the document icon here.
    let document_icns = resources.join("Document.icns");
    write_icns(&svg, &document_icns);

    println!("done.");
}
