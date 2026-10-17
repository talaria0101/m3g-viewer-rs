//! Compares scene state against `tests/golden_state.txt`, which the Java viewer
//! produced (`StateDump`, see tools/java-ref). Every value is compared as raw
//! float bits, including the sign of zero, so this pins the loader, the
//! transform maths and every interpolation mode to the original's arithmetic.

use m3g_viewer::camera;
use m3g_viewer::loader;
use m3g_viewer::scene::*;

const TIMES: [i32; 6] = [0, 16, 250, 777, 1600, 99999];

fn hex(v: f32) -> String {
    format!("{:x}", v.to_bits())
}

fn walk(scene: &Scene, id: Id, out: &mut String) {
    out.push_str(" |");
    for v in scene.composite_transform(id).m {
        out.push(' ');
        out.push_str(&hex(v));
    }
    let n = scene.node_base(id).expect("walk visits nodes");
    out.push_str(&format!(" a={} r={}", hex(n.alpha_factor), n.rendering));
    if let Kind::MorphingMesh(m) = scene.kind(id) {
        for w in &m.weights {
            out.push_str(&format!(" w={}", hex(*w)));
        }
        out.push_str(&format!(" bw={}", hex(m.base_weight)));
    }
    if let Some(g) = scene.group(id) {
        for &c in &g.children {
            walk(scene, c, out);
        }
    }
}

fn dump(path: &str) -> Vec<String> {
    let mut scene = Scene::default();
    let roots = loader::load(&mut scene, path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let (_, world) = loader::pick_root(&scene, &roots);
    let world = world.expect("a World");
    let base = path.rsplit('/').next().unwrap();
    let mut lines = Vec::new();
    let b = camera::bounds(&scene, world);
    lines.push(format!("{base} bounds {}", b.iter().map(|v| hex(*v)).collect::<Vec<_>>().join(" ")));
    for t in TIMES {
        scene.animate(world, t).unwrap_or_else(|e| panic!("{path} at {t}: {e}"));
        let mut s = format!("t={t}");
        walk(&scene, world, &mut s);
        lines.push(s);
    }
    lines
}

#[test]
fn state_matches_the_java_viewer_bit_for_bit() {
    let files = [
        "assets/walls.m3g",
        "assets/snake.m3g",
        "assets/obstacles.m3g",
        "tests/data/animations.m3g",
        "tests/data/skinned.m3g",
    ];
    let golden = std::fs::read_to_string("tests/golden_state.txt").expect("golden file");
    let expected: Vec<&str> = golden.lines().collect();
    let mut actual = Vec::new();
    for f in files {
        actual.extend(dump(f));
    }
    assert_eq!(actual.len(), expected.len(), "line count");
    for (i, (a, e)) in actual.iter().zip(expected.iter()).enumerate() {
        assert_eq!(a, e, "line {} differs", i + 1);
    }
}
