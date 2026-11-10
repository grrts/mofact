//! The game itself, the same on every platform: the simulation, the camera
//! and build tools, the HUD and the renderer. A platform shell (SDL on
//! desktop and phones, a canvas in the browser) owns the window, turns its
//! events into [`Pointer`]s and key names, and calls [`Game::frame`] once a
//! frame. Nothing here knows which shell it runs in.

use crate::built::Built;
use crate::guide::{Coach, Guide};
use crate::ore::OreMap;
use crate::shop::Purchase;
use crate::slots::{Request, Store};
use crate::sound::Sound;
use mofact_core::content::Content;
use mofact_core::geom::TilePos;
use mofact_input::{Blueprint, CameraControl, Controls, Ground, Pointer, Tool};
use mofact_render::{Camera, Frame, Gpu, Look, Overlay, Renderer, Scene, Target};
use mofact_sim::{Sim, SimHandle};
use mofact_ui::{tr, trf, Action, Hud, SlotView, StoreView, View};
use web_time::{Duration, Instant};

/// Frame pacing (T66): 60 fps while playing, 30 in low-power mode or once
/// nothing has been touched for [`IDLE`]. A frame is drawn once at least
/// this long has passed since the last, less a little for timer jitter.
const FAST_FRAME: Duration = Duration::from_micros(15_000);
const SLOW_FRAME: Duration = Duration::from_micros(31_000);
const IDLE: Duration = Duration::from_secs(20);

/// Whether a frame is due at `now`, the last having been drawn at `drawn`.
fn due(drawn: Option<Instant>, now: Instant, slow: bool) -> bool {
    let gap = if slow { SLOW_FRAME } else { FAST_FRAME };
    drawn.is_none_or(|t| now.duration_since(t) >= gap)
}

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
    /// The content with its names in the player's language, for the HUD,
    /// and the language it is in.
    shown: Content,
    shown_lang: mofact_ui::i18n::Lang,
    ore: OreMap,
    pub controls: Controls,
    built: Built,
    /// When the latest snapshot arrived, for smoothing between ticks.
    latest: (u64, Instant),
    /// Drills mined out at the last snapshot, to say so when one more
    /// runs dry; none until the first snapshot, so a loaded game is quiet.
    dry: Option<usize>,
    /// What it sounds like; the platform plays [`Sound::audio`].
    pub sound: Sound,
    /// When the game started showing, the clock machine animations run on.
    started: Instant,
    /// The edges of the screen the display covers, in pixels (top, right,
    /// bottom, left): the platform sets it, and the HUD keeps clear of it.
    pub safe: [f32; 4],
    /// The first-hour coach.
    guide: Guide,
    /// The zoom to go back to when the map closes, in points a tile.
    before_map: Option<f32>,
    /// When the last frame was drawn, and when the player last touched or
    /// pressed anything: frames are paced from these (T66).
    drawn: Option<Instant>,
    touched: Instant,
    /// What the game menu asked of the save slots, for [`Game::serve`],
    /// and the slots as it lists them.
    requests: Vec<Request>,
    slots: Vec<SlotView>,
    /// The full game's purchase; none where nothing is sold, which plays
    /// the full game (tests, tools).
    purchase: Option<Purchase>,
    store: StoreView,
    /// What the phone should feel this frame, for the platform to play.
    haptics: crate::haptics::Haptics,
    pub haptic: Option<crate::haptics::Haptic>,
    /// Last time's crash report, until the player copies it.
    crash: Option<String>,
    /// Text for the platform to put on the clipboard.
    pub clipboard: Option<String>,
    /// The blueprint library (T51), and how many times it has changed, so
    /// the platform knows when to keep it again.
    blueprints: Vec<Blueprint>,
    blueprints_rev: u64,
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
        // Drills mine the same generated map the player sees: a save's own
        // map if it brought one, otherwise the one given.
        let ore = match sim.world_seed() {
            Some((seed, preset)) if (seed, preset) != (ore.seed(), ore.gen().preset) => {
                ore.regen(&content, seed, preset)
            }
            _ => ore,
        };
        sim.set_world_gen(ore.seed(), ore.gen().preset);
        let look = Look::new(&content);
        let (w, h) = target.size();
        let mut controls = Controls::new(CameraControl::new(
            camera.center,
            camera.pixels_per_tile,
            [w as f32, h as f32],
        ));
        // Track is drawn along a drag; everything else is placed by a tap.
        controls.builder.lines = content
            .buildings
            .iter()
            .filter(|b| b.role == mofact_core::content::BuildingRole::Rail)
            .map(|b| b.id.clone())
            .collect();
        controls.builder.on_track = content
            .buildings
            .iter()
            .filter(|b| {
                use mofact_core::content::BuildingRole as R;
                matches!(b.role, R::RailSignal | R::TrainStop | R::Locomotive { .. })
            })
            .map(|b| b.id.clone())
            .collect();
        Self {
            target: Some(target),
            renderer: Renderer::new(&gpu, look.atlases()),
            gpu,
            sim: SimHandle::spawn(sim),
            overlay: Overlay::new(&look),
            look,
            scene: Scene::default(),
            // The game opens on the start menu, over the factory it loaded.
            hud: {
                let mut hud = Hud::new();
                hud.open_title();
                hud
            },
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
            before_map: None,
            drawn: None,
            touched: Instant::now(),
            // List the slots at once, for the start menu's new game.
            requests: vec![Request::Refresh],
            slots: Vec::new(),
            purchase: None,
            store: StoreView::default(),
            haptics: Default::default(),
            haptic: None,
            crash: None,
            clipboard: None,
            blueprints: Vec::new(),
            blueprints_rev: 0,
            shown: content.clone(),
            shown_lang: mofact_ui::i18n::Lang::En,
            content,
            ore,
            controls,
            latest: (0, Instant::now()),
            dry: None,
        }
    }

    /// The blueprint library as text, to keep on the device, and a count
    /// that goes up each time it changes.
    pub fn blueprints(&self) -> (String, u64) {
        (
            mofact_input::blueprint::library_to_text(&self.blueprints),
            self.blueprints_rev,
        )
    }

    /// The blueprint library kept on the device, as a platform reads it at start.
    pub fn set_blueprints(&mut self, text: &str) {
        self.blueprints = mofact_input::blueprint::library_from_text(text);
    }

    /// The selection as a blueprint, and where its top-left corner is.
    fn capture(&self) -> Option<(Blueprint, TilePos)> {
        let latest = self.sim.latest();
        let mut picked: Vec<_> = latest
            .entities
            .iter()
            .filter(|e| self.controls.builder.selection.contains(&e.id))
            .collect();
        picked.sort_by_key(|e| e.id);
        let corner = TilePos::new(
            picked.iter().map(|e| e.pos.x).min()?,
            picked.iter().map(|e| e.pos.y).min()?,
        );
        let size = |e: &&mofact_core::snapshot::EntityView| {
            self.content
                .buildings
                .iter()
                .find(|b| b.id == e.building)
                .map_or(1, |b| b.size.0.max(b.size.1))
        };
        let bp = Blueprint::capture(
            mofact_input::blueprint::next_name(&self.blueprints, tr("blueprint.name")),
            picked
                .iter()
                .map(|e| (e.building.clone(), e.pos, size(e), e.dir, e.recipe.clone())),
        );
        Some((bp, corner))
    }

    /// Whether drone ports reach the middle of every building in `commands`
    /// (T50), so a paste can be planned for drones to build.
    fn drones_cover(&self, commands: &[mofact_core::command::Command]) -> bool {
        use mofact_core::command::Command;
        use mofact_core::content::BuildingRole;
        let latest = self.sim.latest();
        let size = |b: &mofact_core::ids::BuildingId| {
            self.content
                .buildings
                .iter()
                .find(|d| d.id == *b)
                .map(|d| (d.size, &d.role))
        };
        let ports: Vec<([f32; 2], f32)> = latest
            .entities
            .iter()
            .filter_map(|e| match size(&e.building)? {
                ((w, h), BuildingRole::DronePort { reach, .. }) => Some((
                    [
                        e.pos.x as f32 + f32::from(w) / 2.0,
                        e.pos.y as f32 + f32::from(h) / 2.0,
                    ],
                    f32::from(*reach),
                )),
                _ => None,
            })
            .collect();
        !ports.is_empty()
            && commands.iter().all(|c| match c {
                Command::Place { building, pos, .. } => {
                    let (w, h) = size(building).map_or((1, 1), |s| s.0);
                    let m = [
                        pos.x as f32 + f32::from(w) / 2.0,
                        pos.y as f32 + f32::from(h) / 2.0,
                    ];
                    ports
                        .iter()
                        .any(|(p, r)| (p[0] - m[0]).abs() <= *r && (p[1] - m[1]).abs() <= *r)
                }
                _ => false,
            })
    }

    /// The tile a blueprint's corner goes on to sit in the middle of the screen.
    fn centred(&self, bp: &Blueprint) -> TilePos {
        let [x, y] = self.controls.camera.center;
        TilePos::new(
            (x - bp.size.0 as f32 / 2.0).round() as i32,
            (y - bp.size.1 as f32 / 2.0).round() as i32,
        )
    }

    /// The language the player chose (T65): the device's, or one of their own.
    pub fn language(&self) -> mofact_ui::i18n::Choice {
        self.hud.language()
    }

    /// Choose the language, as a saved setting does at start.
    pub fn set_language(&mut self, choice: mofact_ui::i18n::Choice) {
        self.hud.set_language(choice);
    }

    /// Tell the game the device's language, from its preferred locales.
    pub fn set_device_lang(&mut self, lang: mofact_ui::i18n::Lang) {
        self.hud.set_device_lang(lang);
    }

    /// Whether low-power mode is on (T66).
    pub fn low_power(&self) -> bool {
        self.hud.low_power()
    }

    /// Turn low-power mode on or off, as a platform does from a saved setting.
    pub fn set_low_power(&mut self, on: bool) {
        self.hud.set_low_power(on);
        self.scene.low_power = on;
    }

    /// Whether colour-blind mode is on (T58).
    pub fn colour_blind(&self) -> bool {
        self.hud.colour_blind()
    }

    /// Turn colour-blind mode on or off, as a platform does from a saved setting.
    pub fn set_colour_blind(&mut self, on: bool) {
        self.hud.set_colour_blind(on);
        self.use_colour_blind(on);
    }

    /// Whether large text is on.
    pub fn large_text(&self) -> bool {
        self.hud.large_text()
    }

    /// Turn large text on or off, as a platform does from a saved setting.
    pub fn set_large_text(&mut self, on: bool) {
        self.hud.set_large_text(on);
    }

    /// How loud the game is, in percent.
    pub fn volume(&self) -> u8 {
        self.hud.volume()
    }

    /// Set the volume, as a platform does from a saved setting.
    pub fn set_volume(&mut self, percent: u8) {
        self.hud.set_volume(percent);
        self.sound.set_volume(self.hud.volume());
    }

    /// Whether the land's ambient sound plays.
    pub fn ambience(&self) -> bool {
        self.hud.ambience()
    }

    /// Turn the land's ambient sound on or off, as a platform does from a saved setting.
    pub fn set_ambience(&mut self, on: bool) {
        self.hud.set_ambience(on);
        self.sound.set_ambience(on);
    }

    /// Whether the game may vibrate the phone.
    pub fn vibration(&self) -> bool {
        self.hud.vibration()
    }

    /// Turn vibration on or off, as a platform does from a saved setting.
    pub fn set_vibration(&mut self, on: bool) {
        self.hud.set_vibration(on);
    }

    /// Whether reduce motion is on.
    pub fn reduce_motion(&self) -> bool {
        self.hud.reduce_motion()
    }

    /// Turn reduce motion on or off, as a platform does from a saved setting.
    pub fn set_reduce_motion(&mut self, on: bool) {
        self.hud.set_reduce_motion(on);
        self.scene.still = on;
    }

    /// Paint everything again in the colour-blind look, or the usual one.
    /// The game crashed last time: say so, and offer the report in the menu.
    pub fn set_crash_report(&mut self, report: String) {
        self.crash = Some(report);
        self.hud.toast(tr("toast.crashed"));
    }

    /// Sell the full game through `shop`: the demo's gate holds until it
    /// is bought (T61).
    pub fn set_shop(&mut self, shop: Box<dyn crate::shop::Shop>) {
        let p = Purchase::new(shop);
        self.sim.set_demo(p.demo());
        self.store = p.view();
        self.purchase = Some(p);
    }

    fn use_colour_blind(&mut self, on: bool) {
        if self.look.colour_blind() == on {
            return;
        }
        self.look = Look::with_colour_blind(&self.content, on);
        self.renderer = Renderer::new(&self.gpu, self.look.atlases());
        self.overlay = Overlay::new(&self.look);
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
        // Low-power mode, or a while with no touch, draws at 30 fps.
        let now = Instant::now();
        let slow = self.hud.low_power() || now.duration_since(self.touched) >= IDLE;
        if !due(self.drawn, now, slow) {
            return Frame::Skipped;
        }
        self.drawn = Some(now);
        if let Some(p) = &mut self.purchase {
            if p.poll() {
                self.sim.set_demo(None);
                self.hud.toast(tr("toast.full_game_open"));
            }
            self.store = p.view();
        }
        self.sim.pump();
        let results: Vec<_> = self.sim.results().collect();
        self.sound.results(&results);
        self.haptic = self
            .haptics
            .frame(&results, &self.sim.latest())
            .filter(|_| self.hud.vibration());
        for result in results {
            if let mofact_core::command::CommandResult::Refused(why) = result {
                use mofact_core::command::CommandError as E;
                if why == E::CannotAfford {
                    let text = self.short_of();
                    self.hud.toast(&text);
                    continue;
                }
                self.hud.toast(tr(match why {
                    E::Blocked => "error.blocked",
                    E::CannotAfford => "error.cannot_afford",
                    E::Locked => "error.locked",
                    E::NoSuchEntity => "error.no_such_entity",
                    E::UnknownContent => "error.unknown_content",
                    E::FluidsMix => "error.fluids_mix",
                    E::NoOre => "error.no_ore",
                    E::Terrain => "error.terrain",
                    E::Overgrown => "error.overgrown",
                    E::FullGame => "error.full_game",
                }));
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
            let dry = latest
                .entities
                .iter()
                .filter(|e| e.ore_left == Some(0))
                .count();
            if self.dry.is_some_and(|was| dry > was) {
                self.hud.toast(tr("toast.mined_out"));
            }
            self.dry = Some(dry);
        }
        let alpha = (self.latest.1.elapsed().as_secs_f32() / mofact_core::TICK_SECONDS).min(1.0);
        let camera = Camera {
            center: self.controls.camera.center,
            pixels_per_tile: self.controls.camera.pixels_per_tile,
        };
        let atlases = self.renderer.atlases();
        self.scene.night = mofact_render::night_at(latest.tick);
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
        // The land in view, sampled on a coarse grid, sets the ambient beds.
        {
            let (lo, hi) = camera.visible(target.size().0, target.size().1);
            let stride = mofact_render::scene::map_stride(camera.pixels_per_tile);
            let ore = &self.ore;
            let samples = (0..12).flat_map(|j| {
                (0..8).filter_map(move |i| {
                    let x = lo[0] + (hi[0] - lo[0]) * (i as f32 + 0.5) / 8.0;
                    let y = lo[1] + (hi[1] - lo[1]) * (j as f32 + 0.5) / 12.0;
                    let t = mofact_core::geom::TilePos::new(
                        (x / stride as f32).floor() as i32,
                        (y / stride as f32).floor() as i32,
                    );
                    ore.terrain(t, stride).map(|(kind, _)| kind)
                })
            });
            self.sound
                .ground(samples, crate::worldmap::is_map(&camera, scale));
        }
        self.ore.update(&camera, target.size());
        self.scene.ore(
            &self.look,
            atlases,
            &camera,
            target.size(),
            self.ore
                .tiles()
                .filter(|(t, _)| latest.depleted.binary_search(t).is_err()),
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
            .terrain(&camera, target.size(), ore.version(), |t, s| {
                ore.terrain(t, s)
            });
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
        // What drones are yet to build (T50).
        self.scene.ghosts(
            &self.look,
            atlases,
            latest
                .ghosts
                .iter()
                .map(|g| (&g.building, g.pos, g.dir, g.recipe.as_ref())),
        );
        let ghosts = self.controls.builder.ghosts();
        self.scene.ghosts(
            &self.look,
            atlases,
            ghosts
                .iter()
                .map(|g| (&g.building, g.tile, g.dir, g.recipe.as_ref())),
        );
        self.overlay.clear();
        crate::fog::draw(&mut self.overlay, &camera, target.size(), &latest.explored);
        // The select tool's box, what it has selected, and where a
        // blueprint in hand is blocked (T51).
        {
            let ppt = camera.pixels_per_tile;
            let (sw, sh) = target.size();
            let at = |t: TilePos| {
                [
                    (t.x as f32 - camera.center[0]) * ppt + sw as f32 / 2.0,
                    (t.y as f32 - camera.center[1]) * ppt + sh as f32 / 2.0,
                ]
            };
            let [ar, ag, ab, _] = mofact_render::palette::ACCENT;
            for e in latest
                .entities
                .iter()
                .filter(|e| self.controls.builder.selection.contains(&e.id))
            {
                let s = self
                    .content
                    .buildings
                    .iter()
                    .find(|b| b.id == e.building)
                    .map_or(1, |b| b.size.0);
                let s = f32::from(s) * ppt;
                self.overlay
                    .panel(at(e.pos), [s, s], ppt * 0.12, [ar, ag, ab, 0x60]);
            }
            if let Some((corner, (w, h))) = self.controls.builder.selecting() {
                self.overlay.panel(
                    at(corner),
                    [w as f32 * ppt, h as f32 * ppt],
                    ppt * 0.2,
                    [ar, ag, ab, 0x38],
                );
            }
            // Ghosts waiting for drones, edged in blue; what drones are to
            // take down, edged in red (T50).
            let size_of = |b: &mofact_core::ids::BuildingId| {
                self.content
                    .buildings
                    .iter()
                    .find(|d| d.id == *b)
                    .map_or((1, 1), |d| d.size)
            };
            let [gr, gg, gb, _] = mofact_render::palette::GLASS;
            for g in &latest.ghosts {
                let (w, h) = size_of(&g.building);
                let a = if g.coming { 0x50 } else { 0x28 };
                self.overlay.panel(
                    at(g.pos),
                    [f32::from(w) * ppt, f32::from(h) * ppt],
                    ppt * 0.12,
                    [gr, gg, gb, a],
                );
            }
            for e in latest
                .entities
                .iter()
                .filter(|e| latest.marked.contains(&e.id))
            {
                let (w, h) = size_of(&e.building);
                self.overlay.panel(
                    at(e.pos),
                    [f32::from(w) * ppt, f32::from(h) * ppt],
                    ppt * 0.12,
                    [0xff, 0x5d, 0x5d, 0x50],
                );
            }
            self.built.update(&latest);
            for t in self.controls.builder.blocked(&self.built) {
                self.overlay
                    .panel(at(t), [ppt, ppt], ppt * 0.12, [0xff, 0x5d, 0x5d, 0x80]);
            }
        }
        // Drones in flight (T49), over everything on the ground: a shadow,
        // the drone, and what it carries hung under it.
        {
            let ppt = camera.pixels_per_tile;
            let (sw, sh) = target.size();
            let same = prev.drones.len() == latest.drones.len();
            for (i, d) in latest.drones.iter().enumerate() {
                let p = if same {
                    let a = prev.drones[i].at;
                    [
                        a[0] + (d.at[0] - a[0]) * alpha,
                        a[1] + (d.at[1] - a[1]) * alpha,
                    ]
                } else {
                    d.at
                };
                let x = (p[0] - camera.center[0]) * ppt + sw as f32 / 2.0;
                let y = (p[1] - camera.center[1]) * ppt + sh as f32 / 2.0;
                let s = ppt * 0.7;
                self.overlay.panel(
                    [x - s * 0.3 + ppt * 0.15, y - s * 0.15 + ppt * 0.3],
                    [s * 0.6, s * 0.3],
                    s * 0.15,
                    [0, 0, 0, 0x50],
                );
                if let Some((sprite, _)) = self.look.item(&mofact_core::ids::ItemId::from("drone"))
                {
                    self.overlay
                        .icon(sprite, [x - s / 2.0, y - s / 2.0], s, [255, 255, 255, 255]);
                }
                let load = d
                    .cargo
                    .as_ref()
                    .and_then(|c| self.look.item(c))
                    .map(|(sprite, _)| sprite.to_owned())
                    .or_else(|| {
                        let b = d.building.as_ref()?;
                        let sprite =
                            self.look
                                .sprite(b, mofact_core::geom::Dir::East, None, false)?;
                        Some(sprite.to_owned())
                    });
                if let Some(sprite) = load {
                    let c = s * 0.45;
                    self.overlay
                        .icon(&sprite, [x - c / 2.0, y + s * 0.2], c, [255, 255, 255, 255]);
                }
            }
        }
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
        // Far out, the world map: the factory as a schematic, the ore
        // patches as icons. Pins show at every zoom.
        let map = crate::worldmap::is_map(&camera, scale);
        self.hud.set_map(map);
        if map {
            crate::worldmap::factory(
                &mut self.overlay,
                &self.content,
                &self.look,
                &camera,
                target.size(),
                scale,
                &latest,
            );
            // Patches under the fog stay unknown.
            let mut patches = self.ore.patches(&camera, target.size());
            patches.retain(|(p, _)| crate::fog::charted(&latest.explored, p.center));
            crate::worldmap::patches(
                &mut self.overlay,
                &self.look,
                &camera,
                target.size(),
                scale,
                &patches,
            );
        }
        crate::worldmap::pins(
            &mut self.overlay,
            &camera,
            target.size(),
            scale,
            &latest.pins,
        );
        // Trees it has marked to clear.
        for t in self.controls.builder.marked_trees() {
            let ppt = camera.pixels_per_tile;
            let (sw, sh) = target.size();
            let x = (t.x as f32 - camera.center[0]) * ppt + sw as f32 / 2.0;
            let y = (t.y as f32 - camera.center[1]) * ppt + sh as f32 / 2.0;
            self.overlay
                .panel([x, y], [ppt, ppt], ppt * 0.3, [0xff, 0x5d, 0x5d, 0x70]);
        }
        power_marks(
            &mut self.overlay,
            &self.content,
            &latest,
            &camera,
            target.size(),
            scale,
            self.scene.low_power,
        );
        let panels = self.hud.panels();
        let nudge = coach_marks(
            &mut self.overlay,
            coach.as_ref(),
            &panels,
            &camera,
            target.size(),
            scale,
            t,
        );
        if self.shown_lang != mofact_ui::i18n::lang() {
            self.shown_lang = mofact_ui::i18n::lang();
            self.shown = mofact_ui::i18n::localize(&self.content);
        }
        let builder = &self.controls.builder;
        let view = View {
            size: target.size(),
            scale,
            content: &self.shown,
            look: &self.look,
            snapshot: &latest,
            tool: &builder.tool,
            dir: builder.dir,
            recipe: builder.recipe.as_ref(),
            belt: &builder.belt,
            history: self.sim.history(),
            safe: self.safe,
            nudge,
            slots: &self.slots,
            store: &self.store,
            crash: self.crash.is_some(),
            selection: builder.selection.len(),
            blueprints: &self.blueprints,
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
        self.touched = Instant::now();
        // On the start menu, Enter or Escape plays on; other keys wait.
        if self.hud.title_open() {
            if matches!(name, "enter" | "return" | "escape" | " " | "space") {
                self.hud.close_title();
            }
            return;
        }
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

    /// Take a building like one already built in hand: belts with the
    /// belt tool at their tier, anything else facing the same way on the
    /// same recipe.
    fn pipette(
        &mut self,
        building: mofact_core::ids::BuildingId,
        dir: mofact_core::geom::Dir,
        recipe: Option<mofact_core::ids::RecipeId>,
    ) {
        let role = self
            .content
            .buildings
            .iter()
            .find(|b| b.id == building)
            .map(|b| &b.role);
        let b = &mut self.controls.builder;
        if matches!(role, Some(mofact_core::content::BuildingRole::Belt { .. })) {
            b.belt = building;
            b.set_tool(Tool::Belt);
        } else {
            if matches!(
                role,
                Some(mofact_core::content::BuildingRole::Crafter { .. })
            ) {
                b.recipe = recipe;
            }
            b.set_tool(Tool::Place(building));
        }
        b.dir = dir;
    }

    /// Do what the HUD asks.
    /// What the building in hand still needs, for a refusal that says so:
    /// "A Lab needs 10 Brick, 5 Circuit more."
    fn short_of(&self) -> String {
        let id = match &self.controls.builder.tool {
            Tool::Place(b) => b.clone(),
            Tool::Belt => self.controls.builder.belt.clone(),
            _ => return tr("error.cannot_afford").into(),
        };
        let Some(def) = self.shown.buildings.iter().find(|b| b.id == id) else {
            return tr("error.cannot_afford").into();
        };
        let latest = self.sim.latest();
        let have = |item: &mofact_core::ids::ItemId| {
            latest
                .stock
                .iter()
                .find(|(i, _)| i == item)
                .map_or(0, |(_, n)| *n)
        };
        let missing: Vec<String> = def
            .cost
            .iter()
            .filter(|c| have(&c.item) < c.count)
            .map(|c| {
                let name = self
                    .shown
                    .items
                    .iter()
                    .find(|i| i.id == c.item)
                    .map_or(c.item.as_str(), |i| i.name.as_str());
                trf("error.short_item", &[&(c.count - have(&c.item)), &name])
            })
            .collect();
        if missing.is_empty() {
            return tr("error.cannot_afford").into();
        }
        let missing: Vec<&str> = missing.iter().map(String::as_str).collect();
        trf(
            "error.short_of",
            &[&def.name, &mofact_ui::i18n::join(&missing)],
        )
    }

    fn act(&mut self, action: Action) {
        match action {
            Action::Tool(tool) => self.controls.builder.set_tool(tool),
            Action::Rotate => self.controls.builder.rotate(),
            Action::Recipe(recipe) => self.controls.builder.recipe = recipe,
            Action::Belt(belt) => self.controls.builder.belt = belt,
            Action::Pipette {
                building,
                dir,
                recipe,
            } => self.pipette(building, dir, recipe),
            Action::Send(command) => self.sim.send(command),
            Action::Undo => self.sim.undo(),
            Action::MenuOpened => self.requests.push(Request::Refresh),
            // The slots' words are in the language they were listed in.
            Action::Language(_) => self.requests.push(Request::Refresh),
            Action::LoadSlot(n) => self.requests.push(Request::Load(n)),
            Action::DeleteSlot(n) => self.requests.push(Request::Delete(n)),
            Action::NewGame {
                slot,
                seed,
                preset,
                ore_runs_out,
                orders,
            } => self.requests.push(Request::New {
                slot,
                seed,
                preset,
                rules: mofact_sim::Rules {
                    ore_runs_out,
                    orders: mofact_sim::OrderPace::from_u8(orders),
                },
            }),
            Action::Redo => self.sim.redo(),
            Action::CopyCrash => {
                if let Some(r) = self.crash.take() {
                    self.clipboard = Some(r);
                    self.hud.toast(tr("toast.crash_copied"));
                }
            }
            Action::Buy => {
                if let Some(p) = &mut self.purchase {
                    p.buy();
                }
            }
            Action::Restore => {
                if let Some(p) = &mut self.purchase {
                    p.restore();
                }
            }
            // With the hand, a tap inspects what's there or closes the sheet.
            // With a building in hand, a tap on something already built
            // inspects it instead of trying to build on top of it.
            Action::Inspect(at) => {
                // Picking a train stop wins over dropping a pin.
                if self.hud.picking_stop().is_none() && self.map_tap(at) {
                    return;
                }
                let hand = self.controls.builder.tool == Tool::Hand;
                let placing = matches!(self.controls.builder.tool, Tool::Place(_));
                if !hand && !placing {
                    return;
                }
                let [x, y] = self.controls.camera.world_at(at);
                let tile = mofact_core::geom::TilePos::new(x.floor() as i32, y.floor() as i32);
                let latest = self.sim.latest();
                // Choosing a stop for a train's schedule: a tap on a stop adds it.
                if let Some(train) = self.hud.picking_stop() {
                    let stop = latest.entities.iter().any(|e| {
                        e.pos == tile
                            && self.content.buildings.iter().any(|b| {
                                b.id == e.building
                                    && b.role == mofact_core::content::BuildingRole::TrainStop
                            })
                    });
                    match latest.trains.iter().find(|t| t.id == train) {
                        Some(t) if stop => {
                            let stops = mofact_ui::add_stop(&t.schedule, tile);
                            self.sim.send(mofact_core::command::Command::SetSchedule {
                                entity: train,
                                stops,
                            });
                            self.hud.stop_picked();
                        }
                        _ => self.hud.toast(tr("toast.tap_stop")),
                    }
                    return;
                }
                // A train is picked where it stands, or on the tile it's moving onto.
                let train = latest
                    .trains
                    .iter()
                    .find(|t| t.pos == tile || (t.along > 0.0 && t.pos.step(t.dir) == tile));
                if let Some(t) = train {
                    self.hud.select(Some(t.id));
                    return;
                }
                self.built.update(&latest);
                let here = self.built.at(tile);
                // A signal, stop or train in hand goes on the track; the tap builds, not inspects.
                if placing && here.is_some() && self.controls.builder.places_on(here) {
                    return;
                }
                let found = here.map(|o| o.id);
                if hand || found.is_some() {
                    self.hud.select(found);
                }
            }
            Action::Cancel => self.controls.builder.cancel(),
            Action::Copy => {
                if let Some((bp, corner)) = self.capture() {
                    // Beside the original, or mid-screen if that is out of sight.
                    let beside = TilePos::new(corner.x + bp.size.0 + 1, corner.y);
                    let cam = &self.controls.camera;
                    let mid = cam.world_at([0.0, 0.0]);
                    let far = cam.world_at(self.target.as_ref().map_or([1.0, 1.0], |t| {
                        let (w, h) = t.size();
                        [w as f32, h as f32]
                    }));
                    let seen = (beside.x as f32) >= mid[0]
                        && ((beside.x + bp.size.0) as f32) <= far[0]
                        && (beside.y as f32) >= mid[1]
                        && ((beside.y + bp.size.1) as f32) <= far[1];
                    let at = if seen { beside } else { self.centred(&bp) };
                    self.controls.builder.hold(bp, at, Vec::new());
                }
            }
            Action::Cut => {
                if let Some((bp, corner)) = self.capture() {
                    let moving = self.controls.builder.selection.clone();
                    self.controls.builder.hold(bp, corner, moving);
                }
            }
            Action::DeleteSelection => {
                let commands = self
                    .controls
                    .builder
                    .selection
                    .iter()
                    .map(|&entity| mofact_core::command::Command::Remove { entity })
                    .collect();
                self.sim.send_all(commands);
                self.controls.builder.set_tool(Tool::Select);
            }
            Action::SaveBlueprint => {
                if self.blueprints.len() >= mofact_input::blueprint::LIBRARY_SIZE {
                    self.hud.toast(tr("toast.library_full"));
                } else if let Some((bp, _)) = self.capture() {
                    self.hud.toast(trf("toast.blueprint_saved", &[&bp.name]));
                    self.blueprints.insert(0, bp);
                    self.blueprints_rev += 1;
                }
            }
            Action::PlacePaste => {
                self.built.update(&self.sim.latest());
                if self.controls.builder.blocked(&self.built).is_empty() {
                    let mut commands = self.controls.builder.build_paste();
                    // Where drones reach, they build it as the stock comes
                    // in (T50); a move is always done at once.
                    if self.drones_cover(&commands) {
                        self.hud.toast(tr("toast.planned"));
                        for c in &mut commands {
                            if let mofact_core::command::Command::Place {
                                building,
                                pos,
                                dir,
                                recipe,
                            } = c.clone()
                            {
                                *c = mofact_core::command::Command::Plan {
                                    building,
                                    pos,
                                    dir,
                                    recipe,
                                };
                            }
                        }
                    }
                    self.sim.send_all(commands);
                } else {
                    self.hud.toast(tr("toast.paste_blocked"));
                }
            }
            Action::Paste(i) => {
                if let Some(bp) = self.blueprints.get(i).cloned() {
                    let at = self.centred(&bp);
                    self.controls.builder.hold(bp, at, Vec::new());
                }
            }
            Action::ForgetBlueprint(i) => {
                if i < self.blueprints.len() {
                    self.blueprints.remove(i);
                    self.blueprints_rev += 1;
                }
            }
            // A problem from the list: look at it, with the hand, so its
            // inspect sheet (opened by the HUD) says what's wrong.
            Action::Mute(muted) => self.sound.set_muted(muted),
            Action::LowPower(on) => self.scene.low_power = on,
            Action::ColourBlind(on) => self.use_colour_blind(on),
            Action::LargeText(_) => {}
            Action::ReduceMotion(on) => self.scene.still = on,
            Action::Volume(percent) => self.sound.set_volume(percent),
            Action::Ambience(on) => self.sound.set_ambience(on),
            Action::Vibration(_) => {}
            Action::Map(open) => {
                let cam = &mut self.controls.camera;
                let scale = cam.scale();
                if open {
                    self.before_map = Some(cam.pixels_per_tile / scale);
                    cam.zoom_to(crate::worldmap::MAP_OPEN * scale);
                } else {
                    let back = self.before_map.take().unwrap_or(32.0);
                    cam.zoom_to(back.max(crate::worldmap::MAP_ZOOM * 2.0) * scale);
                }
                self.controls.builder.set_tool(Tool::Hand);
            }
            Action::Focus(at) => {
                self.controls.camera.center = at;
                self.controls.builder.set_tool(Tool::Hand);
            }
        }
    }

    /// On the world map a tap drops a pin, or picks one up. Returns whether
    /// the tap was on the map.
    fn map_tap(&mut self, at: [f32; 2]) -> bool {
        let cam = &self.controls.camera;
        let camera = Camera {
            center: cam.center,
            pixels_per_tile: cam.pixels_per_tile,
        };
        let scale = cam.scale();
        if !crate::worldmap::is_map(&camera, scale) {
            return false;
        }
        let Some(size) = self.target.as_ref().map(|t| t.size()) else {
            return false;
        };
        let pins = self.sim.latest().pins.clone();
        let command = match crate::worldmap::pin_at(&camera, size, scale, &pins, at) {
            Some(tile) => mofact_core::command::Command::Unpin { tile },
            None => {
                let [x, y] = cam.world_at(at);
                let tile = mofact_core::geom::TilePos::new(x.floor() as i32, y.floor() as i32);
                mofact_core::command::Command::Pin { tile }
            }
        };
        self.sim.send(command);
        true
    }

    /// A pointer event goes to the HUD first, then to the camera and build tools.
    pub fn handle(&mut self, p: Pointer) {
        self.touched = Instant::now();
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

impl Game {
    /// Whether the game menu has asked anything of the save slots.
    pub fn wants_slots(&self) -> bool {
        !self.requests.is_empty()
    }

    /// The bytes of the game being played, waiting at most two seconds.
    pub fn save_bytes(&self) -> Result<Vec<u8>, String> {
        let rx = self.sim.save();
        // The browser's simulation answers at once; a thread may take a tick.
        rx.try_recv()
            .or_else(|_| rx.recv_timeout(std::time::Duration::from_secs(2)))
            .map_err(|_| "the simulation did not answer the save".to_string())
    }

    /// Do what the game menu asked of the slots in `store`: each switch
    /// saves the game being played first, so nothing is lost.
    pub fn serve(&mut self, store: &mut dyn Store) -> Result<(), String> {
        for request in std::mem::take(&mut self.requests) {
            let current = store.current();
            match request {
                Request::Refresh => store.write(current, &self.save_bytes()?)?,
                Request::Delete(n) => {
                    if n != current && crate::slots::valid(n) {
                        store.remove(n);
                    }
                }
                Request::Load(n) => {
                    if n == current || !crate::slots::valid(n) {
                        continue;
                    }
                    let Some(bytes) = store.read(n) else {
                        continue;
                    };
                    let (sim, _) = Sim::load(&self.content, &bytes).map_err(|e| e.to_string())?;
                    store.write(current, &self.save_bytes()?)?;
                    self.switch(sim);
                    store.set_current(n);
                    self.hud.toast(trf("toast.slot_loaded", &[&(n + 1)]));
                }
                Request::New {
                    slot,
                    seed,
                    preset,
                    rules,
                } => {
                    if !crate::slots::valid(slot) {
                        continue;
                    }
                    let (mut sim, _) = new_game(&self.content, seed)?;
                    sim.set_world_gen(seed, mofact_world::Preset::from_u8(preset));
                    sim.set_rules(rules);
                    if slot != current {
                        store.write(current, &self.save_bytes()?)?;
                    }
                    store.write(slot, &sim.save())?;
                    self.switch(sim);
                    store.set_current(slot);
                    self.hud.toast(trf("menu.new_game_in_slot", &[&(slot + 1)]));
                }
            }
        }
        self.list_slots(store);
        Ok(())
    }

    /// List the slots in `store` for the game menu.
    pub fn list_slots(&mut self, store: &dyn Store) {
        let current = store.current();
        self.slots = (0..mofact_ui::SLOTS)
            .map(|n| crate::slots::summary(&self.content, store.read(n).as_deref(), n == current))
            .collect();
    }

    /// Play `sim` instead: its own map, a fresh look at it, and the hand.
    fn switch(&mut self, mut sim: Sim) {
        let (seed, preset) = sim
            .world_seed()
            .unwrap_or((42, mofact_world::Preset::Standard));
        self.ore = self.ore.regen(&self.content, seed, preset);
        sim.set_world_gen(seed, preset);
        let camera = framing(&self.content, &sim);
        sim.set_demo(self.purchase.as_ref().and_then(Purchase::demo));
        self.sim = SimHandle::spawn(sim);
        self.built = Built::new(&self.content);
        self.built.world = Some(self.ore.gen());
        self.scene.forget_terrain();
        self.controls.camera.center = camera.center;
        self.controls.builder.set_tool(Tool::Hand);
        self.hud.select(None);
        self.guide = Guide::default();
        self.before_map = None;
        self.latest = (0, Instant::now());
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
    panels: &[[f32; 4]],
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
    // A label centred on `mid`, above the span `ys` (top and bottom on
    // screen), or below it when `below`. Where the HUD's cards or a label
    // already drawn cover that spot (a goal card over the hub on a phone
    // held sideways), it tries the other side, then each side slid clear
    // along the row, and keeps its first spot if nothing is clear.
    let mut taken: Vec<[f32; 4]> = panels.to_vec();
    let mut label =
        |o: &mut Overlay, text: &str, mid: f32, ys: [f32; 2], below: bool, ink: [u8; 4]| {
            let (pad, h, gap) = (8.0 * scale, 24.0 * scale, 4.0 * scale);
            let w = o.measure(text, px) + 2.0 * pad;
            let fit = |x: f32| x.clamp(4.0 * scale, sw as f32 - w - 4.0 * scale);
            let x0 = fit(mid - w / 2.0);
            let top_of = |below: bool| if below { ys[1] + gap } else { ys[0] - gap - h };
            let over = |x: f32, top: f32, r: &[f32; 4]| {
                x < r[0] + r[2] && x + w > r[0] && top < r[1] + r[3] && top + h > r[1]
            };
            let clear = |x: f32, top: f32| {
                top >= 0.0 && top + h <= sh as f32 && !taken.iter().any(|r| over(x, top, r))
            };
            let mut spot = None;
            'find: for side in [below, !below] {
                let top = top_of(side);
                let mut xs = vec![x0];
                for r in taken.iter().filter(|r| over(x0, top, r)) {
                    xs.push(fit(r[0] - w - gap));
                    xs.push(fit(r[0] + r[2] + gap));
                }
                for x in xs {
                    // Not so far along that it stops pointing at its building.
                    if (x - x0).abs() <= w * 0.75 && clear(x, top) {
                        spot = Some((x, top));
                        break 'find;
                    }
                }
            }
            let (x, top) = spot.unwrap_or((x0, top_of(below)));
            taken.push([x, top, w, h]);
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
                let ys = [p[1], p[1] + f32::from(*h) * ppt];
                label(o, text, mid, ys, *below, [0xe9, 0xed, 0xf0, 0xff]);
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
            let ys = [
                first[1].min(last[1]) - 2.0 * scale,
                first[1].max(last[1]) + ppt + 2.0 * scale,
            ];
            label(o, step.say(), mid, ys, *below, accent);
            Some(step.tool())
        }
    }
}

/// Power made visible on the map: cables between poles in reach of each
/// other and from each pole to what it powers (as the sim joins them: the
/// first pole whose supply square covers it), and a "No power" tag on
/// anything that needs power and has none.
///
/// Cables hang between connection points: the insulator on top of each pole
/// (in its art) and a socket near the top-right corner of every building
/// that draws or makes power, drawn here. Each cable sags in the middle and,
/// up close, has a highlight along its top and a shadow on the ground that
/// comes nearer the cable where it hangs lowest. In low-power mode, or zoomed
/// far out, a cable is a single run of dots.
fn power_marks(
    o: &mut Overlay,
    content: &Content,
    snap: &mofact_core::snapshot::Snapshot,
    camera: &Camera,
    (sw, sh): (u32, u32),
    scale: f32,
    low_power: bool,
) {
    use mofact_core::content::BuildingRole as R;
    let ppt = camera.pixels_per_tile;
    let def = |e: &mofact_core::snapshot::EntityView| {
        content.buildings.iter().find(|b| b.id == e.building)
    };
    // A point in tiles on screen.
    let at = |x: f32, y: f32| {
        [
            (x - camera.center[0]) * ppt + sw as f32 / 2.0,
            (y - camera.center[1]) * ppt + sh as f32 / 2.0,
        ]
    };
    let size = |e: &mofact_core::snapshot::EntityView| def(e).map_or((1, 1), |d| d.size);
    // The middle of an entity on screen: where a pole's insulator is.
    let mid = |e: &mofact_core::snapshot::EntityView| {
        let (w, h) = size(e);
        at(
            e.pos.x as f32 + f32::from(w) / 2.0,
            e.pos.y as f32 + f32::from(h) / 2.0,
        )
    };
    // A building's socket, in from its top-right corner.
    let inset = 0.24;
    let socket = |e: &mofact_core::snapshot::EntityView| {
        let (w, _) = size(e);
        at(
            e.pos.x as f32 + f32::from(w) - inset,
            e.pos.y as f32 + inset,
        )
    };
    let poles: Vec<_> = snap
        .entities
        .iter()
        .filter_map(|e| match def(e)?.role {
            R::Pole { wire, supply } => Some((e, wire, supply)),
            _ => None,
        })
        .collect();
    // Up close: shading, shadows and sockets. Far out, or saving power: plain.
    let fine = !low_power && ppt >= 10.0 * scale;
    let thick = (2.0 * scale).max(ppt * 0.07);
    // Each cable: its two ends, how far its middle hangs, and its thickness.
    let mut cables: Vec<([f32; 2], [f32; 2], f32, f32)> = Vec::new();
    for (i, (a, wa, _)) in poles.iter().enumerate() {
        for (b, wb, _) in &poles[i + 1..] {
            let reach = i32::from((*wa).min(*wb));
            if (a.pos.x - b.pos.x).abs().max((a.pos.y - b.pos.y).abs()) <= reach {
                let (pa, pb) = (mid(a), mid(b));
                let len = (pb[0] - pa[0]).hypot(pb[1] - pa[1]);
                cables.push((pa, pb, len * 0.1, thick));
            }
        }
    }
    let mut sockets = Vec::new();
    let mut unpowered = Vec::new();
    for e in &snap.entities {
        let Some(d) = def(e) else { continue };
        let powered = d.power > 0.0 || matches!(d.role, R::Generator { .. } | R::Engine { .. });
        if !powered {
            continue;
        }
        let (w, h) = (i32::from(d.size.0), i32::from(d.size.1));
        let covering = poles.iter().find(|(p, _, s)| {
            let s = i32::from(*s);
            e.pos.x <= p.pos.x + s
                && e.pos.x + w > p.pos.x - s
                && e.pos.y <= p.pos.y + s
                && e.pos.y + h > p.pos.y - s
        });
        let end = socket(e);
        if fine {
            sockets.push(end);
        }
        if let Some((p, _, _)) = covering {
            let start = mid(p);
            let len = (end[0] - start[0]).hypot(end[1] - start[1]);
            cables.push((start, end, len * 0.12, thick * 0.8));
        }
        if e.state == mofact_core::snapshot::MachineState::NoPower && ppt >= 14.0 * scale {
            unpowered.push((mid(e), d.size.1));
        }
    }
    let on_screen = |lo: [f32; 2], hi: [f32; 2]| {
        hi[0] >= 0.0 && hi[1] >= 0.0 && lo[0] <= sw as f32 && lo[1] <= sh as f32
    };
    // The ground shadow of a cable at its ends: as high as a pole is.
    let lift = [ppt * 0.08, ppt * 0.11];
    // A cable's centre line, sampled: `f` from 0 to 1 along it, and how much
    // it hangs there from 0 at the ends to 1 in the middle.
    let samples = |(a, b, sag, t): ([f32; 2], [f32; 2], f32, f32), step: f32| {
        let lo = [a[0].min(b[0]) - t - lift[0], a[1].min(b[1]) - t - lift[1]];
        let hi = [
            a[0].max(b[0]) + t + lift[0],
            a[1].max(b[1]) + sag + t + lift[1],
        ];
        let len = (b[0] - a[0]).hypot(b[1] - a[1]) + sag;
        let n = if on_screen(lo, hi) {
            (len / step).ceil().clamp(1.0, 600.0) as usize
        } else {
            0
        };
        (0..=n).filter(move |_| n > 0).map(move |k| {
            let f = k as f32 / n as f32;
            let bow = 4.0 * f * (1.0 - f);
            (
                [
                    a[0] + (b[0] - a[0]) * f,
                    a[1] + (b[1] - a[1]) * f + sag * bow,
                ],
                bow,
            )
        })
    };
    let dot = |o: &mut Overlay, p: [f32; 2], d: f32, ink: [u8; 4]| {
        if p[0] > -d && p[1] > -d && p[0] < sw as f32 + d && p[1] < sh as f32 + d {
            o.dot(p, d, ink);
        }
    };
    if fine {
        // Shadows first, under everything: straight along the ground below
        // the cable, and nearer it where it hangs low. The dots overlap
        // about twice, so each is half as dark.
        for &c in &cables {
            for (p, bow) in samples(c, c.3 * 0.5) {
                let k = 1.0 - 0.55 * bow;
                let ground = [p[0] + lift[0] * k, p[1] - c.2 * bow + lift[1] * k];
                dot(o, ground, c.3 * 1.2, [0, 0, 0, 0x2c]);
            }
        }
        // Sockets: a dark plate with a porcelain insulator on it.
        let u = (ppt * 0.2).max(4.0 * scale);
        for &s in &sockets {
            if !on_screen([s[0] - u, s[1] - u], [s[0] + u, s[1] + u]) {
                continue;
            }
            o.panel(
                [s[0] - u * 0.55, s[1] - u * 0.55],
                [u * 1.1, u * 1.1],
                u * 0.25,
                [0x1b, 0x20, 0x26, 0xff],
            );
            o.dot(
                [s[0] + u * 0.06, s[1] + u * 0.08],
                u * 0.8,
                [0x00, 0x00, 0x00, 0x60],
            );
            o.dot(s, u * 0.72, [0x9c, 0x94, 0x86, 0xff]);
            o.dot(s, u * 0.52, [0xe2, 0xdc, 0xcf, 0xff]);
            o.dot(
                [s[0] - u * 0.1, s[1] - u * 0.1],
                u * 0.16,
                [0xff, 0xff, 0xff, 0xff],
            );
        }
    }
    // The cables: dark rubber, and up close a lit stripe along the top.
    let rubber = [0x17, 0x19, 0x1c, 0xff];
    let shine = [0x6c, 0x74, 0x7e, 0xff];
    for &c in &cables {
        let t = c.3;
        for (p, _) in samples(c, t * 0.5) {
            dot(o, p, t, rubber);
        }
        if fine && t >= 2.5 {
            for (p, _) in samples(c, t * 0.2) {
                dot(o, [p[0] - t * 0.12, p[1] - t * 0.2], t * 0.4, shine);
            }
        }
    }
    // The insulator caps the cables are tied to, over their ends.
    if fine {
        for &(a, b, _, t) in &cables {
            for p in [a, b] {
                dot(o, p, t * 1.5, [0x9c, 0x94, 0x86, 0xff]);
                dot(o, p, t * 1.1, [0xe2, 0xdc, 0xcf, 0xff]);
            }
        }
    }
    for (c, rows) in unpowered {
        let px = 11.0 * scale;
        let text = tr("state.no_power");
        let (pad, ch) = (6.0 * scale, 18.0 * scale);
        let tw = o.measure(text, px) + 2.0 * pad;
        let top = c[1] - f32::from(rows) * ppt / 2.0 - ch - 2.0 * scale;
        o.panel(
            [c[0] - tw / 2.0, top],
            [tw, ch],
            ch / 2.0,
            [0xff, 0x5d, 0x5d, 0xf0],
        );
        o.text(
            text,
            [c[0] - tw / 2.0 + pad, top + (ch + o.cap_height(px)) / 2.0],
            px,
            [0x1b, 0x1f, 0x24, 0xff],
        );
    }
}

#[cfg(test)]
mod pacing {
    use super::*;

    #[test]
    fn sixty_a_second_normally_thirty_when_saving_power() {
        // Frames as a 60 Hz display offers them, for one second.
        let start = Instant::now();
        let count = |slow: bool| {
            let mut drawn = None;
            let mut n = 0;
            for k in 0..60u64 {
                let now = start + Duration::from_micros(16_667 * k);
                if due(drawn, now, slow) {
                    drawn = Some(now);
                    n += 1;
                }
            }
            n
        };
        assert_eq!(count(false), 60);
        assert_eq!(count(true), 30);
    }
}
