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
//! the headless scenarios (the starter line by default).

mod surface;

use mofact_core::content::Content;
use mofact_input::{CameraControl, Pointer, MOUSE};
use mofact_render::{scene, wgpu, Camera, Frame, Gpu, Look, Renderer, Scene, Target, FLOOR};
use mofact_sim::{Sim, SimHandle};
use mofact_ui as _;
use mofact_world as _;
use sdl3::event::{Event, WindowEvent};
use sdl3::mouse::MouseButton;
use std::path::Path;
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
    control: CameraControl,
    /// When the latest snapshot arrived, for smoothing between ticks.
    latest: (u64, Instant),
    /// The platform has taken the window away, or it is minimised.
    background: bool,
}

impl App {
    fn new(
        video: &sdl3::VideoSubsystem,
        sim: Sim,
        look: Look,
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
        let control =
            CameraControl::new(camera.center, camera.pixels_per_tile, [w as f32, h as f32]);
        Ok(Self {
            target: Some(target),
            renderer: Renderer::new(&gpu, scene::atlases()),
            gpu,
            window,
            sim: SimHandle::spawn(sim),
            look,
            scene: Scene::default(),
            control,
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
        let (prev, latest) = self.sim.snapshots();
        if latest.tick != self.latest.0 {
            self.latest = (latest.tick, Instant::now());
        }
        let alpha = (self.latest.1.elapsed().as_secs_f32() / mofact_core::TICK_SECONDS).min(1.0);
        let camera = Camera {
            center: self.control.center,
            pixels_per_tile: self.control.pixels_per_tile,
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
        target.draw(&self.gpu, &mut self.renderer, &camera, &self.scene)
    }

    /// An SDL event as a pointer event in screen pixels, if it is one.
    fn pointer(&self, event: &Event) -> Option<Pointer> {
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
            } if drag(mouse_btn) => Pointer::Down {
                id: MOUSE,
                x: x * scale,
                y: y * scale,
            },
            Event::MouseButtonUp { mouse_btn, .. } if drag(mouse_btn) => Pointer::Up { id: MOUSE },
            Event::MouseMotion { x, y, .. } => Pointer::Move {
                id: MOUSE,
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
    scenario: String,
    data: String,
}

fn args() -> Result<Args, String> {
    let mut args = Args {
        frames: None,
        screenshot: None,
        scenario: "scenarios/starter-line.ron".into(),
        data: "data".into(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--frames" => {
                let n = it.next().ok_or("--frames needs a number")?;
                args.frames = Some(n.parse().map_err(|_| format!("bad frame count `{n}`"))?);
            }
            "--data" => args.data = it.next().ok_or("--data needs a folder")?,
            "--scenario" => args.scenario = it.next().ok_or("--scenario needs a file")?,
            "--screenshot" => {
                args.screenshot = Some(it.next().ok_or("--screenshot needs a folder")?)
            }
            other => {
                return Err(format!(
                    "unknown argument `{other}` (try --frames N, --screenshot DIR, --scenario FILE, --data DIR)"
                ))
            }
        }
    }
    Ok(args)
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("mofact: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args = args()?;
    let content = mofact_content::load_dir(Path::new(&args.data)).map_err(|e| e.to_string())?;
    let scenario = std::fs::read_to_string(&args.scenario)
        .map_err(|e| format!("{}: {e}", args.scenario))
        .and_then(|text| mofact_headless::parse(&text))?;
    let sim = mofact_headless::build(&content, &scenario)?;
    let camera = framing(&content, &sim);
    if let Some(dir) = &args.screenshot {
        return screenshots(&content, sim, camera, Path::new(dir));
    }

    // Touches arrive as finger events; don't also turn them into mouse events.
    sdl3::hint::set("SDL_TOUCH_MOUSE_EVENTS", "0");
    let sdl = sdl3::init().map_err(|e| e.to_string())?;
    let video = sdl.video().map_err(|e| e.to_string())?;
    let mut events = sdl.event_pump().map_err(|e| e.to_string())?;
    let mut app = App::new(&video, sim, Look::new(&content), camera)?;
    let info = app.gpu.info();
    println!(
        "mofact {}: {} ({:?}), surface {:?}",
        env!("CARGO_PKG_VERSION"),
        info.name,
        info.backend,
        app.target.as_ref().map(Target::format),
    );

    let mut presented = 0u64;
    let mut cycled = false;
    let mut second = (Instant::now(), 0u32);
    loop {
        for event in events.poll_iter() {
            if let Some(p) = app.pointer(&event) {
                app.control.handle(p);
                continue;
            }
            match event {
                Event::Quit { .. } | Event::AppTerminating { .. } => return Ok(()),
                Event::AppWillEnterBackground { .. } => app.suspend(),
                Event::AppDidEnterForeground { .. } => app.resume()?,
                Event::Window { win_event, .. } => match win_event {
                    WindowEvent::Minimized | WindowEvent::Hidden => app.suspend(),
                    WindowEvent::Restored | WindowEvent::Shown | WindowEvent::Maximized => {
                        app.resume()?
                    }
                    WindowEvent::PixelSizeChanged(w, h) => {
                        if let Some(t) = &mut app.target {
                            t.resize(&app.gpu, w.max(1) as u32, h.max(1) as u32);
                        }
                        app.control.set_viewport(w as f32, h as f32);
                    }
                    _ => {}
                },
                _ => {}
            }
        }

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
                std::thread::sleep(Duration::from_millis(200));
                let during = app.sim.latest().tick;
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
                    "presented {presented} frames, tick {}, went to the background and back",
                    app.sim.latest().tick
                );
                return Ok(());
            }
        }
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
fn screenshots(content: &Content, mut sim: Sim, camera: Camera, dir: &Path) -> Result<(), String> {
    for _ in 0..10 * mofact_core::TICKS_PER_SECOND {
        sim.step();
    }
    let prev = sim.snapshot();
    sim.step();
    let latest = sim.snapshot();
    let gpu = Gpu::headless().map_err(|e| e.to_string())?;
    let mut renderer = Renderer::new(&gpu, scene::atlases());
    let look = Look::new(content);
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
        let pixels = renderer.capture(&gpu, size, &camera, &scene, FLOOR);
        let path = dir.join(format!("zoom-{ppt}.png"));
        write_png(&path, size, &pixels)?;
        println!(
            "{}: {} sprites, band {}",
            path.display(),
            scene.instances.len(),
            scene.band
        );
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
