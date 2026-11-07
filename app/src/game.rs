//! The game itself, the same on every platform: the simulation, the camera
//! and build tools, the HUD and the renderer. A platform shell (SDL on
//! desktop and phones, a canvas in the browser) owns the window, turns its
//! events into [`Pointer`]s and key names, and calls [`Game::frame`] once a
//! frame. Nothing here knows which shell it runs in.

use crate::built::Built;
use crate::guide::{Coach, Guide};
use crate::ore::OreMap;
use crate::sound::Sound;
use mofact_core::content::Content;
use mofact_core::geom::TilePos;
use mofact_input::{CameraControl, Controls, Ground, Pointer, Tool};
use mofact_render::{Camera, Frame, Gpu, Look, Overlay, Renderer, Scene, Target};
use mofact_sim::{Sim, SimHandle};
use mofact_ui::{Action, Hud, View};
use web_time::Instant;

pub struct Game {
    /// None while the platform has taken the window away.
    pub target: Option<Target>,
    renderer: Renderer,
    pub gpu: Gpu,
    pub sim: SimHandle,
    look: Look,
    scene: Scene,
    overlay: Overlay,
    pub hud: Hud,
    /// A letter key for each building the toolbar has no number for.
    hotkeys: Vec<(char, mofact_core::ids::BuildingId)>,
    content: Content,
    ore: OreMap,
    pub controls: Controls,
    built: Built,
    /// When the latest snapshot arrived, for smoothing between ticks.
    latest: (u64, Instant),
    /// What it sounds like; the platform plays [`Sound::audio`].
    pub sound: Sound,
    /// When the game started showing, the clock machine animations run on.
    started: Instant,
    /// The edges of the screen the display covers, in pixels (top, right,
    /// bottom, left): the platform sets it, and the HUD keeps clear of it.
    pub safe: [f32; 4],
    /// The first-hour coach.
    guide: Guide,
}

impl Game {
    pub fn new(
        gpu: Gpu,
        target: Target,
        content: Content,
        mut sim: Sim,
        ore: OreMap,
        camera: Camera,
    ) -> Self {
        // Drills mine the same generated map the player sees.
        sim.set_world(ore.seed());
        let look = Look::new(&content);
        let (w, h) = target.size();
        let controls = Controls::new(CameraControl::new(
            camera.center,
            camera.pixels_per_tile,
            [w as f32, h as f32],
        ));
        Self {
            target: Some(target),
            renderer: Renderer::new(&gpu, look.atlases()),
            gpu,
            sim: SimHandle::spawn(sim),
            overlay: Overlay::new(&look),
            look,
            scene: Scene::default(),
            hud: Hud::new(),
            hotkeys: mofact_ui::hotkeys(&content),
            built: {
                let mut built = Built::new(&content);
                built.world = Some(ore.gen());
                built
            },
            sound: Sound::new(&content),
            started: Instant::now(),
            safe: [0.0; 4],
            guide: Guide::default(),
            content,
            ore,
            controls,
            latest: (0, Instant::now()),
        }
    }

    /// The window was resized to `w` × `h` pixels.
    pub fn resize(&mut self, w: u32, h: u32) {
        if let Some(t) = &mut self.target {
            t.resize(&self.gpu, w.max(1), h.max(1));
        }
        self.controls.camera.set_viewport(w as f32, h as f32);
    }

    /// One frame: run due ticks, answer finished commands, fire held
    /// presses, and draw at `size` pixels with `scale` pixels per point.
    pub fn frame(&mut self, size: (u32, u32), scale: f32) -> Frame {
        self.sim.pump();
        let results: Vec<_> = self.sim.results().collect();
        self.sound.results(&results);
        for result in results {
            if let mofact_core::command::CommandResult::Refused(why) = result {
                use mofact_core::command::CommandError as E;
                self.hud.toast(match why {
                    E::Blocked => "Something is in the way.",
                    E::CannotAfford => "Not enough plates in stock.",
                    E::Locked => "That isn't unlocked yet.",
                    E::NoSuchEntity => "That's already gone.",
                    E::UnknownContent => "That isn't in this game's data.",
                    E::FluidsMix => "That would mix two fluids.",
                    E::NoOre => "Drills only go on ore: tap a coloured ore tile.",
                    E::Terrain => "Nothing can be built on water or cliffs.",
                    E::Overgrown => "Trees in the way: drag over them with Remove to clear them.",
                });
            }
        }
        for action in self.hud.poll(Instant::now()) {
            self.act(action);
        }
        self.draw(size, scale)
    }

    fn draw(&mut self, (w, h): (u32, u32), scale: f32) -> Frame {
        let Some(target) = self.target.as_mut() else {
            return Frame::Skipped;
        };
        self.controls.camera.set_scale(scale);
        // Some platforms resize without telling us; follow the window every frame.
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
        // Wrapped at an hour, so the clock stays precise in an f32.
        let t = self.started.elapsed().as_secs_f32() % 3600.0;
        self.scene
            .animate(&self.look, atlases, &camera, target.size(), &latest, t);
        self.sound.frame(&latest, &camera, target.size());
        self.ore.update(&camera, target.size());
        self.scene.ore(
            &self.look,
            atlases,
            &camera,
            target.size(),
            self.ore.tiles(),
        );
        self.scene.decor(
            atlases,
            &camera,
            target.size(),
            self.ore
                .decor(&camera, target.size())
                .filter(|(t, _)| latest.cleared.binary_search(t).is_err()),
        );
        let ore = &self.ore;
        self.scene
            .terrain(&camera, target.size(), ore.version(), |t| ore.terrain(t));
        self.scene.time = self.started.elapsed().as_secs_f32() % 3600.0;
        let (coach, turn) = self.guide.update(
            &self.content,
            &latest,
            &self.ore,
            &self.controls.builder.tool,
        );
        if let Some(dir) = turn {
            self.controls.builder.dir = dir;
        }
        // The coach's line to build, faint, under what the player is placing.
        if let Some(Coach::Line(plan, ..)) = &coach {
            let built = |t: &TilePos| latest.entities.iter().any(|e| e.pos == *t);
            self.scene.ghosts(
                &self.look,
                atlases,
                plan.iter().flat_map(|s| {
                    s.tiles
                        .iter()
                        .filter(|t| !built(t))
                        .map(move |t| (&s.building, *t, s.dir, None))
                }),
            );
        }
        let ghosts = self.controls.builder.ghosts();
        let recipe = self.controls.builder.recipe.as_ref();
        self.scene.ghosts(
            &self.look,
            atlases,
            ghosts.iter().map(|g| (&g.building, g.tile, g.dir, recipe)),
        );
        self.overlay.clear();
        self.controls.slop = 12.0 * scale.max(0.5);
        // What a remove drag has marked, in red, before it goes.
        for id in self.controls.builder.marked() {
            let Some(e) = latest.entities.iter().find(|e| e.id == *id) else {
                continue;
            };
            let size = self
                .content
                .buildings
                .iter()
                .find(|b| b.id == e.building)
                .map_or((1, 1), |b| b.size);
            let ppt = camera.pixels_per_tile;
            let (sw, sh) = target.size();
            let x = (e.pos.x as f32 - camera.center[0]) * ppt + sw as f32 / 2.0;
            let y = (e.pos.y as f32 - camera.center[1]) * ppt + sh as f32 / 2.0;
            let (w, h) = (f32::from(size.0) * ppt, f32::from(size.1) * ppt);
            self.overlay
                .panel([x, y], [w, h], ppt * 0.12, [0xff, 0x5d, 0x5d, 0x80]);
        }
        // Trees it has marked to clear.
        for t in self.controls.builder.marked_trees() {
            let ppt = camera.pixels_per_tile;
            let (sw, sh) = target.size();
            let x = (t.x as f32 - camera.center[0]) * ppt + sw as f32 / 2.0;
            let y = (t.y as f32 - camera.center[1]) * ppt + sh as f32 / 2.0;
            self.overlay
                .panel([x, y], [ppt, ppt], ppt * 0.3, [0xff, 0x5d, 0x5d, 0x70]);
        }
        let nudge = coach_marks(
            &mut self.overlay,
            coach.as_ref(),
            &camera,
            target.size(),
            scale,
            t,
        );
        let builder = &self.controls.builder;
        let view = View {
            size: target.size(),
            scale,
            content: &self.content,
            look: &self.look,
            snapshot: &latest,
            tool: &builder.tool,
            dir: builder.dir,
            recipe: builder.recipe.as_ref(),
            belt: &builder.belt,
            history: self.sim.history(),
            safe: self.safe,
            nudge,
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

    /// A key, by its name in lower case ("1", "escape", "t").
    ///
    /// Keys follow the toolbar: 1 to 5 pick hand, belt, drill, furnace and
    /// assembler, 6 opens the build sheet and 7 (or X) removes; T opens
    /// research; R rotates; M turns the sound off and on; Escape goes back
    /// to the hand. The shells send "undo" and "redo" for Ctrl+Z and
    /// Ctrl+Shift+Z (or Ctrl+Y). Every other building has a letter of its
    /// own (see [`mofact_ui::hotkeys`]).
    pub fn key(&mut self, name: &str) {
        let tool = match name {
            "1" | "escape" => Tool::Hand,
            "2" => Tool::Belt,
            "3" => Tool::Place("drill".into()),
            "4" => Tool::Place("furnace".into()),
            "5" => Tool::Place("assembler".into()),
            "6" => return self.hud.toggle_build_sheet(),
            "t" => return self.hud.toggle_research(),
            "7" | "x" => Tool::Remove,
            "r" => return self.controls.builder.rotate(),
            "m" => {
                let muted = !self.hud.muted();
                self.hud.set_muted(muted);
                return self.sound.set_muted(muted);
            }
            "undo" => return self.sim.undo(),
            "redo" => return self.sim.redo(),
            other => {
                let mut letters = other.chars();
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
            Action::Undo => self.sim.undo(),
            Action::Redo => self.sim.redo(),
            // With the hand, a tap inspects what's there or closes the sheet.
            // With a building in hand, a tap on something already built
            // inspects it instead of trying to build on top of it.
            Action::Inspect(at) => {
                let hand = self.controls.builder.tool == Tool::Hand;
                let placing = matches!(self.controls.builder.tool, Tool::Place(_));
                if !hand && !placing {
                    return;
                }
                let [x, y] = self.controls.camera.world_at(at);
                let tile = mofact_core::geom::TilePos::new(x.floor() as i32, y.floor() as i32);
                self.built.update(&self.sim.latest());
                let found = self.built.at(tile).map(|o| o.id);
                if hand || found.is_some() {
                    self.hud.select(found);
                }
            }
            Action::Cancel => self.controls.builder.cancel(),
            // A problem from the list: look at it, with the hand, so its
            // inspect sheet (opened by the HUD) says what's wrong.
            Action::Mute(muted) => self.sound.set_muted(muted),
            Action::Focus(at) => {
                self.controls.camera.center = at;
                self.controls.builder.set_tool(Tool::Hand);
            }
        }
    }

    /// A pointer event goes to the HUD first, then to the camera and build tools.
    pub fn handle(&mut self, p: Pointer) {
        let (taken, actions) = self.hud.pointer(p);
        if !actions.is_empty() && taken {
            self.sound.tap();
        }
        for action in actions {
            self.act(action);
        }
        if !taken {
            self.built.update(&self.sim.latest());
            // Everything one gesture builds is one undo step.
            self.sim.send_all(self.controls.handle(p, &self.built));
        }
    }
}

/// A camera on the middle of everything built, at the medium zoom band.
pub fn framing(content: &Content, sim: &Sim) -> Camera {
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

/// A new game: the starter factory and its ore field.
pub fn new_game(content: &Content, seed: u64) -> Result<(Sim, OreMap), String> {
    let scenario = mofact_headless::parse(crate::bundled::STARTER)?;
    let sim = mofact_headless::build(content, &scenario)?;
    Ok((sim, ore_for(content, &scenario, seed)))
}

/// The ore field for `scenario`: the ore it places plus the seeded field.
pub fn ore_for(content: &Content, scenario: &mofact_headless::Scenario, seed: u64) -> OreMap {
    let placed = scenario
        .ore
        .iter()
        .map(|(x, y, item)| (mofact_core::geom::TilePos::new(*x, *y), item.clone()))
        .collect();
    OreMap::new(content, seed, placed)
}

/// Draw what the coach shows on the map, and return the tool it wants
/// the player to pick next.
fn coach_marks(
    o: &mut Overlay,
    coach: Option<&Coach>,
    camera: &Camera,
    (sw, sh): (u32, u32),
    scale: f32,
    t: f32,
) -> Option<Tool> {
    let ppt = camera.pixels_per_tile;
    let at = |p: TilePos| {
        [
            (p.x as f32 - camera.center[0]) * ppt + sw as f32 / 2.0,
            (p.y as f32 - camera.center[1]) * ppt + sh as f32 / 2.0,
        ]
    };
    let px = 13.0 * scale;
    // A label centred on `mid`, its bottom edge at `y` (or top edge, `below`).
    let label = |o: &mut Overlay, text: &str, mid: f32, y: f32, below: bool, ink: [u8; 4]| {
        let (pad, h) = (8.0 * scale, 24.0 * scale);
        let w = o.measure(text, px) + 2.0 * pad;
        let x = (mid - w / 2.0).clamp(4.0 * scale, sw as f32 - w - 4.0 * scale);
        let top = if below { y } else { y - h };
        o.panel([x, top], [w, h], h / 2.0, [0x1d, 0x23, 0x2a, 0xf0]);
        o.text(text, [x + pad, top + (h + o.cap_height(px)) / 2.0], px, ink);
    };
    let accent = [0xff, 0xd1, 0x66, 0xff];
    match coach? {
        Coach::Labels(labels) => {
            // Only while the buildings are big enough to read beside.
            if ppt < 20.0 * scale {
                return None;
            }
            for (pos, (w, h), text, below) in labels {
                let p = at(*pos);
                let mid = p[0] + f32::from(*w) * ppt / 2.0;
                let y = if *below {
                    p[1] + f32::from(*h) * ppt + 4.0 * scale
                } else {
                    p[1] - 4.0 * scale
                };
                label(o, text, mid, y, *below, [0xe9, 0xed, 0xf0, 0xff]);
            }
            None
        }
        Coach::Line(plan, next, below) => {
            let a = 0x50 + (((t * 4.0).sin() * 0.5 + 0.5) * 90.0) as u8;
            for (i, step) in plan.iter().enumerate() {
                for tile in &step.tiles {
                    let p = at(*tile);
                    let ink = if i == *next {
                        [0xff, 0xd1, 0x66, a]
                    } else {
                        [0xff, 0xd1, 0x66, 0x18]
                    };
                    o.panel(p, [ppt, ppt], ppt * 0.18, ink);
                }
            }
            let step = &plan[*next];
            let first = at(step.tiles[0]);
            let last = at(*step.tiles.last().unwrap_or(&step.tiles[0]));
            let mid = (first[0] + last[0]) / 2.0 + ppt / 2.0;
            let y = if *below {
                first[1].max(last[1]) + ppt + 6.0 * scale
            } else {
                first[1].min(last[1]) - 6.0 * scale
            };
            label(o, step.say(), mid, y, *below, accent);
            Some(step.tool())
        }
    }
}
