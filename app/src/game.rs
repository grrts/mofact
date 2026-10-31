//! The game itself, the same on every platform: the simulation, the camera
//! and build tools, the HUD and the renderer. A platform shell (SDL on
//! desktop and phones, a canvas in the browser) owns the window, turns its
//! events into [`Pointer`]s and key names, and calls [`Game::frame`] once a
//! frame. Nothing here knows which shell it runs in.

use crate::built::Built;
use crate::ore::OreMap;
use mofact_core::content::Content;
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
}

impl Game {
    pub fn new(
        gpu: Gpu,
        target: Target,
        content: Content,
        sim: Sim,
        ore: OreMap,
        camera: Camera,
    ) -> Self {
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
            built: Built::new(&content),
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
        for result in self.sim.results() {
            if let mofact_core::command::CommandResult::Refused(why) = result {
                use mofact_core::command::CommandError as E;
                self.hud.toast(match why {
                    E::Blocked => "Something is in the way.",
                    E::CannotAfford => "Not enough plates in stock.",
                    E::Locked => "That isn't unlocked yet.",
                    E::NoSuchEntity => "That's already gone.",
                    E::UnknownContent => "That isn't in this game's data.",
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
            scale,
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

    /// A key, by its name in lower case ("1", "escape", "t").
    ///
    /// Keys follow the toolbar: 1 to 5 pick hand, belt, drill, furnace and
    /// assembler, 6 opens the build sheet and 7 (or X) removes; T opens
    /// research; R rotates; Escape goes back to the hand. Every other
    /// building has a letter of its own (see [`mofact_ui::hotkeys`]).
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
    pub fn handle(&mut self, p: Pointer) {
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
