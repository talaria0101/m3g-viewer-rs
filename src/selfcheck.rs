//! `--selfcheck`: loader and animation checks over `./assets`.

use std::collections::BTreeSet;

use m3g_viewer::loader;
use m3g_viewer::scene::*;

const ASSETS: [&str; 3] = ["snake.m3g", "walls.m3g", "obstacles.m3g"];

struct Loaded {
    scene: Scene,
    roots: Vec<Id>,
    world: Option<Id>,
}

fn load(name: &str, failures: &mut u32) -> Option<Loaded> {
    let mut scene = Scene::default();
    let path = format!("assets/{name}");
    let roots = match loader::load(&mut scene, &path) {
        Ok(r) => r,
        Err(e) => {
            fail(failures, &format!("{name}: load failed: {e}"));
            return None;
        }
    };
    if roots.is_empty() {
        fail(failures, &format!("{name}: loader returned no root objects"));
        return None;
    }
    let (_, world) = loader::pick_root(&scene, &roots);
    Some(Loaded { scene, roots, world })
}

fn fail(failures: &mut u32, message: &str) {
    *failures += 1;
    eprintln!("FAIL {message}");
}

/// Everything animation can move on a node: its composite matrix and morph weights.
fn state_of(scene: &Scene, id: Id) -> Vec<f32> {
    let mut v = scene.composite_transform(id).m.to_vec();
    if let Kind::MorphingMesh(m) = scene.kind(id) {
        v.extend_from_slice(&m.weights);
    }
    v
}

fn snapshot(scene: &Scene) -> Vec<(Id, Vec<f32>)> {
    (0..scene.objs.len()).filter(|&i| scene.is_node(i)).map(|i| (i, state_of(scene, i))).collect()
}

/// Returns the process exit code.
pub fn run() -> i32 {
    let mut failures = 0u32;

    for name in ASSETS {
        let Some(l) = load(name, &mut failures) else { continue };
        let kinds: BTreeSet<&str> = l.scene.objs.iter().map(|o| o.kind.name()).collect();
        let count = |f: fn(&Kind) -> bool| l.scene.objs.iter().filter(|o| f(&o.kind)).count();
        let textures = count(|k| matches!(k, Kind::Texture2D(_)));
        let images = count(|k| matches!(k, Kind::Image2D(_)));
        let meshes = count(|k| matches!(k, Kind::Mesh(_) | Kind::MorphingMesh(_)));
        let nodes = (0..l.scene.objs.len()).filter(|&i| l.scene.is_node(i)).count();
        println!(
            "{name}: roots={} objects={} nodes={nodes} meshes={meshes} textures={textures} images={images} kinds={kinds:?}",
            l.roots.len(),
            l.scene.objs.len()
        );
        if l.world.is_none() {
            fail(&mut failures, &format!("{name}: expected a World among the roots"));
        }
        if meshes == 0 {
            fail(&mut failures, &format!("{name}: expected at least one Mesh"));
        }
        if textures == 0 {
            fail(&mut failures, &format!("{name}: expected at least one Texture2D"));
        }
        if images == 0 {
            fail(&mut failures, &format!("{name}: expected at least one Image2D"));
        }
    }

    for name in ASSETS {
        let Some(mut l) = load(name, &mut failures) else { continue };
        let Some(world) = l.world else { continue };
        if let Err(e) = l.scene.animate(world, 0) {
            fail(&mut failures, &format!("{name}: animate(0) failed: {e}"));
            continue;
        }
        let at_zero = snapshot(&l.scene);
        if let Err(e) = l.scene.animate(world, 250) {
            fail(&mut failures, &format!("{name}: animate(250) failed: {e}"));
            continue;
        }
        let at_quarter = snapshot(&l.scene);
        let mut worst = 0.0f32;
        let mut what = "none";
        for ((id, a), (_, b)) in at_zero.iter().zip(at_quarter.iter()) {
            let d = a.iter().zip(b.iter()).fold(0.0f32, |m, (x, y)| m.max((x - y).abs()));
            if d > worst {
                worst = d;
                what = l.scene.kind(*id).name();
            }
        }
        println!("{name}: largest animation delta 0ms->250ms = {worst} on {what}");
        if worst == 0.0 {
            fail(&mut failures, &format!("{name}: animation produced no change over 250ms"));
        }
    }

    let mut scene = Scene::default();
    match loader::load(&mut scene, "assets/definitely-not-here.m3g") {
        Ok(_) => fail(&mut failures, "missing file: expected an error"),
        Err(e) if e.contains("not found") => println!("missing file reported: {e}"),
        Err(e) => fail(&mut failures, &format!("missing file: message should name the problem, got: {e}")),
    }

    if failures > 0 {
        eprintln!("{failures} check(s) failed");
        1
    } else {
        println!("all checks passed");
        0
    }
}
