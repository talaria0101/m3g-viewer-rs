//! m3g-viewer: a viewer for M3G (JSR-184) models, built on macroquad.
//!
//! A port of the Java/LWJGL viewer. See README.md for how "the same render"
//! is defined and checked.

mod selfcheck;

use std::path::{Path, PathBuf};
use std::time::Instant;

use macroquad::prelude::*;

use m3g_viewer::camera::{self, OrbitCamera};
use m3g_viewer::loader;
use m3g_viewer::render::{CameraSource, Config, Renderer};
use m3g_viewer::scene::*;

/// Fixed world-time step used by batch mode so output is reproducible.
const STEP_MILLIS: i32 = 16;

struct Options {
    file: PathBuf,
    headless: bool,
    frames: u32,
    out: Option<PathBuf>,
    dump_dir: Option<PathBuf>,
    depth_out: Option<PathBuf>,
    quit_after: Option<u32>,
    scene_camera: bool,
    config: Config,
}

/// A loaded scene plus the nodes the viewer cares about.
struct Loaded {
    scene: Scene,
    root: Id,
    world: Option<Id>,
}

impl Loaded {
    fn load(file: &Path) -> std::result::Result<Loaded, String> {
        let mut scene = Scene::default();
        let name = file.to_string_lossy().into_owned();
        let roots =
            loader::load(&mut scene, &name).map_err(|e| format!("Could not load {name}: {e}"))?;
        if roots.is_empty() {
            return Err(format!("{name} contains no objects."));
        }
        let (root, world) = loader::pick_root(&scene, &roots);
        let root =
            root.ok_or_else(|| format!("{name} contains no Node and cannot be displayed."))?;
        Ok(Loaded { scene, root, world })
    }

    fn advance(&mut self, world_time: i32) -> Result<()> {
        if let Some(w) = self.world {
            if let Kind::World(wd) = self.scene.kind_mut(w) {
                wd.world_time = world_time;
            }
            self.scene.animate(w, world_time)?;
        }
        Ok(())
    }
}

fn usage() {
    println!(
        "m3g-viewer: view M3G (JSR-184) models\n\
         \n\
         \x20 m3g-viewer [options] model.m3g\n\
         \n\
         \x20 --headless                    render without interaction, then write a PNG\n\
         \x20 --scene-camera                place the camera where the .m3g file puts it\n\
         \x20 --frames N                    render N frames at a fixed 16ms step, then stop\n\
         \x20 --out FILE.png                write the last frame to FILE.png (default shot.png)\n\
         \x20 --dump-dir DIR                write every batch frame to DIR/frame_NNNN.png\n\
         \x20 --quit-after N               interactive mode: exit after N frames (for automated smoke tests)\n\
         \x20 --dump-depth FILE            write the last frame's depth buffer as raw little-endian f32 (bottom row first)\n\
         \x20 --tex-filter nearest|linear   override the texture filter the file asks for\n\
         \x20 --force-perspective-correction ignore per-node perspective correction (no effect on modern GL)\n\
         \x20 --disable-light-clamp         do not clamp vertex colours before interpolation\n\
         \x20 --selfcheck                   run the loader and animation checks (needs ./assets) and exit\n"
    );
}

fn fail(message: &str) -> ! {
    eprintln!("[m3g-viewer] ERROR {message}");
    std::process::exit(1);
}

fn next_value(args: &[String], i: usize, option: &str) -> String {
    match args.get(i) {
        Some(v) => v.clone(),
        None => fail(&format!("{option} requires a value")),
    }
}

fn parse_args(args: &[String]) -> Options {
    let mut file: Option<PathBuf> = None;
    let mut o = Options {
        file: PathBuf::new(),
        headless: false,
        frames: 0,
        out: None,
        dump_dir: None,
        depth_out: None,
        quit_after: None,
        scene_camera: false,
        config: Config {
            force_perspective_correction: false,
            disable_light_clamp: false,
            tex_filter: None,
        },
    };
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "--headless" => o.headless = true,
            "--scene-camera" => o.scene_camera = true,
            "--frames" => {
                i += 1;
                let v = next_value(args, i, "--frames");
                o.frames = v
                    .parse()
                    .unwrap_or_else(|_| fail(&format!("--frames needs a number, got {v}")));
            }
            "--out" => {
                i += 1;
                o.out = Some(PathBuf::from(next_value(args, i, "--out")));
            }
            "--dump-dir" => {
                i += 1;
                o.dump_dir = Some(PathBuf::from(next_value(args, i, "--dump-dir")));
            }
            "--quit-after" => {
                i += 1;
                let v = next_value(args, i, "--quit-after");
                o.quit_after =
                    Some(v.parse().unwrap_or_else(|_| {
                        fail(&format!("--quit-after needs a number, got {v}"))
                    }));
            }
            "--dump-depth" => {
                i += 1;
                o.depth_out = Some(PathBuf::from(next_value(args, i, "--dump-depth")));
            }
            "--force-perspective-correction" => o.config.force_perspective_correction = true,
            "--disable-light-clamp" => o.config.disable_light_clamp = true,
            "--tex-filter" => {
                i += 1;
                o.config.tex_filter = match next_value(args, i, "--tex-filter").as_str() {
                    "nearest" => Some(FILTER_NEAREST),
                    "linear" => Some(FILTER_LINEAR),
                    v => fail(&format!(
                        "Unknown --tex-filter value: {v} (expected nearest or linear)"
                    )),
                };
            }
            _ if a.starts_with("--") => fail(&format!("Unknown option: {a}")),
            _ => {
                if let Some(prev) = &file {
                    fail(&format!(
                        "More than one .m3g file given: {} and {a}",
                        prev.display()
                    ));
                }
                file = Some(PathBuf::from(a));
            }
        }
        i += 1;
    }
    match file {
        Some(f) => o.file = f,
        None => {
            usage();
            std::process::exit(2);
        }
    }
    o
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        usage();
        std::process::exit(2);
    }
    if args[0] == "--help" || args[0] == "-h" {
        usage();
        return;
    }
    if args[0] == "--selfcheck" {
        std::process::exit(selfcheck::run());
    }

    let mut opts = parse_args(&args);
    let loaded = match Loaded::load(&opts.file) {
        Ok(l) => l,
        Err(e) => fail(&e),
    };

    let batch = opts.headless || opts.frames > 0;
    if batch {
        if opts.frames == 0 {
            opts.frames = 1;
        }
        if opts.out.is_none() {
            opts.out = Some(PathBuf::from("shot.png"));
        }
    }

    let title = if batch {
        "m3g-viewer".to_string()
    } else {
        opts.file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let conf = macroquad::miniquad::conf::Conf {
        window_title: title,
        window_width: if batch { 800 } else { 1024 },
        window_height: if batch { 600 } else { 768 },
        window_resizable: true,
        high_dpi: false,
        ..Default::default()
    };
    macroquad::Window::from_config(conf, run(opts, loaded, batch));
}

fn write_png(rgba: &[u8], width: usize, height: usize, flip: bool, out: &Path) {
    if let Some(dir) = out.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)
                .unwrap_or_else(|e| fail(&format!("Could not create {}: {e}", dir.display())));
        }
    }
    let mut rgb = Vec::with_capacity(width * height * 3);
    for row in 0..height {
        let src_row = if flip { height - 1 - row } else { row };
        for x in 0..width {
            let i = (src_row * width + x) * 4;
            rgb.extend_from_slice(&rgba[i..i + 3]);
        }
    }
    let file = std::fs::File::create(out)
        .unwrap_or_else(|e| fail(&format!("Could not write {}: {e}", out.display())));
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), width as u32, height as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc
        .write_header()
        .unwrap_or_else(|e| fail(&format!("PNG error: {e}")));
    w.write_image_data(&rgb)
        .unwrap_or_else(|e| fail(&format!("PNG error: {e}")));
}

/// Reads the depth buffer of the current frame and writes it as raw little-endian f32.
fn write_depth(path: &Path, width: usize, height: usize) {
    let mut depth = vec![0f32; width * height];
    // SAFETY: the buffer holds exactly width * height floats, which is what
    // glReadPixels(GL_DEPTH_COMPONENT, GL_FLOAT) writes for this rectangle.
    unsafe {
        macroquad::miniquad::gl::glReadPixels(
            0,
            0,
            width as i32,
            height as i32,
            0x1902,
            0x1406,
            depth.as_mut_ptr() as *mut _,
        );
    }
    let bytes: Vec<u8> = depth.iter().flat_map(|d| d.to_le_bytes()).collect();
    std::fs::write(path, bytes)
        .unwrap_or_else(|e| fail(&format!("Could not write {}: {e}", path.display())));
}

/// Perspective camera for scenes whose camera is not used, matching the orbit
/// camera's field of view and depth range.
fn synth_camera(aspect: f32) -> CameraData {
    let mut c = CameraData::new(NodeBase::default());
    c.set_perspective(60.0, aspect, 0.05, 1000.0)
        .expect("fixed arguments are valid");
    c
}

fn set_frame_camera(
    renderer: &mut Renderer,
    loaded: &Loaded,
    scene_camera: bool,
    orbit: &OrbitCamera,
    w: u32,
    h: u32,
) {
    let authored = if scene_camera {
        camera::scene_camera_transform(&loaded.scene, loaded.world)
    } else {
        None
    };
    match authored {
        Some(t) => {
            let cam = match loaded.world.map(|w| loaded.scene.kind(w)) {
                Some(Kind::World(wd)) => wd
                    .active_camera
                    .expect("a transform implies an active camera"),
                _ => unreachable!("a transform implies a world"),
            };
            renderer.set_camera(CameraSource::Scene(cam), Some(t));
        }
        None => {
            renderer.set_camera(
                CameraSource::Synth(synth_camera(w as f32 / h as f32)),
                Some(orbit.view_matrix()),
            );
        }
    }
}

fn draw_frame(renderer: &mut Renderer, loaded: &Loaded) {
    // SAFETY: macroquad hands out its GL context through this accessor; it is
    // used only from this task, between frames.
    let gl = unsafe { get_internal_gl() };
    let ctx = gl.quad_context;
    renderer.collect_world_lights(&loaded.scene, loaded.world);
    renderer.clear_background(ctx, &loaded.scene, loaded.world);
    let result = renderer.render(ctx, &loaded.scene, loaded.root);
    renderer.finish_frame(ctx);
    if let Err(e) = result {
        fail(&e);
    }
}

async fn run(opts: Options, mut loaded: Loaded, batch: bool) {
    let mut renderer = {
        // SAFETY: see draw_frame.
        let gl = unsafe { get_internal_gl() };
        let info = gl.quad_context.info();
        // SAFETY: glGetString on the current context returns a NUL-terminated string or null.
        let renderer_name = unsafe {
            let p = macroquad::miniquad::gl::glGetString(0x1F01) as *const std::ffi::c_char;
            if p.is_null() {
                String::from("?")
            } else {
                std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
            }
        };
        eprintln!(
            "[m3g-viewer] {:?} | GL {} | {renderer_name}",
            info.backend, info.gl_version_string
        );
        // SAFETY: glGetIntegerv writes one GLint for each of these queries.
        let bits = |what: u32| unsafe {
            let mut v = 0i32;
            macroquad::miniquad::gl::glGetIntegerv(what, &mut v);
            v
        };
        eprintln!(
            "[m3g-viewer] framebuffer bits: depth {} stencil {} red {} alpha {}",
            bits(0x0D56),
            bits(0x0D57),
            bits(0x0D52),
            bits(0x0D55)
        );
        Renderer::new(gl.quad_context, opts.config)
    };

    let mut orbit = OrbitCamera::new();
    orbit.frame(&camera::bounds(&loaded.scene, loaded.root));

    if batch {
        let out = opts.out.expect("batch mode always has an output path");
        run_batch(
            &out,
            &opts.dump_dir,
            &opts.depth_out,
            opts.frames,
            opts.scene_camera,
            &mut renderer,
            &mut loaded,
            &orbit,
        )
        .await;
    } else {
        run_interactive(
            opts.scene_camera,
            opts.quit_after,
            &mut renderer,
            &mut loaded,
            orbit,
        )
        .await;
    }
}

async fn run_batch(
    out: &Path,
    dump_dir: &Option<PathBuf>,
    depth_out: &Option<PathBuf>,
    frames: u32,
    scene_camera: bool,
    renderer: &mut Renderer,
    loaded: &mut Loaded,
    orbit: &OrbitCamera,
) {
    let (w, h) = macroquad::miniquad::window::screen_size();
    set_frame_camera(renderer, loaded, scene_camera, orbit, w as u32, h as u32);

    for frame in 0..frames {
        if let Err(e) = loaded.advance(frame as i32 * STEP_MILLIS) {
            fail(&e);
        }
        draw_frame(renderer, loaded);

        // Read before the swap: the back buffer is undefined afterwards.
        let last = frame == frames - 1;
        if last {
            if let Some(path) = depth_out.as_deref() {
                write_depth(path, w as usize, h as usize);
            }
        }
        if dump_dir.is_some() || last {
            let image = get_screen_data();
            let (iw, ih) = (image.width as usize, image.height as usize);
            // get_screen_data returns rows bottom-up, like glReadPixels.
            if let Some(dir) = dump_dir {
                write_png(
                    &image.bytes,
                    iw,
                    ih,
                    true,
                    &dir.join(format!("frame_{frame:04}.png")),
                );
            }
            if last {
                write_png(&image.bytes, iw, ih, true, out);
                let shown = std::fs::canonicalize(out).unwrap_or_else(|_| out.to_path_buf());
                eprintln!(
                    "[m3g-viewer] wrote {} ({iw}x{ih}, {frames} frame(s))",
                    shown.display()
                );
            }
        }
        next_frame().await;
    }
}

async fn run_interactive(
    scene_camera: bool,
    quit_after: Option<u32>,
    renderer: &mut Renderer,
    loaded: &mut Loaded,
    mut orbit: OrbitCamera,
) {
    let start = Instant::now();
    let mut frames_drawn = 0u32;
    let mut dragging = false;
    let mut last = (0.0f32, 0.0f32);

    loop {
        // Input, in the same units the original used.
        let (mx, my) = mouse_position();
        if is_mouse_button_pressed(MouseButton::Left) {
            dragging = true;
            last = (mx, my);
        }
        if !is_mouse_button_down(MouseButton::Left) {
            dragging = false;
        }
        if dragging && (mx != last.0 || my != last.1) {
            orbit.orbit((mx - last.0) * 0.01, (my - last.1) * 0.01);
            last = (mx, my);
        }
        let (_, wheel) = mouse_wheel();
        if wheel != 0.0 {
            orbit.zoom(wheel * 0.15);
        }
        if is_key_pressed(KeyCode::Escape) {
            return;
        }
        if is_key_pressed(KeyCode::W) {
            orbit.pan(0.0, 1.0);
        }
        if is_key_pressed(KeyCode::S) {
            orbit.pan(0.0, -1.0);
        }
        if is_key_pressed(KeyCode::A) {
            orbit.pan(-1.0, 0.0);
        }
        if is_key_pressed(KeyCode::D) {
            orbit.pan(1.0, 0.0);
        }
        if is_key_pressed(KeyCode::R) {
            orbit.frame(&camera::bounds(&loaded.scene, loaded.root));
        }

        let (w, h) = macroquad::miniquad::window::screen_size();
        let (w, h) = ((w as u32).max(1), (h as u32).max(1));
        let elapsed = start.elapsed().as_millis() as i32;
        if let Err(e) = loaded.advance(elapsed) {
            fail(&e);
        }
        set_frame_camera(renderer, loaded, scene_camera, &orbit, w, h);
        draw_frame(renderer, loaded);
        frames_drawn += 1;
        if quit_after.is_some_and(|n| frames_drawn >= n) {
            return;
        }
        next_frame().await;
    }
}
