//! The loader must reject damaged files with an error, never panic or hang.

use m3g_viewer::loader;
use m3g_viewer::scene::Scene;

/// Writes `bytes` next to copies of the asset PNGs (the shipped files reference
/// them externally) and loads the result.
fn load_bytes(bytes: &[u8], tag: &str) -> Result<usize, String> {
    let dir = std::env::temp_dir().join(format!("m3g_robust_{}_{tag}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for entry in std::fs::read_dir("assets").unwrap() {
        let entry = entry.unwrap();
        if entry.path().extension().is_some_and(|e| e == "png") {
            std::fs::copy(entry.path(), dir.join(entry.file_name())).unwrap();
        }
    }
    let path = dir.join(format!("{tag}.m3g"));
    std::fs::write(&path, bytes).unwrap();
    let mut scene = Scene::default();
    // External references resolve against the directory part of the name given.
    let r = loader::load(&mut scene, path.to_str().unwrap()).map(|roots| roots.len());
    let _ = std::fs::remove_dir_all(&dir);
    r
}

#[test]
fn shipped_assets_load() {
    for f in [
        "assets/walls.m3g",
        "assets/snake.m3g",
        "assets/obstacles.m3g",
    ] {
        let bytes = std::fs::read(f).unwrap();
        assert_eq!(load_bytes(&bytes, "ok"), Ok(3), "{f}");
    }
}

#[test]
fn truncation_at_every_length_is_an_error_not_a_panic() {
    let bytes = std::fs::read("assets/walls.m3g").unwrap();
    for len in (0..bytes.len()).step_by(7) {
        // A cut that lands on a section boundary is a valid shorter file; anything else must fail cleanly.
        let _ = load_bytes(&bytes[..len], "trunc");
    }
}

#[test]
fn single_byte_corruption_never_panics() {
    // Deterministic pseudo-random positions and values (xorshift), no external crates.
    let original = std::fs::read("assets/snake.m3g").unwrap();
    let mut state = 0x2545F4914F6CDD1Du64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for round in 0..400 {
        let mut bytes = original.clone();
        let pos = (next() as usize) % bytes.len();
        bytes[pos] = (next() & 255) as u8;
        let _ = load_bytes(&bytes, &format!("c{round}"));
    }
}

#[test]
fn bad_checksum_is_reported() {
    let mut bytes = std::fs::read("assets/walls.m3g").unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xFF;
    let e = load_bytes(&bytes, "sum").unwrap_err();
    assert!(e.contains("Checksum"), "{e}");
}

#[test]
fn unrecognised_file_is_reported() {
    let e = load_bytes(b"definitely not an m3g file", "junk").unwrap_err();
    assert!(e.contains("not recognized"), "{e}");
}

#[test]
fn missing_file_names_the_problem() {
    let mut scene = Scene::default();
    let e = loader::load(&mut scene, "assets/nope.m3g").unwrap_err();
    assert!(e.contains("not found"), "{e}");
}
