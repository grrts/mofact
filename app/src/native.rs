//! The SDL shell: desktop and phones (T03).

use crate::game::{self, Game};
use crate::ore::OreMap;
use crate::{autosave, bundled, surface};
use mofact_core::content::Content;
use mofact_input::{Pointer, Tool, MOUSE, MOUSE_PAN};
use mofact_render::{wgpu, Camera, Frame, Gpu, Look, Overlay, Renderer, Scene, Target, FLOOR};
use mofact_sim::Sim;
use mofact_ui::{Hud, View};
use sdl3::event::{Event, WindowEvent};
use sdl3::mouse::MouseButton;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

struct App {
    // Field order is drop order: the game's surface goes before the window it draws into.
    game: Game,
    window: sdl3::video::Window,
    /// The mouse button held, as a pointer id.
    mouse: u64,
    /// The platform has taken the window away, or it is minimised.
    background: bool,
}

impl App {
    fn new(
        video: &sdl3::VideoSubsystem,
        content: Content,
        sim: Sim,
        ore: OreMap,
        camera: Camera,
    ) -> Result<Self, String> {
        let window = video
            .window("Mofact", 1280, 720)
            .resizable()
            .high_pixel_density()
            .build()
            .map_err(|e| e.to_string())?;
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let surface = surface::create(&instance, &window)?;
        let gpu = Gpu::new(instance, &surface).map_err(|e| e.to_string())?;
        let (w, h) = window.size_in_pixels();
        let target = Target::new(&gpu, surface, w, h).map_err(|e| e.to_string())?;
        Ok(Self {
            game: Game::new(gpu, target, content, sim, ore, camera),
            window,
            mouse: MOUSE,
            background: false,
        })
    }

    /// The platform is taking the window away: let go of the surface, stop ticking.
    fn suspend(&mut self) {
        if !self.background {
            self.background = true;
            self.game.target = None;
            self.game.sim.set_paused(true);
        }
    }

    /// Back in front: a new surface, and the simulation carries on where it stopped.
    fn resume(&mut self) -> Result<(), String> {
        if self.background {
            let surface = surface::create(&self.game.gpu.instance, &self.window)?;
            let (w, h) = self.window.size_in_pixels();
            self.game.target =
                Some(Target::new(&self.game.gpu, surface, w, h).map_err(|e| e.to_string())?);
            self.background = false;
            self.game.sim.set_paused(false);
        }
        Ok(())
    }

    fn draw(&mut self) -> Frame {
        if self.background {
            return Frame::Skipped;
        }
        let size = self.window.size_in_pixels();
        let scale = self.window.display_scale();
        self.game.frame(size, scale)
    }

    /// An SDL event as a pointer event in screen pixels, if it is one.
    fn pointer(&mut self, event: &Event) -> Option<Pointer> {
        // Mouse positions are in window points; touches are 0 to 1.
        let (pw, ph) = self.window.size_in_pixels();
        let (ww, _) = self.window.size();
        let scale = pw as f32 / ww.max(1) as f32;
        let touch = |x: f32, y: f32| (x * pw as f32, y * ph as f32);
        let drag = |b: MouseButton| {
            matches!(
                b,
                MouseButton::Left | MouseButton::Middle | MouseButton::Right
            )
        };
        Some(match *event {
            Event::MouseButtonDown {
                mouse_btn, x, y, ..
            } if drag(mouse_btn) => {
                self.mouse = if mouse_btn == MouseButton::Left {
                    MOUSE
                } else {
                    MOUSE_PAN
                };
                Pointer::Down {
                    id: self.mouse,
                    x: x * scale,
                    y: y * scale,
                }
            }
            Event::MouseButtonUp { mouse_btn, .. } if drag(mouse_btn) => {
                Pointer::Up { id: self.mouse }
            }
            Event::MouseMotion { x, y, .. } => Pointer::Move {
                id: self.mouse,
                x: x * scale,
                y: y * scale,
            },
            Event::MouseWheel {
                y,
                mouse_x,
                mouse_y,
                ..
            } => Pointer::Wheel {
                steps: y,
                x: mouse_x * scale,
                y: mouse_y * scale,
            },
            Event::FingerDown {
                finger_id, x, y, ..
            } => {
                let (x, y) = touch(x, y);
                Pointer::Down {
                    id: finger_id,
                    x,
                    y,
                }
            }
            Event::FingerMotion {
                finger_id, x, y, ..
            } => {
                let (x, y) = touch(x, y);
                Pointer::Move {
                    id: finger_id,
                    x,
                    y,
                }
            }
            Event::FingerUp { finger_id, .. } | Event::FingerCanceled { finger_id, .. } => {
                Pointer::Up { id: finger_id }
            }
            _ => return None,
        })
    }
}

struct Args {
    frames: Option<u64>,
    screenshot: Option<String>,
    /// A scenario named on the command line starts fresh instead of loading the save.
    scenario: Option<String>,
    save: Option<String>,
    data: Option<String>,
    seed: u64,
}

fn args() -> Result<Args, String> {
    let mut args = Args {
        frames: None,
        screenshot: None,
        scenario: None,
        save: None,
        data: None,
        seed: 42,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--frames" => {
                let n = it.next().ok_or("--frames needs a number")?;
                args.frames = Some(n.parse().map_err(|_| format!("bad frame count `{n}`"))?);
            }
            "--data" => args.data = Some(it.next().ok_or("--data needs a folder")?),
            "--seed" => {
                let n = it.next().ok_or("--seed needs a number")?;
                args.seed = n.parse().map_err(|_| format!("bad seed `{n}`"))?;
            }
            "--scenario" => args.scenario = Some(it.next().ok_or("--scenario needs a file")?),
            "--save" => args.save = Some(it.next().ok_or("--save needs a file")?),
            "--screenshot" => {
                args.screenshot = Some(it.next().ok_or("--screenshot needs a folder")?)
            }
            other => {
                return Err(format!(
                    "unknown argument `{other}` (try --frames N, --screenshot DIR, --scenario FILE, --save FILE, --seed N, --data DIR)"
                ))
            }
        }
    }
    Ok(args)
}

/// Run the game with the process's arguments, as every platform's entry point does.
pub fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("mofact: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Run the game, reporting why it could not start or stopped early.
pub fn run() -> Result<(), String> {
    let args = args()?;
    let content = match &args.data {
        Some(dir) => mofact_content::load_dir(Path::new(dir)).map_err(|e| e.to_string())?,
        None => bundled::content()?,
    };
    let scenario = match &args.scenario {
        Some(file) => std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?,
        None => bundled::STARTER.to_owned(),
    };
    let scenario = mofact_headless::parse(&scenario)?;
    let save_path = match &args.save {
        Some(p) => PathBuf::from(p),
        None => sdl3::filesystem::get_pref_path("mofact", "mofact")
            .map(|d| d.join("autosave.mofact"))
            .unwrap_or_else(|_| PathBuf::from("autosave.mofact")),
    };
    let saved = match (&args.scenario, &args.screenshot) {
        (None, None) => autosave::load(&save_path, &content),
        _ => None,
    };
    let sim = match saved {
        Some(sim) => {
            println!("mofact: continuing {}", save_path.display());
            sim
        }
        None => mofact_headless::build(&content, &scenario)?,
    };
    let camera = game::framing(&content, &sim);
    let mut ore = game::ore_for(&content, &scenario, args.seed);
    if let Some(dir) = &args.screenshot {
        return screenshots(&content, sim, &mut ore, camera, Path::new(dir));
    }

    // Touches arrive as finger events; don't also turn them into mouse events.
    sdl3::hint::set("SDL_TOUCH_MOUSE_EVENTS", "0");
    let sdl = sdl3::init().map_err(|e| e.to_string())?;
    let video = sdl.video().map_err(|e| e.to_string())?;
    let mut events = sdl.event_pump().map_err(|e| e.to_string())?;
    let mut app = App::new(&video, content.clone(), sim, ore, camera)?;
    let info = app.game.gpu.info();
    println!(
        "mofact {}: {} ({:?}), surface {:?}",
        env!("CARGO_PKG_VERSION"),
        info.name,
        info.backend,
        app.game.target.as_ref().map(Target::format),
    );

    let mut saves = autosave::Autosave::new(save_path);
    let mut presented = 0u64;
    let mut cycled = false;
    let mut second = (Instant::now(), 0u32);
    loop {
        for event in events.poll_iter() {
            if let Some(p) = app.pointer(&event) {
                app.game.handle(p);
                continue;
            }
            if let Event::KeyDown {
                keycode: Some(key),
                keymod,
                ..
            } = event
            {
                let name = key.name().to_lowercase();
                let ctrl = keymod.intersects(
                    sdl3::keyboard::Mod::LCTRLMOD
                        | sdl3::keyboard::Mod::RCTRLMOD
                        | sdl3::keyboard::Mod::LGUIMOD
                        | sdl3::keyboard::Mod::RGUIMOD,
                );
                let shift = keymod
                    .intersects(sdl3::keyboard::Mod::LSHIFTMOD | sdl3::keyboard::Mod::RSHIFTMOD);
                match (ctrl, name.as_str()) {
                    (true, "z") if shift => app.game.key("redo"),
                    (true, "z") => app.game.key("undo"),
                    (true, "y") => app.game.key("redo"),
                    (true, _) => {}
                    (false, _) => app.game.key(&name),
                }
                continue;
            }
            match event {
                Event::Quit { .. } | Event::AppTerminating { .. } => {
                    return saves.now(&app.game.sim);
                }
                Event::AppWillEnterBackground { .. } => {
                    app.suspend();
                    report(saves.now(&app.game.sim));
                }
                Event::AppDidEnterForeground { .. } => app.resume()?,
                Event::Window { win_event, .. } => match win_event {
                    WindowEvent::Minimized | WindowEvent::Hidden => {
                        app.suspend();
                        report(saves.now(&app.game.sim));
                    }
                    WindowEvent::Restored | WindowEvent::Shown | WindowEvent::Maximized => {
                        app.resume()?
                    }
                    WindowEvent::PixelSizeChanged(w, h) => {
                        app.game.resize(w.max(1) as u32, h.max(1) as u32)
                    }
                    _ => {}
                },
                _ => {}
            }
        }

        report(saves.update(&app.game.sim));
        if app.background {
            // Nothing to draw: sleep instead of spinning, to save the battery.
            std::thread::sleep(Duration::from_millis(50));
            continue;
        }
        if app.draw() == Frame::Presented {
            presented += 1;
            second.1 += 1;
        }

        if second.0.elapsed() >= Duration::from_secs(1) {
            let tick = app.game.sim.latest().tick;
            let _ = app
                .window
                .set_title(&format!("Mofact · tick {tick} · {} fps", second.1));
            second = (Instant::now(), 0);
        }

        if let Some(frames) = args.frames {
            if !cycled && presented >= frames / 2 {
                // Exercise the background path the way a phone would.
                let before = app.game.sim.latest().tick;
                app.suspend();
                saves.now(&app.game.sim)?;
                std::thread::sleep(Duration::from_millis(200));
                let during = app.game.sim.latest().tick;
                if autosave::load(saves.path(), &content).is_none() {
                    return Err("the background save did not load back".into());
                }
                app.resume()?;
                if during > before + 1 {
                    return Err(format!(
                        "simulation kept ticking in the background ({before} → {during})"
                    ));
                }
                cycled = true;
            }
            if presented >= frames {
                println!(
                    "presented {presented} frames, tick {}, went to the background and back, saved to {}",
                    app.game.sim.latest().tick,
                    saves.path().display()
                );
                return Ok(());
            }
        }
    }
}

/// Saving must never crash the game; say what went wrong and play on.
fn report(result: Result<(), String>) {
    if let Err(e) = result {
        eprintln!("mofact: {e}");
    }
}

/// Run the scenario for ten seconds, then write it at three zooms, one per band.
fn screenshots(
    content: &Content,
    mut sim: Sim,
    ore: &mut OreMap,
    camera: Camera,
    dir: &Path,
) -> Result<(), String> {
    for _ in 0..10 * mofact_core::TICKS_PER_SECOND {
        sim.step();
    }
    let prev = sim.snapshot();
    sim.step();
    let latest = sim.snapshot();
    let gpu = Gpu::headless().map_err(|e| e.to_string())?;
    let look = Look::new(content);
    let mut renderer = Renderer::new(&gpu, look.atlases());
    let mut scene = Scene::default();
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let size = (960, 540);
    for ppt in [14.0, 32.0, 64.0] {
        let camera = Camera {
            pixels_per_tile: ppt,
            ..camera
        };
        scene.build(
            &look,
            renderer.atlases(),
            &camera,
            size,
            &prev,
            &latest,
            0.5,
        );
        ore.fill(&camera, size);
        scene.ore(&look, renderer.atlases(), &camera, size, ore.tiles());
        let pixels = renderer.capture(&gpu, size, &camera, &scene, None, FLOOR);
        let path = dir.join(format!("zoom-{ppt}.png"));
        write_png(&path, size, &pixels)?;
        println!(
            "{}: {} sprites, band {}",
            path.display(),
            scene.ore.len() + scene.instances.len(),
            scene.band
        );
    }
    // The HUD on a phone held upright, with each sheet open.
    let size = (1080, 2340);
    let camera = Camera {
        pixels_per_tile: 96.0,
        ..camera
    };
    scene.build(
        &look,
        renderer.atlases(),
        &camera,
        size,
        &prev,
        &latest,
        0.5,
    );
    ore.fill(&camera, size);
    scene.ore(&look, renderer.atlases(), &camera, size, ore.tiles());
    let mut overlay = Overlay::new(&look);
    for sheet in ["toolbar", "build", "research", "radial", "recipes"] {
        let mut hud = Hud::new();
        match sheet {
            "build" => hud.toggle_build_sheet(),
            "research" => hud.toggle_research(),
            "recipes" => hud.browse("iron-plate".into()),
            "radial" => {
                hud.pointer(Pointer::Down {
                    id: 1,
                    x: 540.0,
                    y: 1100.0,
                });
                hud.poll(Instant::now() + mofact_ui::LONG_PRESS);
            }
            _ => {}
        }
        overlay.clear();
        let view = View {
            size,
            scale: 3.0,
            content,
            look: &look,
            snapshot: &latest,
            tool: &Tool::Hand,
            dir: mofact_core::geom::Dir::East,
            recipe: None,
            belt: &"belt".into(),
        };
        hud.draw(&view, &mut overlay);
        let pixels = renderer.capture(&gpu, size, &camera, &scene, Some(&overlay), FLOOR);
        let path = dir.join(format!("hud-{sheet}.png"));
        write_png(&path, size, &pixels)?;
        println!("{}", path.display());
    }
    Ok(())
}

fn write_png(path: &Path, (w, h): (u32, u32), rgba: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())
}
