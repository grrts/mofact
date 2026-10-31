//! The Mofact app: an SDL3 window with a wgpu surface, and the simulation on
//! its own thread (T03).
//!
//! Going to the background (a phone app switch, or a minimised desktop
//! window) drops the GPU surface and pauses the simulation; coming back makes
//! a new surface and resumes. The GPU device and everything on it survive.
//!
//! `mofact --frames N` presents N frames and exits, going to the background
//! and back halfway through. CI runs it to prove the shell works on a real
//! (software) GPU.

mod surface;

use mofact_input as _;
use mofact_render::{wgpu, Frame, Gpu, Target, FLOOR};
use mofact_sim::{Sim, SimHandle};
use mofact_ui as _;
use mofact_world as _;
use sdl3::event::{Event, WindowEvent};
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, Instant};

struct App {
    // Field order is drop order: the surface goes before the window it draws into.
    target: Option<Target>,
    gpu: Gpu,
    window: sdl3::video::Window,
    sim: SimHandle,
    /// The platform has taken the window away, or it is minimised.
    background: bool,
}

impl App {
    fn new(video: &sdl3::VideoSubsystem, sim: Sim) -> Result<Self, String> {
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
            target: Some(target),
            gpu,
            window,
            sim: SimHandle::spawn(sim),
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
        match &mut self.target {
            Some(target) if !self.background => target.draw(&self.gpu, FLOOR),
            _ => Frame::Skipped,
        }
    }
}

struct Args {
    frames: Option<u64>,
    data: String,
}

fn args() -> Result<Args, String> {
    let mut args = Args {
        frames: None,
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
            other => {
                return Err(format!(
                    "unknown argument `{other}` (try --frames N, --data DIR)"
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
    let sim = match mofact_content::load_dir(Path::new(&args.data)) {
        Ok(content) => Sim::with_content(&content),
        Err(e) => {
            eprintln!("mofact: {e}; using built-in content");
            Sim::new()
        }
    };

    let sdl = sdl3::init().map_err(|e| e.to_string())?;
    let video = sdl.video().map_err(|e| e.to_string())?;
    let mut events = sdl.event_pump().map_err(|e| e.to_string())?;
    let mut app = App::new(&video, sim)?;
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
