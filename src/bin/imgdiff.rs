//! Compares two directories (or two files) of PNG frames pixel by pixel.
//!
//! Usage: imgdiff A B [--tolerance N] [--diff-out DIR]
//!
//! For every frame present in both it reports how many pixels differ, the
//! largest per-channel difference, and how many pixels differ by more than the
//! tolerance. Exit status is 0 when every frame is within the tolerance
//! (default 0), 1 when some pixel exceeds it, 2 on usage or I/O errors.

use std::path::{Path, PathBuf};

fn read_png(path: &Path) -> Result<(usize, usize, Vec<u8>), String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut reader = png::Decoder::new(file)
        .read_info()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if info.bit_depth != png::BitDepth::Eight {
        return Err(format!("{}: only 8-bit PNGs are supported", path.display()));
    }
    let channels = info.color_type.samples();
    let mut rgb = Vec::with_capacity(info.width as usize * info.height as usize * 3);
    for px in buf[..info.buffer_size()].chunks(channels) {
        match info.color_type {
            png::ColorType::Rgb | png::ColorType::Rgba => rgb.extend_from_slice(&px[..3]),
            png::ColorType::Grayscale | png::ColorType::GrayscaleAlpha => {
                rgb.extend_from_slice(&[px[0]; 3])
            }
            png::ColorType::Indexed => {
                return Err(format!(
                    "{}: indexed PNGs are not supported",
                    path.display()
                ))
            }
        }
    }
    Ok((info.width as usize, info.height as usize, rgb))
}

fn write_diff(path: &Path, w: usize, h: usize, a: &[u8], b: &[u8]) {
    let mut out = Vec::with_capacity(a.len());
    for i in 0..a.len() / 3 {
        let d = (0..3)
            .map(|c| a[i * 3 + c].abs_diff(b[i * 3 + c]))
            .max()
            .unwrap_or(0);
        let v = if d == 0 { 0 } else { 255 };
        out.extend_from_slice(&[v, 0, 0]);
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let file = std::fs::File::create(path).expect("create diff image");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .expect("png header")
        .write_image_data(&out)
        .expect("png data");
}

fn pairs(a: &Path, b: &Path) -> Result<Vec<(String, PathBuf, PathBuf)>, String> {
    if a.is_file() && b.is_file() {
        return Ok(vec![(
            a.file_name().unwrap().to_string_lossy().into_owned(),
            a.to_path_buf(),
            b.to_path_buf(),
        )]);
    }
    let mut names: Vec<String> = std::fs::read_dir(a)
        .map_err(|e| format!("{}: {e}", a.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".png"))
        .collect();
    names.sort();
    let mut out = Vec::new();
    for n in names {
        let pb = b.join(&n);
        if !pb.is_file() {
            return Err(format!("{} has no counterpart in {}", n, b.display()));
        }
        out.push((n.clone(), a.join(&n), pb));
    }
    Ok(out)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut positional = Vec::new();
    let mut tolerance = 0u8;
    let mut diff_out: Option<PathBuf> = None;
    let mut quiet = false;
    let mut list_pixels = 0usize;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--tolerance" => {
                i += 1;
                tolerance = args.get(i).and_then(|v| v.parse().ok()).unwrap_or_else(|| {
                    eprintln!("--tolerance needs a number 0..255");
                    std::process::exit(2)
                });
            }
            "--diff-out" => {
                i += 1;
                diff_out = args.get(i).map(PathBuf::from);
            }
            "--quiet" => quiet = true,
            "--list" => {
                i += 1;
                list_pixels = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(10);
            }
            a => positional.push(a.to_string()),
        }
        i += 1;
    }
    if positional.len() != 2 {
        eprintln!("usage: imgdiff A B [--tolerance N] [--diff-out DIR] [--quiet] [--list N]");
        std::process::exit(2);
    }
    let list = pairs(Path::new(&positional[0]), Path::new(&positional[1])).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2)
    });
    if list.is_empty() {
        eprintln!("no PNG frames found");
        std::process::exit(2);
    }

    let mut frames_identical = 0;
    let mut frames_over = 0;
    let mut worst = 0u8;
    let mut worst_pixels = 0usize;
    for (name, pa, pb) in &list {
        let (wa, ha, a) = read_png(pa).unwrap_or_else(|e| {
            eprintln!("{e}");
            std::process::exit(2)
        });
        let (wb, hb, b) = read_png(pb).unwrap_or_else(|e| {
            eprintln!("{e}");
            std::process::exit(2)
        });
        if (wa, ha) != (wb, hb) {
            eprintln!("{name}: size differs: {wa}x{ha} vs {wb}x{hb}");
            std::process::exit(2);
        }
        let mut differing = 0usize;
        let mut over = 0usize;
        let mut max = 0u8;
        for i in 0..a.len() / 3 {
            let d = (0..3)
                .map(|c| a[i * 3 + c].abs_diff(b[i * 3 + c]))
                .max()
                .unwrap_or(0);
            if d > 0 {
                differing += 1;
                if differing <= list_pixels {
                    println!(
                        "  {name} px ({}, {}): {:?} vs {:?}",
                        i % wa,
                        i / wa,
                        &a[i * 3..i * 3 + 3],
                        &b[i * 3..i * 3 + 3]
                    );
                }
            }
            if d > tolerance {
                over += 1;
            }
            max = max.max(d);
        }
        if differing == 0 {
            frames_identical += 1;
        }
        if over > 0 {
            frames_over += 1;
            if let Some(dir) = &diff_out {
                write_diff(&dir.join(name), wa, ha, &a, &b);
            }
        }
        worst = worst.max(max);
        worst_pixels = worst_pixels.max(differing);
        if !quiet && differing > 0 {
            println!("{name}: {differing} px differ (max channel delta {max}), {over} above tolerance {tolerance}");
        }
    }
    println!(
        "{} frame(s): {} identical, {} with pixels above tolerance {}; worst channel delta {}; most differing pixels in one frame {}",
        list.len(),
        frames_identical,
        frames_over,
        tolerance,
        worst,
        worst_pixels
    );
    std::process::exit(if frames_over > 0 { 1 } else { 0 });
}
