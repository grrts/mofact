//! The Mofact app: an SDL3 window with a wgpu surface, and the simulation on
//! its own thread (T03).
//!
//! Going to the background (a phone app switch, or a minimised desktop
//! window) drops the GPU surface and pauses the simulation; coming back makes
//! a new surface and resumes. The GPU device and everything on it survive.
//!
//! `mofact --frames N` presents N frames and exits, going to the background
//! and back halfway through. CI runs it to prove the shell works on a real
//! (software) GPU. `mofact --screenshot DIR` writes the scene at three zooms
//! as PNGs instead of opening a window. `--scenario FILE` starts from one of
//! the headless scenarios instead of the saved game.
//!
//! The game autosaves every 30 seconds and on going to the background, to
//! `--save FILE` (by default the platform's per-user data folder), and opens
//! that save next time.

mod autosave;
mod built;
mod bundled;
mod ore;
#[cfg(test)]
mod replay;
mod surface;

use mofact_core::content::Content;
use mofact_input::Ground;
use mofact_input::{CameraControl, Controls, Pointer, Tool, MOUSE, MOUSE_PAN};
use mofact_render::{wgpu, Camera, Frame, Gpu, Look, Overlay, Renderer, Scene, Target, FLOOR};
use mofact_sim::{Sim, SimHandle};
use mofact_ui::{Action, Hud, View};
use ore::OreMap;
use sdl3::event::{Event, WindowEvent};
use sdl3::keyboard::Keycode;
use sdl3::mouse::MouseButton;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

struct App {
    // Field order is drop order: the surface goes before the window it draws into.
    target: Option<Target>,
    renderer: Renderer,
    gpu: Gpu,
    window: sdl3::video::Window,
    sim: SimHandle,
    look: Look,
    scene: Scene,
    overlay: Overlay,
    hud: Hud,
    /// A letter key for each building the toolbar has no number for.
    hotkeys: Vec<(char, mofact_core::ids::BuildingId)>,
    content: Content,
    ore: OreMap,
    controls: Controls,
    built: built::Built,
    /// The mouse button held, as a pointer id.
    mouse: u64,
    /// When the latest snapshot arrived, for smoothing between ticks.
    latest: (u64, Instant),
    /// The platform has taken the window away, or it is minimised.
    background: bool,
}

impl App {
    fn new(
        video: &sdl3::VideoSubsystem,
        content: Content,
        sim: Sim,
        look: Look,
        ore: OreMap,
        built: built::Built,
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
        let controls = Controls::new(CameraControl::new(
            camera.center,
            camera.pixels_per_tile,
            [w as f32, h as f32],
        ));
        let overlay = Overlay::new(&look);
        Ok(Self {
            target: Some(target),
            renderer: Renderer::new(&gpu, look.atlases()),
            gpu,
            window,
            sim: SimHandle::spawn(sim),
            look,
            scene: Scene::default(),
            overlay,
            hud: Hud::new(),
            hotkeys: mofact_ui::hotkeys(&content),
            content,
            ore,
            controls,
            built,
            mouse: MOUSE,
            latest: (0, Instant::now()),
            background: false,
        })
    }

    /// The platform is taking the window away: let go of the surface, stop ticking.
    fn suspend(&mut self) {
        if !self.background {
            self.background = true;
            self.target = None;
            self.sim.set_paused(true);
        }
    }

    /// Back in front: a new surface, and the simulation carries on where it stopped.
    fn resume(&mut self) -> Result<(), String> {
        if self.background {
            let surface = surface::create(&self.gpu.instance, &self.window)?;
            let (w, h) = self.window.size_in_pixels();
            self.target = Some(Target::new(&self.gpu, surface, w, h).map_err(|e| e.to_string())?);
            self.background = false;
            self.sim.set_paused(false);
        }
        Ok(())
    }

    fn draw(&mut self) -> Frame {
        let Some(target) = self.target.as_mut().filter(|_| !self.background) else {
            return Frame::Skipped;
        };
        // Some platforms resize without telling us; follow the window every frame.
        let (w, h) = self.window.size_in_pixels();
        if (w, h) != target.size() {
            target.resize(&self.gpu, w, h);
            self.controls.camera.set_viewport(w as f32, h as f32);
        }
        let (prev, latest) = self.sim.snapshots();
        if latest.tick != self.latest.0 {
            self.latest = (latest.tick, Instant::now());
        }
        let alpha = (self.latest.1.elapsed().as_secs_f32() / mofact_core::TICK_SECONDS).min(1.0);
        let camera = Camera {
            center: self.controls.camera.center,
            pixels_per_tile: self.controls.camera.pixels_per_tile,
        };
        let atlases = self.renderer.atlases();
        self.scene.build(
            &self.look,
            atlases,
            &camera,
            target.size(),
            &prev,
            &latest,
            alpha,
        );
        self.ore.update(&camera, target.size());
        self.scene.ore(
            &self.look,
            atlases,
            &camera,
            target.size(),
            self.ore.tiles(),
        );
        let ghosts = self.controls.builder.ghosts();
        let recipe = self.controls.builder.recipe.as_ref();
        self.scene.ghosts(
            &self.look,
            atlases,
            ghosts.iter().map(|g| (&g.building, g.tile, g.dir, recipe)),
        );
        self.overlay.clear();
        let builder = &self.controls.builder;
        let view = View {
            size: target.size(),
            scale: self.window.display_scale(),
            content: &self.content,
            look: &self.look,
            snapshot: &latest,
            tool: &builder.tool,
            dir: builder.dir,
            recipe: builder.recipe.as_ref(),
            belt: &builder.belt,
        };
        self.hud.draw(&view, &mut self.overlay);
        target.draw(
            &self.gpu,
            &mut self.renderer,
            &camera,
            &self.scene,
            Some(&self.overlay),
        )
    }

    /// Keys follow the toolbar: 1 to 5 pick hand, belt, drill, furnace and
    /// assembler, 6 opens the build sheet and 7 (or X) removes; T opens
    /// research; R rotates; Escape goes back to the hand. Every other
    /// building has a letter of its own (see [`mofact_ui::hotkeys`]).
    fn key(&mut self, key: Keycode) {
        let tool = match key {
            Keycode::_1 | Keycode::Escape => Tool::Hand,
            Keycode::_2 => Tool::Belt,
            Keycode::_3 => Tool::Place("drill".into()),
            Keycode::_4 => Tool::Place("furnace".into()),
            Keycode::_5 => Tool::Place("assembler".into()),
            Keycode::_6 => return self.hud.toggle_build_sheet(),
            Keycode::T => return self.hud.toggle_research(),
            Keycode::_7 | Keycode::X => Tool::Remove,
            Keycode::R => return self.controls.builder.rotate(),
            other => {
                let name = other.name().to_lowercase();
                let mut letters = name.chars();
                let (Some(c), None) = (letters.next(), letters.next()) else {
                    return;
                };
                let Some((_, b)) = self.hotkeys.iter().find(|(k, _)| *k == c) else {
                    return;
                };
                Tool::Place(b.clone())
            }
        };
        if tool != Tool::Hand {
            self.hud.select(None);
        }
        self.controls.builder.set_tool(tool);
    }

    /// Do what the HUD asks.
    fn act(&mut self, action: Action) {
        match action {
            Action::Tool(tool) => self.controls.builder.set_tool(tool),
            Action::Rotate => self.controls.builder.rotate(),
            Action::Recipe(recipe) => self.controls.builder.recipe = recipe,
            Action::Belt(belt) => self.controls.builder.belt = belt,
            Action::Send(command) => self.sim.send(command),
            Action::Inspect(at) if self.controls.builder.tool == Tool::Hand => {
                let [x, y] = self.controls.camera.world_at(at);
                let tile = mofact_core::geom::TilePos::new(x.floor() as i32, y.floor() as i32);
                self.built.update(&self.sim.latest());
                self.hud.select(self.built.at(tile).map(|o| o.id));
            }
            Action::Inspect(_) => {}
            Action::Cancel => self.controls.builder.cancel(),
        }
    }

    /// A pointer event goes to the HUD first, then to the camera and build tools.
    fn handle(&mut self, p: Pointer) {
        let (taken, actions) = self.hud.pointer(p);
        for action in actions {
            self.act(action);
        }
        if !taken {
            self.built.update(&self.sim.latest());
            for command in self.controls.handle(p, &self.built) {
                self.sim.send(command);
            }
        }
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
    let camera = framing(&content, &sim);
    let placed = scenario
        .ore
        .iter()
        .map(|(x, y, item)| (mofact_core::geom::TilePos::new(*x, *y), item.clone()))
        .collect();
    let mut ore = OreMap::new(&content, args.seed, placed);
    if let Some(dir) = &args.screenshot {
        return screenshots(&content, sim, &mut ore, camera, Path::new(dir));
    }

    // Touches arrive as finger events; don't also turn them into mouse events.
    sdl3::hint::set("SDL_TOUCH_MOUSE_EVENTS", "0");
    let sdl = sdl3::init().map_err(|e| e.to_string())?;
    let video = sdl.video().map_err(|e| e.to_string())?;
    let mut events = sdl.event_pump().map_err(|e| e.to_string())?;
    let mut app = App::new(
        &video,
        content.clone(),
        sim,
        Look::new(&content),
        ore,
        built::Built::new(&content),
        camera,
    )?;
    let info = app.gpu.info();
    println!(
        "mofact {}: {} ({:?}), surface {:?}",
        env!("CARGO_PKG_VERSION"),
        info.name,
        info.backend,
        app.target.as_ref().map(Target::format),
    );

    let mut saves = autosave::Autosave::new(save_path);
    let mut presented = 0u64;
    let mut cycled = false;
    let mut second = (Instant::now(), 0u32);
    loop {
        for event in events.poll_iter() {
            if let Some(p) = app.pointer(&event) {
                app.handle(p);
                continue;
            }
            if let Event::KeyDown {
                keycode: Some(key), ..
            } = event
            {
                app.key(key);
                continue;
            }
            match event {
                Event::Quit { .. } | Event::AppTerminating { .. } => {
                    return saves.now(&app.sim);
                }
                Event::AppWillEnterBackground { .. } => {
                    app.suspend();
                    report(saves.now(&app.sim));
                }
                Event::AppDidEnterForeground { .. } => app.resume()?,
                Event::Window { win_event, .. } => match win_event {
                    WindowEvent::Minimized | WindowEvent::Hidden => {
                        app.suspend();
                        report(saves.now(&app.sim));
                    }
                    WindowEvent::Restored | WindowEvent::Shown | WindowEvent::Maximized => {
                        app.resume()?
                    }
                    WindowEvent::PixelSizeChanged(w, h) => {
                        if let Some(t) = &mut app.target {
                            t.resize(&app.gpu, w.max(1) as u32, h.max(1) as u32);
                        }
                        app.controls.camera.set_viewport(w as f32, h as f32);
                    }
                    _ => {}
                },
                _ => {}
            }
        }

        for result in app.sim.results() {
            if let mofact_core::command::CommandResult::Refused(why) = result {
                use mofact_core::command::CommandError as E;
                app.hud.toast(match why {
                    E::Blocked => "Something is in the way.",
                    E::CannotAfford => "Not enough plates in stock.",
                    E::Locked => "That isn't unlocked yet.",
                    E::NoSuchEntity => "That's already gone.",
                    E::UnknownContent => "That isn't in this game's data.",
                });
            }
        }
        report(saves.update(&app.sim));
        if app.background {
            // Nothing to draw: sleep instead of spinning, to save the battery.
            std::thread::sleep(Duration::from_millis(50));
            continue;
        }
        for action in app.hud.poll(Instant::now()) {
            app.act(action);
        }
        if app.draw() == Frame::Presented {
            presented += 1;
            second.1 += 1;
        }

        if second.0.elapsed() >= Duration::from_secs(1) {
            let tick = app.sim.latest().tick;
            let _ = app
                .window
                .set_title(&format!("Mofact · tick {tick} · {} fps", second.1));
            second = (Instant::now(), 0);
        }

        if let Some(frames) = args.frames {
            if !cycled && presented >= frames / 2 {
                // Exercise the background path the way a phone would.
                let before = app.sim.latest().tick;
                app.suspend();
                saves.now(&app.sim)?;
                std::thread::sleep(Duration::from_millis(200));
                let during = app.sim.latest().tick;
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
                    app.sim.latest().tick,
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

/// A camera on the middle of everything built, at the medium zoom band.
fn framing(content: &Content, sim: &Sim) -> Camera {
    let snap = sim.snapshot();
    let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
    for e in &snap.entities {
        let size = content
            .buildings
            .iter()
            .find(|b| b.id == e.building)
            .map_or((1, 1), |b| b.size);
        lo = [lo[0].min(e.pos.x as f32), lo[1].min(e.pos.y as f32)];
        hi = [
            hi[0].max((e.pos.x + i32::from(size.0)) as f32),
            hi[1].max((e.pos.y + i32::from(size.1)) as f32),
        ];
    }
    let center = if snap.entities.is_empty() {
        [0.0, 0.0]
    } else {
        [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0]
    };
    Camera {
        center,
        pixels_per_tile: 32.0,
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
    for sheet in ["toolbar", "build", "research", "radial"] {
        let mut hud = Hud::new();
        match sheet {
            "build" => hud.toggle_build_sheet(),
            "research" => hud.toggle_research(),
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
