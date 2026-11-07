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
        self.game.safe = self.safe_area(size);
        self.game.frame(size, scale)
    }

    /// The edges a notch, rounded corners or the home indicator cover, in
    /// pixels: top, right, bottom, left. None on a plain window.
    fn safe_area(&self, (pw, ph): (u32, u32)) -> [f32; 4] {
        let (ww, _) = self.window.size();
        let Ok(r) = self.window.safe_area() else {
            return [0.0; 4];
        };
        let k = pw as f32 / ww.max(1) as f32;
        let (x, y) = (r.x() as f32 * k, r.y() as f32 * k);
        let (w, h) = (r.width() as f32 * k, r.height() as f32 * k);
        [
            y.max(0.0),
            (pw as f32 - x - w).max(0.0),
            (ph as f32 - y - h).max(0.0),
            x.max(0.0),
        ]
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

/// SDL's audio thread pulls the game's sound through this.
struct Speaker {
    audio: mofact_audio::Audio,
    buf: Vec<f32>,
}

impl sdl3::audio::AudioCallback<f32> for Speaker {
    fn callback(&mut self, stream: &mut sdl3::audio::AudioStream, requested: i32) {
        // Whole stereo frames.
        let n = (requested.max(0) as usize + 1) & !1;
        self.buf.resize(n, 0.0);
        self.audio.render(&mut self.buf);
        if stream.put_data_f32(&self.buf).is_err() {
            self.buf.clear();
        }
    }
}

/// Start the game's sound, or say why there is none: a machine with no
/// audio device (CI) still plays, silently.
fn speaker(
    sdl: &sdl3::Sdl,
    audio: mofact_audio::Audio,
) -> Option<sdl3::audio::AudioStreamWithCallback<Speaker>> {
    let open = || -> Result<_, String> {
        let sub = sdl.audio().map_err(|e| e.to_string())?;
        let spec = sdl3::audio::AudioSpec {
            freq: Some(audio.lock().rate() as i32),
            channels: Some(2),
            format: Some(sdl3::audio::AudioFormat::f32_sys()),
        };
        let stream = sub
            .open_playback_stream(
                &spec,
                Speaker {
                    audio,
                    buf: Vec::new(),
                },
            )
            .map_err(|e| e.to_string())?;
        stream.resume().map_err(|e| e.to_string())?;
        Ok(stream)
    };
    open()
        .inspect_err(|e| eprintln!("mofact: no sound ({e})"))
        .ok()
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
    run_with(Platform::default())
}

/// Makes the platform's [`Shop`](crate::shop::Shop) from whether the full
/// game is remembered as bought, and a way to remember it.
pub type MakeShop = Box<dyn FnOnce(bool, Box<dyn FnMut(bool)>) -> Box<dyn crate::shop::Shop>>;

/// What a phone brings to [`run_with`] that SDL doesn't give.
#[derive(Default)]
pub struct Platform {
    /// Its store for the full game; with none, the
    /// [`Sandbox`](crate::shop::Sandbox).
    pub shop: Option<MakeShop>,
    /// Plays a feel on the device.
    pub haptic: Option<fn(crate::haptics::Haptic)>,
}

/// [`run`], with what the platform brings.
pub fn run_with(platform: Platform) -> Result<(), String> {
    let Platform { shop, haptic } = platform;
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
    let mut files = autosave::Files::new(save_path);
    let save_path = files.path(crate::slots::Store::current(&files));
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
    let speaker = speaker(&sdl, app.game.sound.audio());
    // Quiet in the background, as the game is paused there.
    let hush = |quiet: bool| {
        if let Some(s) = &speaker {
            let _ = if quiet { s.pause() } else { s.resume() };
        }
    };
    let info = app.game.gpu.info();
    println!(
        "mofact {}: {} ({:?}), surface {:?}",
        env!("CARGO_PKG_VERSION"),
        info.name,
        info.backend,
        app.game.target.as_ref().map(Target::format),
    );

    let mut saves = autosave::Autosave::new(save_path);
    let mut settings = files.settings();
    app.game.set_low_power(settings.0);
    app.game.set_colour_blind(settings.1);
    let (owned, keep) = (files.full_game(), Box::new(files.keep_full_game()));
    app.game.set_shop(match shop {
        Some(make) => make(owned, keep),
        None => Box::new(crate::shop::Sandbox::new(owned, keep)),
    });
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
                    hush(true);
                    report(saves.now(&app.game.sim));
                }
                Event::AppDidEnterForeground { .. } => {
                    app.resume()?;
                    hush(false);
                }
                Event::Window { win_event, .. } => match win_event {
                    WindowEvent::Minimized | WindowEvent::Hidden => {
                        app.suspend();
                        hush(true);
                        report(saves.now(&app.game.sim));
                    }
                    WindowEvent::Restored | WindowEvent::Shown | WindowEvent::Maximized => {
                        app.resume()?;
                        hush(false);
                    }
                    WindowEvent::PixelSizeChanged(w, h) => {
                        app.game.resize(w.max(1) as u32, h.max(1) as u32)
                    }
                    _ => {}
                },
                _ => {}
            }
        }

        if app.game.wants_slots() {
            report(app.game.serve(&mut files));
            saves.set_path(files.path(crate::slots::Store::current(&files)));
        }
        let now = (app.game.low_power(), app.game.colour_blind());
        if now != settings {
            settings = now;
            report(files.set_settings(now));
        }
        report(saves.update(&app.game.sim));
        if app.background {
            // Nothing to draw: sleep instead of spinning, to save the battery.
            std::thread::sleep(Duration::from_millis(50));
            continue;
        }
        let frame = app.draw();
        if let (Some(play), Some(feel)) = (haptic, app.game.haptic.take()) {
            play(feel);
        }
        match frame {
            Frame::Presented => {
                presented += 1;
                second.1 += 1;
            }
            // Not due yet (30 fps pacing): wait a moment rather than spin.
            Frame::Skipped => std::thread::sleep(Duration::from_millis(2)),
            Frame::Reconfigured => {}
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
                hush(true);
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
    // The last at the dead of night (S9).
    for (ppt, night) in [(14.0, 0.0), (32.0, 0.0), (64.0, 0.0), (32.0, 1.0)] {
        let camera = Camera {
            pixels_per_tile: ppt,
            ..camera
        };
        scene.night = night;
        scene.build(
            &look,
            renderer.atlases(),
            &camera,
            size,
            &prev,
            &latest,
            0.5,
        );
        // Animations as they stand 1.3 seconds in.
        scene.animate(&look, renderer.atlases(), &camera, size, &latest, 1.3);
        ore.fill(&camera, size);
        scene.ore(&look, renderer.atlases(), &camera, size, ore.tiles());
        scene.decor(renderer.atlases(), &camera, size, ore.decor(&camera, size));
        scene.terrain(&camera, size, ore.version(), |t, s| ore.terrain(t, s));
        let pixels = renderer.capture(&gpu, size, &camera, &scene, None, FLOOR);
        let path = if night > 0.0 {
            dir.join(format!("night-{ppt}.png"))
        } else {
            dir.join(format!("zoom-{ppt}.png"))
        };
        write_png(&path, size, &pixels)?;
        println!(
            "{}: {} sprites, band {}",
            path.display(),
            scene.ore.len() + scene.instances.len(),
            scene.band
        );
    }
    scene.night = 0.0;
    // The world around the start and further out, on a phone held upright.
    let size = (1080, 2340);
    for (name, center, ppt) in [
        ("world-start", camera.center, 48.0),
        ("world-wide", camera.center, 20.0),
        ("world-west", [-60.0, -10.0], 40.0),
        ("world-north", [40.0, -110.0], 40.0),
        ("world-cliffs", [27.0, 70.0], 32.0),
        ("world-river", [54.0, -6.0], 32.0),
        ("world-desert", [-409.0, 326.0], 32.0),
        ("world-tundra", [991.0, -381.0], 32.0),
        ("world-volcanic", [312.0, 1670.0], 32.0),
    ] {
        let camera = Camera {
            center,
            pixels_per_tile: ppt,
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
        scene.decor(renderer.atlases(), &camera, size, ore.decor(&camera, size));
        scene.terrain(&camera, size, ore.version(), |t, s| ore.terrain(t, s));
        let pixels = renderer.capture(&gpu, size, &camera, &scene, None, FLOOR);
        let path = dir.join(format!("{name}.png"));
        write_png(&path, size, &pixels)?;
        println!("{}", path.display());
    }
    // The HUD with each sheet open, on a small phone, a large phone and a
    // tablet (T40), each held both ways, with their notches and home
    // indicators: top, right, bottom, left insets in points.
    let screens: [Screen; 6] = [
        ("small-phone", (640, 1136), 2.0, [20.0, 0.0, 0.0, 0.0]),
        ("small-phone-wide", (1136, 640), 2.0, [0.0; 4]),
        ("phone", (1179, 2556), 3.0, [59.0, 0.0, 34.0, 0.0]),
        ("phone-wide", (2556, 1179), 3.0, [0.0, 59.0, 21.0, 59.0]),
        ("tablet", (2048, 2732), 2.0, [24.0, 0.0, 20.0, 0.0]),
        ("tablet-wide", (2732, 2048), 2.0, [24.0, 0.0, 20.0, 0.0]),
    ];
    // The problems list needs something stuck: say the machines are starved.
    let mut stuck = latest.clone();
    for e in &mut stuck.entities {
        if look.has_badge(&e.building) {
            e.state = mofact_core::snapshot::MachineState::Starved;
        }
    }
    let mut overlay = Overlay::new(&look);
    // The game menu lists this game, another, and an empty slot.
    let slots = [
        crate::slots::describe(&sim, true),
        mofact_ui::SlotView {
            used: true,
            current: false,
            title: "Lakes · seed 90210".into(),
            detail: "2 h 5 min played · 412 built".into(),
        },
        mofact_ui::SlotView::default(),
    ];
    // The world map on a phone: the factory as a schematic, ore patches as
    // icons, two pins, and the map button lit.
    for (name, ppt) in [("map", 1.0), ("map-far", 0.5)] {
        let (size, scale) = ((1179, 2556), 3.0);
        let camera = Camera {
            pixels_per_tile: ppt * scale,
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
        scene.decor(renderer.atlases(), &camera, size, ore.decor(&camera, size));
        scene.terrain(&camera, size, ore.version(), |t, s| ore.terrain(t, s));
        overlay.clear();
        crate::worldmap::factory(&mut overlay, content, &look, &camera, size, scale, &latest);
        let patches = ore.patches(&camera, size);
        crate::worldmap::patches(&mut overlay, &look, &camera, size, scale, &patches);
        let pins = [
            mofact_core::geom::TilePos::new(-409, 326),
            mofact_core::geom::TilePos::new(180, -260),
        ];
        crate::worldmap::pins(&mut overlay, &camera, size, scale, &pins);
        let mut hud = Hud::new();
        hud.set_map(true);
        let view = View {
            size,
            scale,
            content,
            look: &look,
            snapshot: &latest,
            tool: &Tool::Hand,
            dir: mofact_core::geom::Dir::East,
            recipe: None,
            belt: &"belt".into(),
            history: (false, false),
            safe: [59.0 * scale, 0.0, 34.0 * scale, 0.0],
            nudge: None,
            slots: &[],
            store: &mofact_ui::StoreView::default(),
        };
        hud.draw(&view, &mut overlay);
        let pixels = renderer.capture(&gpu, size, &camera, &scene, Some(&overlay), FLOOR);
        let path = dir.join(format!("{name}.png"));
        write_png(&path, size, &pixels)?;
        println!("{}", path.display());
    }
    // Colour-blind mode (T58): stuck machines up close with the problems
    // list, then the same factory on the map.
    {
        let look = Look::with_colour_blind(content, true);
        let mut renderer = Renderer::new(&gpu, look.atlases());
        let mut overlay = Overlay::new(&look);
        let (size, scale) = ((1179, 2556), 3.0);
        let mut stuck = stuck.clone();
        for (n, e) in stuck.entities.iter_mut().enumerate() {
            if look.has_badge(&e.building) {
                e.state = [
                    mofact_core::snapshot::MachineState::Working,
                    mofact_core::snapshot::MachineState::Starved,
                    mofact_core::snapshot::MachineState::Blocked,
                ][n % 3];
            }
        }
        for (name, ppt) in [("hud-phone-colour-blind", 32.0), ("map-colour-blind", 1.0)] {
            let camera = Camera {
                pixels_per_tile: ppt * scale,
                ..camera
            };
            scene.build(&look, renderer.atlases(), &camera, size, &prev, &stuck, 0.5);
            scene.animate(&look, renderer.atlases(), &camera, size, &stuck, 1.3);
            ore.fill(&camera, size);
            scene.ore(&look, renderer.atlases(), &camera, size, ore.tiles());
            scene.decor(renderer.atlases(), &camera, size, ore.decor(&camera, size));
            scene.terrain(&camera, size, ore.version(), |t, s| ore.terrain(t, s));
            overlay.clear();
            let mut hud = Hud::new();
            hud.set_colour_blind(true);
            if ppt < 4.0 {
                hud.set_map(true);
                crate::worldmap::factory(
                    &mut overlay,
                    content,
                    &look,
                    &camera,
                    size,
                    scale,
                    &stuck,
                );
                let patches = ore.patches(&camera, size);
                crate::worldmap::patches(&mut overlay, &look, &camera, size, scale, &patches);
            } else {
                hud.toggle_problems();
            }
            let view = View {
                size,
                scale,
                content,
                look: &look,
                snapshot: &stuck,
                tool: &Tool::Hand,
                dir: mofact_core::geom::Dir::East,
                recipe: None,
                belt: &"belt".into(),
                history: (false, false),
                safe: [59.0 * scale, 0.0, 34.0 * scale, 0.0],
                nudge: None,
                slots: &slots,
                store: &mofact_ui::StoreView::default(),
            };
            hud.draw(&view, &mut overlay);
            let pixels = renderer.capture(&gpu, size, &camera, &scene, Some(&overlay), FLOOR);
            let path = dir.join(format!("{name}.png"));
            write_png(&path, size, &pixels)?;
            println!("{}", path.display());
        }
    }
    for (name, size, scale, safe) in screens {
        let camera = Camera {
            pixels_per_tile: 32.0 * scale,
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
        scene.decor(renderer.atlases(), &camera, size, ore.decor(&camera, size));
        scene.terrain(&camera, size, ore.version(), |t, s| ore.terrain(t, s));
        let sheets = [
            "toolbar", "build", "research", "radial", "recipes", "problems", "train", "machine",
            "menu", "new-game", "plan", "mega", "unlock",
        ];
        // As a fresh install sees it: the demo, with the price known.
        let store = mofact_ui::StoreView {
            demo: Some(crate::shop::DEMO_TIER),
            price: Some("€4.99".into()),
            ..Default::default()
        };
        for sheet in sheets {
            let mut hud = Hud::new();
            match sheet {
                // A world with trains shows the first one's sheet.
                "train" => match latest.trains.first() {
                    Some(t) => hud.select(Some(t.id)),
                    None => continue,
                },
                // A world with an assembler shows its sheet and recipes.
                "machine" => {
                    let asm = latest
                        .entities
                        .iter()
                        .find(|e| e.building.as_str() == "assembler");
                    match asm {
                        Some(e) => hud.select(Some(e.id)),
                        None => continue,
                    }
                }
                // A world with a megaproject shows its stage.
                "mega" => match &latest.mega {
                    Some(m) => hud.select(Some(m.id)),
                    None => continue,
                },
                "build" => hud.toggle_build_sheet(),
                "research" => hud.toggle_research(),
                "unlock" => hud.open_unlock(),
                "problems" => hud.toggle_problems(),
                "recipes" => hud.browse("iron-plate".into()),
                // A whole chain for the ratio calculator.
                "plan" => hud.browse("circuit".into()),
                "menu" => hud.open_menu(None),
                "new-game" => hud.open_menu(Some((2, 271_828))),
                "radial" => {
                    hud.pointer(Pointer::Down {
                        id: 1,
                        x: size.0 as f32 / 2.0,
                        y: size.1 as f32 / 2.0,
                    });
                    hud.poll(Instant::now() + mofact_ui::LONG_PRESS);
                }
                _ => {}
            }
            overlay.clear();
            let view = View {
                size,
                scale,
                content,
                look: &look,
                snapshot: if sheet == "problems" { &stuck } else { &latest },
                tool: &Tool::Hand,
                dir: mofact_core::geom::Dir::East,
                recipe: None,
                belt: &"belt".into(),
                history: (false, false),
                safe: safe.map(|v| v * scale),
                nudge: None,
                slots: &slots,
                store: &store,
            };
            hud.draw(&view, &mut overlay);
            let pixels = renderer.capture(&gpu, size, &camera, &scene, Some(&overlay), FLOOR);
            let path = dir.join(format!("hud-{name}-{sheet}.png"));
            write_png(&path, size, &pixels)?;
            println!("{}", path.display());
        }
    }
    Ok(())
}

/// A screen to show the HUD on: its name, size in pixels, pixels per point
/// and the insets its display covers, in points.
type Screen = (&'static str, (u32, u32), f32, [f32; 4]);

fn write_png(path: &Path, (w, h): (u32, u32), rgba: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())
}
