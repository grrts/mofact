//! The browser shell: the same [`Game`] the phone and desktop apps run,
//! drawn with WebGL2 on a canvas. This is what the playable link serves.
//!
//! The browser gives the game no thread of its own, so the simulation ticks
//! on the frame loop (see `mofact_sim::LocalSim`), still at 60 ticks a
//! second of real time. Pointer events become the same [`Pointer`]s SDL's
//! touch and mouse events do, keys go by name, and the game autosaves to
//! `localStorage` every 30 seconds and whenever the tab is hidden or closed.
//! `?new` in the address starts a new game.
//!
//! Sound plays through Web Audio, which a page may only start from a tap or
//! a key, so the first of those starts it.

#![cfg(target_arch = "wasm32")]

use mofact::game::{self, Game};
use mofact_input::{Pointer, MOUSE, MOUSE_PAN};
use mofact_render::{Gpu, Target};
use mofact_sim::Sim;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{HtmlCanvasElement, KeyboardEvent, PointerEvent, WheelEvent};

const SAVE_KEY: &str = "mofact-save";
/// Milliseconds between autosaves.
const SAVE_EVERY: f64 = 30_000.0;

/// The frame callback, which queues itself again each frame.
type FrameFn = Closure<dyn FnMut()>;

struct Shell {
    game: Game,
    canvas: HtmlCanvasElement,
    window: web_sys::Window,
    /// The mouse button held, as a pointer id.
    mouse: u64,
    /// When the game last saved, in page milliseconds.
    saved: f64,
    /// Where the sound plays, if the browser has Web Audio.
    sound: Option<web_sys::AudioContext>,
}

impl Shell {
    /// The canvas size in device pixels, and device pixels per CSS pixel.
    fn size(&self) -> ((u32, u32), f32) {
        let dpr = self.window.device_pixel_ratio();
        let w = (f64::from(self.canvas.client_width()) * dpr)
            .round()
            .max(1.0) as u32;
        let h = (f64::from(self.canvas.client_height()) * dpr)
            .round()
            .max(1.0) as u32;
        ((w, h), dpr as f32)
    }

    fn frame(&mut self) {
        let (size, scale) = self.size();
        if (self.canvas.width(), self.canvas.height()) != size {
            self.canvas.set_width(size.0);
            self.canvas.set_height(size.1);
        }
        self.game.frame(size, scale);
        if self.now() - self.saved >= SAVE_EVERY {
            self.save();
        }
    }

    /// Start or stop the sound. Starting only works from a tap or a key.
    fn hush(&self, quiet: bool) {
        if let Some(ctx) = &self.sound {
            let running = ctx.state() == web_sys::AudioContextState::Running;
            if quiet && running {
                let _ = ctx.suspend();
            } else if !quiet && !running {
                let _ = ctx.resume();
            }
        }
    }

    fn now(&self) -> f64 {
        self.window.performance().map_or(0.0, |p| p.now())
    }

    fn save(&mut self) {
        self.saved = self.now();
        let Ok(bytes) = self.game.sim.save().try_recv() else {
            return;
        };
        if let Ok(Some(storage)) = self.window.local_storage() {
            if storage.set_item(SAVE_KEY, &hex(&bytes)).is_err() {
                log("mofact: the browser refused the save (storage full?)");
            }
        }
    }

    /// A pointer event's position in canvas pixels.
    fn at(&self, e: &web_sys::MouseEvent) -> (f32, f32) {
        let rect = self.canvas.get_bounding_client_rect();
        let sx = f64::from(self.canvas.width()) / rect.width().max(1.0);
        let sy = f64::from(self.canvas.height()) / rect.height().max(1.0);
        (
            ((f64::from(e.client_x()) - rect.left()) * sx) as f32,
            ((f64::from(e.client_y()) - rect.top()) * sy) as f32,
        )
    }

    fn pointer(&mut self, kind: &str, e: &PointerEvent) -> Option<Pointer> {
        let mouse = e.pointer_type() == "mouse";
        let id = if mouse {
            self.mouse
        } else {
            e.pointer_id() as u32 as u64
        };
        let (x, y) = self.at(e);
        Some(match kind {
            "pointerdown" => {
                let id = if mouse {
                    self.mouse = if e.button() == 0 { MOUSE } else { MOUSE_PAN };
                    self.mouse
                } else {
                    id
                };
                Pointer::Down { id, x, y }
            }
            "pointermove" => Pointer::Move { id, x, y },
            "pointerup" | "pointercancel" => Pointer::Up { id },
            _ => return None,
        })
    }
}

fn log(text: &str) {
    web_sys::console::log_1(&text.into());
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(DIGITS[usize::from(b >> 4)] as char);
        s.push(DIGITS[usize::from(b & 15)] as char);
    }
    s
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    let digit = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    s.as_bytes()
        .chunks(2)
        .map(|p| match p {
            [a, b] => Some(digit(*a)? << 4 | digit(*b)?),
            _ => None,
        })
        .collect()
}

/// The saved game, unless there is none, it is damaged, or the address asks for a new one.
fn saved(window: &web_sys::Window, content: &mofact_core::content::Content) -> Option<Sim> {
    let search = window.location().search().unwrap_or_default();
    if search.contains("new") {
        return None;
    }
    let text = window.local_storage().ok()??.get_item(SAVE_KEY).ok()??;
    match Sim::load(content, &unhex(&text)?) {
        Ok((sim, warnings)) => {
            for w in warnings {
                log(&format!("mofact: save: {w}"));
            }
            Some(sim)
        }
        Err(e) => {
            log(&format!(
                "mofact: the save didn't load ({e}); starting a new game"
            ));
            None
        }
    }
}

/// The game's sound through Web Audio, at the browser's own sample rate.
/// The page's thread fills each block as it is due.
fn speaker(game: &mut Game) -> Result<web_sys::AudioContext, JsValue> {
    let ctx = web_sys::AudioContext::new()?;
    let audio = mofact_audio::Audio::new(ctx.sample_rate() as u32);
    game.sound.use_audio(audio.clone());
    let node = ctx.create_script_processor_with_buffer_size_and_number_of_input_channels_and_number_of_output_channels(2048, 0, 2)?;
    let mut buf = Vec::new();
    let mut side = Vec::new();
    let fill = Closure::<dyn FnMut(web_sys::AudioProcessingEvent)>::new(
        move |e: web_sys::AudioProcessingEvent| {
            let Ok(out) = e.output_buffer() else {
                return;
            };
            let n = out.length() as usize;
            buf.resize(n * 2, 0.0);
            audio.render(&mut buf);
            for channel in 0..2 {
                side.clear();
                side.extend(buf.chunks(2).map(|f| f[channel]));
                let _ = out.copy_to_channel(&side, channel as i32);
            }
        },
    );
    node.set_onaudioprocess(Some(fill.as_ref().unchecked_ref()));
    fill.forget();
    node.connect_with_audio_node(&ctx.destination())?;
    Ok(ctx)
}

fn js(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}

/// Start the game on the canvas with id `canvas`. Resolves once the first
/// frame is queued; rejects with the reason if the game can't start.
#[wasm_bindgen]
pub async fn start(canvas: String) -> Result<(), JsValue> {
    // A panic would otherwise surface only as "unreachable"; say what it was.
    std::panic::set_hook(Box::new(|info| {
        web_sys::console::error_1(&format!("mofact panicked: {info}").into());
    }));
    let window = web_sys::window().ok_or("no window")?;
    let document = window.document().ok_or("no document")?;
    let canvas: HtmlCanvasElement = document
        .get_element_by_id(&canvas)
        .ok_or("no canvas")?
        .dyn_into()?;

    let content = mofact::content().map_err(js)?;
    let (fresh, ore) = game::new_game(&content, 42).map_err(js)?;
    let sim = saved(&window, &content).unwrap_or(fresh);
    let camera = game::framing(&content, &sim);

    let mut desc = mofact_render::wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = mofact_render::wgpu::Backends::GL;
    let instance = mofact_render::wgpu::Instance::new(desc);
    let surface = instance
        .create_surface(mofact_render::wgpu::SurfaceTarget::Canvas(canvas.clone()))
        .map_err(js)?;
    let gpu = Gpu::open(instance, &surface).await.map_err(js)?;
    let dpr = window.device_pixel_ratio();
    let (w, h) = (
        (f64::from(canvas.client_width()) * dpr).max(1.0) as u32,
        (f64::from(canvas.client_height()) * dpr).max(1.0) as u32,
    );
    canvas.set_width(w);
    canvas.set_height(h);
    let target = Target::new(&gpu, surface, w, h).map_err(js)?;
    let info = gpu.info();
    log(&format!(
        "mofact {}: {} ({:?}), surface {:?}",
        env!("CARGO_PKG_VERSION"),
        info.name,
        info.backend,
        target.format()
    ));

    let shell = Rc::new(RefCell::new(Shell {
        game: Game::new(gpu, target, content, sim, ore, camera),
        canvas: canvas.clone(),
        window: window.clone(),
        mouse: MOUSE,
        saved: 0.0,
        sound: None,
    }));
    {
        let mut s = shell.borrow_mut();
        s.saved = s.now();
        match speaker(&mut s.game) {
            Ok(ctx) => s.sound = Some(ctx),
            Err(e) => log(&format!("mofact: no sound ({e:?})")),
        }
    }

    for kind in ["pointerdown", "pointermove", "pointerup", "pointercancel"] {
        let s = shell.clone();
        listen(&canvas, kind, move |e: PointerEvent| {
            e.prevent_default();
            let mut s = s.borrow_mut();
            if kind == "pointerdown" {
                let _ = s.canvas.set_pointer_capture(e.pointer_id());
                s.hush(false);
            }
            if let Some(p) = s.pointer(kind, &e) {
                s.game.handle(p);
            }
        })?;
    }
    {
        let s = shell.clone();
        listen(&canvas, "wheel", move |e: WheelEvent| {
            e.prevent_default();
            let mut s = s.borrow_mut();
            let (x, y) = s.at(&e);
            // Pixels, lines or pages: about one step per notch either way.
            let per_step = match e.delta_mode() {
                0 => 100.0,
                1 => 3.0,
                _ => 1.0,
            };
            let steps = (-e.delta_y() / per_step).clamp(-3.0, 3.0) as f32;
            s.game.handle(Pointer::Wheel { steps, x, y });
        })?;
    }
    listen(&canvas, "contextmenu", |e: web_sys::Event| {
        e.prevent_default()
    })?;
    {
        let s = shell.clone();
        listen(&window, "keydown", move |e: KeyboardEvent| {
            let key = e.key().to_lowercase();
            if (e.ctrl_key() || e.meta_key()) && !e.alt_key() && (key == "z" || key == "y") {
                e.prevent_default();
                let redo = key == "y" || e.shift_key();
                s.borrow_mut().game.key(if redo { "redo" } else { "undo" });
                return;
            }
            if e.ctrl_key() || e.meta_key() || e.alt_key() || e.repeat() {
                return;
            }
            let mut s = s.borrow_mut();
            s.hush(false);
            s.game.key(&e.key().to_lowercase());
        })?;
    }
    {
        let s = shell.clone();
        let doc = document.clone();
        listen(&document, "visibilitychange", move |_: web_sys::Event| {
            let mut s = s.borrow_mut();
            let hidden = doc.visibility_state() == web_sys::VisibilityState::Hidden;
            if hidden {
                s.save();
            }
            s.hush(hidden);
            s.game.sim.set_paused(hidden);
        })?;
    }
    {
        let s = shell.clone();
        listen(&window, "pagehide", move |_: web_sys::Event| {
            s.borrow_mut().save()
        })?;
    }

    // The frame loop: each frame asks for the next.
    let tick: Rc<RefCell<Option<FrameFn>>> = Rc::new(RefCell::new(None));
    let next = tick.clone();
    let win = window.clone();
    *tick.borrow_mut() = Some(Closure::new(move || {
        shell.borrow_mut().frame();
        if let Some(f) = next.borrow().as_ref() {
            let _ = win.request_animation_frame(f.as_ref().unchecked_ref());
        }
    }));
    if let Some(f) = tick.borrow().as_ref() {
        window.request_animation_frame(f.as_ref().unchecked_ref())?;
    }
    Ok(())
}

/// Call `f` for every `kind` event on `target`, for the life of the page.
fn listen<E: JsCast + 'static>(
    target: &web_sys::EventTarget,
    kind: &str,
    mut f: impl FnMut(E) + 'static,
) -> Result<(), JsValue> {
    let closure = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
        if let Ok(e) = e.dyn_into::<E>() {
            f(e);
        }
    });
    let options = web_sys::AddEventListenerOptions::new();
    options.set_passive(false);
    target.add_event_listener_with_callback_and_add_event_listener_options(
        kind,
        closure.as_ref().unchecked_ref(),
        &options,
    )?;
    // The page owns its listeners until it closes.
    closure.forget();
    Ok(())
}
