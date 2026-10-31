//! UI lane: touch-first UI kit on the game renderer: toolbar, inspect sheet,
//! radial build menu, recipe browser and starving dashboard.
//!
//! The [`Hud`] is immediate mode. Each frame, [`Hud::draw`] lays out and
//! paints the toolbar, the rotate button, the recipe pills, the stock, tips
//! the research button and sheet, and the inspect sheet onto the render [`Overlay`], and remembers where
//! every control went. Pointer events go to [`Hud::pointer`] first: a press
//! that lands on a control belongs to the HUD until it lifts, and anything
//! else falls through to the camera and the build tools.
//!
//! The layout is one-handed (R30): in portrait the toolbar runs along the
//! bottom edge; in landscape it stands on the right edge, where a right thumb
//! rests. The rotate button, recipe pills and inspect sheet sit next to it.

use mofact_core::command::Command;
use mofact_core::content::{BuildingDef, BuildingRole, Content, RecipeDef, ResearchDef, Stack};
use mofact_core::geom::Dir;
use mofact_core::ids::{BuildingId, EntityId, ItemId, RecipeId, ResearchId};
use mofact_core::snapshot::{EntityView, MachineState, Snapshot};
use mofact_input::{Pointer, Tool, MOUSE_PAN};
use mofact_render::look;
use mofact_render::palette::{self, Rgba};
use mofact_render::{Look, Overlay};
use std::time::{Duration, Instant};

pub use mofact_core as core;

const PANEL: Rgba = [0x1d, 0x23, 0x2a, 0xeb];
const RAISED: Rgba = [0x2a, 0x31, 0x39, 0xff];
const PRESSED: Rgba = [0x3a, 0x44, 0x4f, 0xff];
const TEXT: Rgba = [0xe9, 0xed, 0xf0, 0xff];
const MUTED: Rgba = [0x8a, 0x95, 0xa1, 0xff];
const WORKING: Rgba = [0x63, 0xd4, 0x8a, 0xff];
/// A finger that moves further than this (in points) is dragging, not tapping.
const TAP_SLOP: f32 = 12.0;
const TOAST_FOR: Duration = Duration::from_millis(2500);

/// The toolbar, left to right (top to bottom in landscape).
const TOOLS: [(&str, Option<&str>); 7] = [
    ("Move", None),
    ("Belt", Some("belt")),
    ("Drill", Some("drill")),
    ("Furnace", Some("furnace")),
    ("Assembler", Some("assembler")),
    ("More", None),
    ("Remove", None),
];
/// The slot that opens the build sheet with every other building, and
/// shows the one in hand when it is one of those.
const MORE: usize = 5;
/// The last tool, Remove.
const REMOVE: usize = TOOLS.len() - 1;

fn tool_at(i: usize) -> Tool {
    match i {
        0 => Tool::Hand,
        1 => Tool::Belt,
        REMOVE => Tool::Remove,
        _ => Tool::Place(TOOLS[i].1.unwrap_or_default().into()),
    }
}

fn tool_index(tool: &Tool) -> Option<usize> {
    match tool {
        Tool::Hand => Some(0),
        Tool::Belt => Some(1),
        Tool::Remove => Some(REMOVE),
        Tool::Place(b) => TOOLS
            .iter()
            .position(|(_, id)| *id == Some(b.as_str()))
            .or(Some(MORE)),
    }
}

/// What the HUD needs to know about the game this frame.
pub struct View<'a> {
    /// The screen, in pixels.
    pub size: (u32, u32),
    /// Pixels per point: the display's scale factor.
    pub scale: f32,
    pub content: &'a Content,
    pub look: &'a Look,
    pub snapshot: &'a Snapshot,
    pub tool: &'a Tool,
    /// The way new buildings face.
    pub dir: Dir,
    /// The recipe new crafters get.
    pub recipe: Option<&'a RecipeId>,
    /// The belt tier the belt tool lays.
    pub belt: &'a BuildingId,
}

impl View<'_> {
    fn building(&self, id: &BuildingId) -> Option<&BuildingDef> {
        self.content.buildings.iter().find(|b| &b.id == id)
    }

    fn recipe_def(&self, id: &RecipeId) -> Option<&RecipeDef> {
        self.content.recipes.iter().find(|r| &r.id == id)
    }

    fn stock(&self, item: &ItemId) -> u32 {
        self.snapshot
            .stock
            .binary_search_by(|(i, _)| i.cmp(item))
            .map_or(0, |at| self.snapshot.stock[at].1)
    }

    fn affordable(&self, cost: &[Stack]) -> bool {
        cost.iter().all(|s| self.stock(&s.item) >= s.count)
    }

    fn done(&self, id: &ResearchId) -> bool {
        self.snapshot.research.done.contains(id)
    }

    /// Whether no unfinished research still locks `recipe`.
    fn recipe_open(&self, recipe: &RecipeId) -> bool {
        self.content
            .research
            .iter()
            .all(|r| self.done(&r.id) || !r.recipes.contains(recipe))
    }

    /// Whether no unfinished research still locks `building`.
    fn building_open(&self, building: &BuildingId) -> bool {
        self.content
            .research
            .iter()
            .all(|r| self.done(&r.id) || !r.buildings.contains(building))
    }

    /// Whether `r` can be picked now: not done, and its prerequisites are.
    fn research_ready(&self, r: &ResearchDef) -> bool {
        !self.done(&r.id) && r.requires.iter().all(|q| self.done(q))
    }

    /// The unlocked recipes a building can be set to, if it is a crafter.
    fn recipes_for(&self, id: &BuildingId) -> Option<Vec<&RecipeDef>> {
        let BuildingRole::Crafter { categories, .. } = &self.building(id)?.role else {
            return None;
        };
        Some(
            self.content
                .recipes
                .iter()
                .filter(|r| categories.contains(&r.category) && self.recipe_open(&r.id))
                .collect(),
        )
    }
}

/// What a tap on the HUD asks the game to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Pick a build tool.
    Tool(Tool),
    /// Turn the way new buildings face.
    Rotate,
    /// Choose the recipe new crafters get; `None` lets them pick from what arrives.
    Recipe(Option<RecipeId>),
    /// Choose the belt tier the belt tool lays.
    Belt(BuildingId),
    /// Send a command to the simulation.
    Send(Command),
    /// A tap on the world with the hand tool, at this screen point: inspect what's there.
    Inspect([f32; 2]),
}

/// A control on screen, and what tapping it does.
#[derive(Clone, Debug, PartialEq)]
enum Hit {
    Tool(usize),
    Rotate,
    Recipe(Option<RecipeId>),
    Belt(BuildingId),
    SetRecipe(EntityId, Option<RecipeId>),
    Turn(EntityId, Dir),
    Remove(EntityId),
    /// The research button: opens or closes the research sheet.
    Research,
    /// Set what labs work on.
    Study(Option<ResearchId>),
    /// Pick a building from the build sheet.
    Build(BuildingId),
    Close,
    /// A panel: it takes the touch, so nothing builds underneath.
    Block,
}

type Rect = [f32; 4];

fn inside(r: &Rect, [x, y]: [f32; 2]) -> bool {
    x >= r[0] && y >= r[1] && x < r[0] + r[2] && y < r[1] + r[3]
}

#[derive(Debug)]
struct Press {
    id: u64,
    hit: Hit,
    at: [f32; 2],
}

/// A touch on the world that may turn out to be a tap.
#[derive(Debug)]
struct Tap {
    id: u64,
    start: [f32; 2],
}

/// The heads-up display: toolbar, rotate button, recipe pills, stock, tips and
/// the inspect sheet.
#[derive(Debug, Default)]
pub struct Hud {
    /// The entity the inspect sheet shows.
    selected: Option<EntityId>,
    /// Whether the research sheet is open.
    research: bool,
    /// Whether the build sheet is open.
    more: bool,
    /// Where each control went in the last frame, topmost last.
    hits: Vec<(Rect, Hit)>,
    press: Option<Press>,
    tap: Option<Tap>,
    toast: Option<(String, Instant)>,
    /// Pixels per point in the last frame.
    unit: f32,
}

impl Hud {
    pub fn new() -> Self {
        Self::default()
    }

    /// The entity the inspect sheet is showing.
    pub fn selected(&self) -> Option<EntityId> {
        self.selected
    }

    /// Show `entity` in the inspect sheet, or close it.
    pub fn select(&mut self, entity: Option<EntityId>) {
        self.selected = entity;
        if entity.is_some() {
            self.research = false;
            self.more = false;
        }
    }

    /// Whether the research sheet is open.
    pub fn researching(&self) -> bool {
        self.research
    }

    /// Open or close the research sheet, as its button does.
    pub fn toggle_research(&mut self) {
        self.tapped(Hit::Research);
    }

    /// Open or close the build sheet, as the More slot does.
    pub fn toggle_build_sheet(&mut self) {
        self.tapped(Hit::Tool(MORE));
    }

    /// Show a short message, such as why a build was refused.
    pub fn toast(&mut self, text: impl Into<String>) {
        self.toast = Some((text.into(), Instant::now()));
    }

    fn hit(&self, p: [f32; 2]) -> Option<&Hit> {
        self.hits
            .iter()
            .rev()
            .find(|(r, _)| inside(r, p))
            .map(|(_, h)| h)
    }

    /// Whether `p` is over a control or panel.
    pub fn covers(&self, p: [f32; 2]) -> bool {
        self.hit(p).is_some()
    }

    /// Offer a pointer event to the HUD. Returns whether the HUD took it (so
    /// the camera and build tools must not see it) and what it asks for.
    pub fn pointer(&mut self, event: Pointer) -> (bool, Vec<Action>) {
        match event {
            Pointer::Down { id, x, y } => {
                if self.press.is_none() {
                    if let Some(hit) = self.hit([x, y]).cloned() {
                        self.press = Some(Press {
                            id,
                            hit,
                            at: [x, y],
                        });
                        return (true, Vec::new());
                    }
                }
                // A second finger makes it a pinch, not a tap.
                self.tap = match self.tap {
                    None if id != MOUSE_PAN => Some(Tap { id, start: [x, y] }),
                    _ => None,
                };
                (false, Vec::new())
            }
            Pointer::Move { id, x, y } => {
                if let Some(press) = self.press.as_mut().filter(|p| p.id == id) {
                    press.at = [x, y];
                    return (true, Vec::new());
                }
                if let Some(tap) = &self.tap {
                    let slop = TAP_SLOP * self.unit.max(1.0);
                    if tap.id == id && (x - tap.start[0]).hypot(y - tap.start[1]) > slop {
                        self.tap = None;
                    }
                }
                (false, Vec::new())
            }
            Pointer::Up { id } => {
                if self.press.as_ref().is_some_and(|p| p.id == id) {
                    let press = self.press.take().expect("checked");
                    // Sliding off a control before lifting cancels the tap.
                    let actions = if self.hit(press.at) == Some(&press.hit) {
                        self.tapped(press.hit)
                    } else {
                        Vec::new()
                    };
                    return (true, actions);
                }
                match self.tap.take() {
                    Some(tap) if tap.id == id => (false, vec![Action::Inspect(tap.start)]),
                    other => {
                        self.tap = other;
                        (false, Vec::new())
                    }
                }
            }
            Pointer::Wheel { x, y, .. } => (self.covers([x, y]), Vec::new()),
        }
    }

    fn tapped(&mut self, hit: Hit) -> Vec<Action> {
        match hit {
            Hit::Tool(MORE) => {
                self.more = !self.more;
                self.research = false;
                self.selected = None;
                Vec::new()
            }
            Hit::Tool(i) => {
                if i != 0 {
                    self.selected = None;
                }
                self.more = false;
                vec![Action::Tool(tool_at(i))]
            }
            Hit::Build(b) => {
                self.more = false;
                self.selected = None;
                vec![Action::Tool(Tool::Place(b))]
            }
            Hit::Rotate => vec![Action::Rotate],
            Hit::Recipe(r) => vec![Action::Recipe(r)],
            Hit::Belt(b) => vec![Action::Belt(b)],
            Hit::SetRecipe(entity, recipe) => {
                vec![Action::Send(Command::SetRecipe { entity, recipe })]
            }
            Hit::Turn(entity, dir) => vec![Action::Send(Command::Rotate {
                entity,
                dir: dir.rotate_cw(),
            })],
            Hit::Remove(entity) => {
                self.selected = None;
                vec![Action::Send(Command::Remove { entity })]
            }
            Hit::Research => {
                self.research = !self.research;
                self.more = false;
                self.selected = None;
                Vec::new()
            }
            Hit::Study(research) => vec![Action::Send(Command::SetResearch { research })],
            Hit::Close => {
                self.selected = None;
                self.research = false;
                self.more = false;
                Vec::new()
            }
            Hit::Block => Vec::new(),
        }
    }

    /// Lay out and paint the HUD for this frame.
    pub fn draw(&mut self, view: &View, o: &mut Overlay) {
        self.hits.clear();
        let (w, h) = (view.size.0 as f32, view.size.1 as f32);
        let portrait = h > w;
        // Points to pixels, shrunk if the toolbar would not fit across.
        let across = if portrait { w } else { h };
        let n = TOOLS.len() as f32;
        let u = view.scale.max(0.5).min(across / (n * 64.0 + 24.0));
        self.unit = u;
        let m = 12.0 * u;
        let gap = 10.0 * u;
        let selected = self
            .selected
            .and_then(|id| view.snapshot.entities.iter().find(|e| e.id == id));
        if selected.is_none() {
            self.selected = None;
        }

        // The toolbar, and the space left beside it for everything else.
        let bar = if portrait {
            let bh = 76.0 * u;
            [m, h - m - bh, w - 2.0 * m, bh]
        } else {
            let bw = 92.0 * u;
            let bh = (h - 2.0 * m).min(n * 80.0 * u);
            [w - m - bw, h - m - bh, bw, bh]
        };
        self.toolbar(view, o, bar, portrait);
        // The bottom-right corner of the free area, where the thumb is.
        let corner = if portrait {
            [w - m, bar[1] - gap]
        } else {
            [bar[0] - gap, h - m]
        };

        let top = self.stock(view, o, [m, m]);
        let top = self.research_button(view, o, [m, top + gap], corner[0] - m);
        let mut tip_y = top + gap;
        if let Some((text, at)) = &self.toast {
            if at.elapsed() < TOAST_FOR {
                let text = text.clone();
                tip_y = chip(o, &text, [m, tip_y], corner[0] - m, u, palette::STARVE) + gap;
            } else {
                self.toast = None;
            }
        }
        if selected.is_none() && !self.research && !self.more {
            chip(o, tip(view.tool), [m, tip_y], corner[0] - m, u, TEXT);
        }
        let width = if portrait {
            w - 2.0 * m
        } else {
            (360.0 * u).min(corner[0] - m)
        };

        if self.more {
            let room = corner[1] - tip_y;
            self.build_sheet(view, o, [corner[0] - width, corner[1]], width, room);
            return;
        }
        if self.research {
            let room = corner[1] - tip_y;
            self.research_sheet(view, o, [corner[0] - width, corner[1]], width, room);
            return;
        }

        if let Some(e) = selected {
            self.sheet(view, o, e, [corner[0] - width, corner[1]], width);
            return;
        }

        // The rotate button and recipe pills for the tool in hand.
        let placing = match view.tool {
            Tool::Place(b) => Some(b),
            _ => None,
        };
        let mut right = corner[0];
        if placing.is_some() || *view.tool == Tool::Belt {
            let d = 60.0 * u;
            let r = [right - d, corner[1] - d, d, d];
            o.panel([r[0], r[1]], [d, d], d / 2.0, PANEL);
            let icon = 34.0 * u;
            let turns = Dir::ALL.iter().position(|&d| d == view.dir).unwrap_or(0) as u32;
            o.icon_turned(
                look::BELT,
                [r[0] + (d - icon) / 2.0, r[1] + (d - icon) / 2.0],
                icon,
                TEXT,
                turns,
            );
            self.hits.push((r, Hit::Rotate));
            right = r[0] - gap;
        }
        if let Some(recipes) = placing.and_then(|b| view.recipes_for(b)) {
            let pills = std::iter::once(None).chain(recipes.iter().map(|r| Some(*r)));
            let choices: Vec<_> = pills
                .map(|r| {
                    let chosen = r.map(|r| &r.id) == view.recipe;
                    recipe_pill(view, r, chosen, Hit::Recipe(r.map(|r| r.id.clone())))
                })
                .collect();
            self.pills(o, choices, [m, corner[1]], right - m, true);
        }
        // Belt tiers, once there is more than one to lay.
        if *view.tool == Tool::Belt {
            let tiers: Vec<Pill> = view
                .content
                .buildings
                .iter()
                .filter(|b| {
                    matches!(b.role, BuildingRole::Belt { .. }) && view.building_open(&b.id)
                })
                .map(|b| {
                    let chosen = &b.id == view.belt;
                    let icon = view.look.building(&b.id);
                    (b.name.as_str(), icon, chosen, Hit::Belt(b.id.clone()))
                })
                .collect();
            if tiers.len() > 1 {
                self.pills(o, tiers, [m, corner[1]], right - m, true);
            }
        }
    }

    fn toolbar(&mut self, view: &View, o: &mut Overlay, bar: Rect, portrait: bool) {
        let u = self.unit;
        o.panel([bar[0], bar[1]], [bar[2], bar[3]], 14.0 * u, PANEL);
        self.hits.push((bar, Hit::Block));
        let current = tool_index(view.tool);
        let pad = 4.0 * u;
        for (i, (label, building)) in TOOLS.iter().enumerate() {
            let r = if portrait {
                let bw = (bar[2] - 2.0 * pad) / TOOLS.len() as f32;
                [
                    bar[0] + pad + bw * i as f32,
                    bar[1] + pad,
                    bw,
                    bar[3] - 2.0 * pad,
                ]
            } else {
                let bh = (bar[3] - 2.0 * pad) / TOOLS.len() as f32;
                [
                    bar[0] + pad,
                    bar[1] + pad + bh * i as f32,
                    bar[2] - 2.0 * pad,
                    bh,
                ]
            };
            // The More slot stands for the building in hand, if it is one of its own.
            let (label, def) = match view.tool {
                Tool::Place(b) if i == MORE && current == Some(MORE) => {
                    let def = view.building(b);
                    (def.map_or(*label, |d| d.name.as_str()), def)
                }
                _ => (*label, building.and_then(|b| view.building(&b.into()))),
            };
            let cost = def.map(|d| d.cost.as_slice()).unwrap_or_default();
            let open = def.is_none_or(|d| view.building_open(&d.id));
            let afford = open && view.affordable(cost);
            if current == Some(i) || (i == MORE && self.more) {
                o.panel([r[0], r[1]], [r[2], r[3]], 10.0 * u, PRESSED);
                o.panel(
                    [r[0] + r[2] * 0.3, r[1] + r[3] - 3.0 * u],
                    [r[2] * 0.4, 3.0 * u],
                    1.5 * u,
                    palette::ACCENT,
                );
            }
            let icon = (28.0 * u).min(r[3] * 0.4);
            let ix = r[0] + (r[2] - icon) / 2.0;
            let iy = r[1] + 6.0 * u;
            let tint = |t: Rgba| if afford { t } else { [t[0], t[1], t[2], 0x70] };
            match (i, def) {
                (0, _) => {
                    // Move: a cross of bars.
                    let t = 4.0 * u;
                    let c = [ix + icon / 2.0, iy + icon / 2.0];
                    o.panel(
                        [ix + 2.0 * u, c[1] - t / 2.0],
                        [icon - 4.0 * u, t],
                        t / 2.0,
                        TEXT,
                    );
                    o.panel(
                        [c[0] - t / 2.0, iy + 2.0 * u],
                        [t, icon - 4.0 * u],
                        t / 2.0,
                        TEXT,
                    );
                }
                (MORE, None) => {
                    // More: three dots, so it doesn't read as Move's cross.
                    let d = icon * 0.2;
                    let cy = iy + (icon - d) / 2.0;
                    for k in 0..3 {
                        let x = ix + icon * 0.1 + k as f32 * icon * 0.3;
                        o.panel([x, cy], [d, d], d / 2.0, TEXT);
                    }
                }
                (REMOVE, _) => {
                    let px = icon * 1.3;
                    let cw = o.measure("×", px);
                    o.text(
                        "×",
                        [ix + (icon - cw) / 2.0, iy + icon * 0.95],
                        px,
                        palette::STARVE,
                    );
                }
                (_, Some(d)) => {
                    if let Some((sprite, t)) = view.look.building(&d.id) {
                        o.icon(sprite, [ix, iy], icon, tint(t));
                    }
                }
                _ => {}
            }
            let px = fit(o, label, 12.0 * u, r[2] - 6.0 * u);
            let lw = o.measure(label, px);
            let base = iy + icon + 4.0 * u + o.cap_height(px);
            o.text(label, [r[0] + (r[2] - lw) / 2.0, base], px, tint(TEXT));
            if !cost.is_empty() {
                let px = 10.0 * u;
                let s = 10.0 * u;
                let parts: Vec<String> = cost.iter().map(|c| c.count.to_string()).collect();
                let width: f32 = parts
                    .iter()
                    .map(|p| s + 2.0 * u + o.measure(p, px) + 4.0 * u)
                    .sum();
                let mut x = r[0] + (r[2] - width + 4.0 * u) / 2.0;
                let y = base + 4.0 * u;
                let color = if afford { MUTED } else { palette::STARVE };
                for (c, text) in cost.iter().zip(&parts) {
                    if let Some((sprite, t)) = view.look.item(&c.item) {
                        o.icon(sprite, [x, y], s, t);
                    }
                    x += s + 2.0 * u;
                    x += o.text(text, [x, y + s * 0.85], px, color) + 4.0 * u;
                }
            }
            self.hits.push((r, Hit::Tool(i)));
        }
    }

    /// The player's stock along the top. Returns the bottom edge.
    fn stock(&mut self, view: &View, o: &mut Overlay, at: [f32; 2]) -> f32 {
        let u = self.unit;
        let h = 30.0 * u;
        let px = 14.0 * u;
        let s = 18.0 * u;
        let mut x = at[0];
        for (item, count) in view.snapshot.stock.iter().filter(|(_, n)| *n > 0) {
            let text = count.to_string();
            let w = 8.0 * u + s + 5.0 * u + o.measure(&text, px) + 10.0 * u;
            o.panel([x, at[1]], [w, h], 8.0 * u, PANEL);
            if let Some((sprite, t)) = view.look.item(item) {
                o.icon(sprite, [x + 8.0 * u, at[1] + (h - s) / 2.0], s, t);
            }
            let base = at[1] + (h + o.cap_height(px)) / 2.0;
            o.text(&text, [x + 8.0 * u + s + 5.0 * u, base], px, TEXT);
            self.hits.push(([x, at[1], w, h], Hit::Block));
            x += w + 6.0 * u;
        }
        at[1] + h
    }

    /// The research button under the stock: what labs study and how far
    /// along. Returns the bottom edge.
    fn research_button(&mut self, view: &View, o: &mut Overlay, at: [f32; 2], width: f32) -> f32 {
        if view.content.research.is_empty() {
            return at[1] - 6.0 * self.unit;
        }
        let u = self.unit;
        let h = 36.0 * u;
        let px = 14.0 * u;
        let r = &view.snapshot.research;
        let current = r
            .current
            .as_ref()
            .and_then(|id| view.content.research.iter().find(|d| &d.id == id));
        let (text, ink) = match current {
            Some(d) => (
                format!("Research: {} {}/{}", d.name, r.progress.0, r.progress.1),
                TEXT,
            ),
            None if view.content.research.iter().any(|d| view.research_ready(d)) => (
                "Research: pick what labs study".to_string(),
                palette::ACCENT,
            ),
            None => ("Research: all done".to_string(), MUTED),
        };
        let px = fit(o, &text, px, width - 28.0 * u);
        let w = (o.measure(&text, px) + 28.0 * u).min(width);
        let fill = if self.research { PRESSED } else { RAISED };
        o.panel(at, [w, h], h / 2.0, fill);
        if current.is_some() {
            let done = r.progress.0 as f32 / r.progress.1.max(1) as f32;
            o.panel(
                [at[0] + h / 2.0, at[1] + h - 5.0 * u],
                [(w - h) * done.clamp(0.0, 1.0), 3.0 * u],
                1.5 * u,
                WORKING,
            );
        }
        o.text(
            &text,
            [at[0] + 14.0 * u, at[1] + (h + o.cap_height(px)) / 2.0],
            px,
            ink,
        );
        self.hits.push(([at[0], at[1], w, h], Hit::Research));
        at[1] + h
    }

    /// The build sheet, bottom-left at `at`, at most `room` tall: a grid of
    /// every building the toolbar has no slot for. Locked ones show but
    /// can't be picked.
    fn build_sheet(&mut self, view: &View, o: &mut Overlay, at: [f32; 2], width: f32, room: f32) {
        let u = self.unit;
        let pad = 16.0 * u;
        let gap = 8.0 * u;
        let title = 30.0 * u;
        let on_bar = |id: &BuildingId| TOOLS.iter().any(|(_, b)| *b == Some(id.as_str()));
        let list: Vec<&BuildingDef> = view
            .content
            .buildings
            .iter()
            .filter(|b| {
                !matches!(b.role, BuildingRole::Hub | BuildingRole::Belt { .. }) && !on_bar(&b.id)
            })
            .collect();
        let inner = width - 2.0 * pad;
        let cols = ((inner + gap) / (96.0 * u + gap)).floor().max(1.0) as usize;
        let cw = (inner - gap * (cols - 1) as f32) / cols as f32;
        let ch = 92.0 * u;
        let fits = ((room - 2.0 * pad - title + gap) / (ch + gap))
            .floor()
            .max(1.0) as usize;
        let rows = list.len().div_ceil(cols).clamp(1, fits);
        let shown = &list[..list.len().min(rows * cols)];
        let height = pad + title + rows as f32 * (ch + gap) - gap + pad;
        let r = [at[0], at[1] - height, width, height];
        o.panel([r[0], r[1]], [r[2], r[3]], 14.0 * u, PANEL);
        self.hits.push((r, Hit::Block));
        let left = r[0] + pad;
        let top = r[1] + pad;
        let px = 20.0 * u;
        o.text("Build", [left, top + o.cap_height(px) + 4.0 * u], px, TEXT);
        let close = 32.0 * u;
        let c = [r[0] + r[2] - pad - close, top - 2.0 * u, close, close];
        o.panel([c[0], c[1]], [close, close], close / 2.0, RAISED);
        let xw = o.measure("×", 22.0 * u);
        o.text(
            "×",
            [c[0] + (close - xw) / 2.0, c[1] + close * 0.72],
            22.0 * u,
            TEXT,
        );
        self.hits.push((c, Hit::Close));

        let holding = match view.tool {
            Tool::Place(b) => Some(b),
            _ => None,
        };
        for (k, d) in shown.iter().enumerate() {
            let (col, row) = (k % cols, k / cols);
            let b = [
                left + col as f32 * (cw + gap),
                top + title + row as f32 * (ch + gap),
                cw,
                ch,
            ];
            let open = view.building_open(&d.id);
            let afford = open && view.affordable(&d.cost);
            let fill = if holding == Some(&d.id) {
                PRESSED
            } else {
                RAISED
            };
            o.panel([b[0], b[1]], [b[2], b[3]], 10.0 * u, fill);
            let tint = |t: Rgba| if afford { t } else { [t[0], t[1], t[2], 0x70] };
            let icon = 32.0 * u;
            let iy = b[1] + 8.0 * u;
            if let Some((sprite, t)) = view.look.building(&d.id) {
                o.icon(sprite, [b[0] + (cw - icon) / 2.0, iy], icon, tint(t));
            }
            let npx = fit(o, &d.name, 13.0 * u, cw - 8.0 * u);
            let nw = o.measure(&d.name, npx);
            let base = iy + icon + 6.0 * u + o.cap_height(npx);
            o.text(&d.name, [b[0] + (cw - nw) / 2.0, base], npx, tint(TEXT));
            let y = base + 6.0 * u;
            if !open {
                let lpx = 11.0 * u;
                let lw = o.measure("Locked", lpx);
                o.text(
                    "Locked",
                    [b[0] + (cw - lw) / 2.0, y + lpx * 0.85],
                    lpx,
                    MUTED,
                );
            } else {
                let spx = 10.0 * u;
                let s = 10.0 * u;
                let parts: Vec<String> = d.cost.iter().map(|c| c.count.to_string()).collect();
                let total: f32 = parts
                    .iter()
                    .map(|p| s + 2.0 * u + o.measure(p, spx) + 4.0 * u)
                    .sum();
                let mut x = b[0] + (cw - total + 4.0 * u) / 2.0;
                let color = if afford { MUTED } else { palette::STARVE };
                for (st, text) in d.cost.iter().zip(&parts) {
                    if let Some((sprite, t)) = view.look.item(&st.item) {
                        o.icon(sprite, [x, y], s, t);
                    }
                    x += s + 2.0 * u;
                    x += o.text(text, [x, y + s * 0.85], spx, color) + 4.0 * u;
                }
                self.hits.push((b, Hit::Build(d.id.clone())));
            }
        }
    }

    /// The research sheet, bottom-left at `at`, at most `room` tall: one row
    /// per research with its cost and state. Tapping one that is ready
    /// sets the labs on it; tapping the current one stops them.
    fn research_sheet(
        &mut self,
        view: &View,
        o: &mut Overlay,
        at: [f32; 2],
        width: f32,
        room: f32,
    ) {
        let u = self.unit;
        let pad = 16.0 * u;
        let row = 52.0 * u;
        let title = 30.0 * u;
        let fits = ((room - 2.0 * pad - title) / row).floor().max(1.0) as usize;
        let shown = &view.content.research[..view.content.research.len().min(fits)];
        let height = pad + title + shown.len() as f32 * row - 6.0 * u + pad;
        let r = [at[0], at[1] - height, width, height];
        o.panel([r[0], r[1]], [r[2], r[3]], 14.0 * u, PANEL);
        self.hits.push((r, Hit::Block));
        let left = r[0] + pad;
        let mut y = r[1] + pad;

        let px = 20.0 * u;
        o.text("Research", [left, y + o.cap_height(px) + 4.0 * u], px, TEXT);
        let close = 32.0 * u;
        let c = [r[0] + r[2] - pad - close, y - 2.0 * u, close, close];
        o.panel([c[0], c[1]], [close, close], close / 2.0, RAISED);
        let xw = o.measure("×", 22.0 * u);
        o.text(
            "×",
            [c[0] + (close - xw) / 2.0, c[1] + close * 0.72],
            22.0 * u,
            TEXT,
        );
        self.hits.push((c, Hit::Close));
        y += title;

        let current = view.snapshot.research.current.as_ref();
        for d in shown {
            let b = [left, y, r[2] - 2.0 * pad, row - 6.0 * u];
            let is_current = current == Some(&d.id);
            let (label, color, hit) = if view.done(&d.id) {
                ("Done", WORKING, None)
            } else if is_current {
                ("Studying", palette::ACCENT, Some(Hit::Study(None)))
            } else if view.research_ready(d) {
                ("Study", TEXT, Some(Hit::Study(Some(d.id.clone()))))
            } else {
                ("Locked", MUTED, None)
            };
            let fill = if is_current { PRESSED } else { RAISED };
            o.panel([b[0], b[1]], [b[2], b[3]], 10.0 * u, fill);
            let npx = 15.0 * u;
            let ink = if label == "Locked" { MUTED } else { TEXT };
            o.text(
                &d.name,
                [b[0] + 12.0 * u, b[1] + 6.0 * u + o.cap_height(npx)],
                npx,
                ink,
            );
            // Cost per unit, times the units.
            let spx = 12.0 * u;
            let s = 14.0 * u;
            let sy = b[1] + b[3] - 8.0 * u - s;
            let mut x = b[0] + 12.0 * u;
            x += o.text(&format!("{}×", d.units), [x, sy + s * 0.85], spx, MUTED) + 4.0 * u;
            for st in &d.cost {
                if let Some((sprite, t)) = view.look.item(&st.item) {
                    o.icon(sprite, [x, sy], s, t);
                }
                x += s + 2.0 * u;
                x += o.text(&st.count.to_string(), [x, sy + s * 0.85], spx, MUTED) + 6.0 * u;
            }
            let lpx = 13.0 * u;
            let lw = o.measure(label, lpx);
            o.text(
                label,
                [
                    b[0] + b[2] - 12.0 * u - lw,
                    b[1] + (b[3] + o.cap_height(lpx)) / 2.0,
                ],
                lpx,
                color,
            );
            if let Some(hit) = hit {
                self.hits.push((b, hit));
            }
            y += row;
        }
    }

    /// Recipe pills, bottom-left at `at`, at most `width` wide, wrapping
    /// upwards so they stay near the thumb. Returns how many rows they take;
    /// with `draw` false, only measures.
    fn pills(
        &mut self,
        o: &mut Overlay,
        choices: Vec<Pill>,
        at: [f32; 2],
        width: f32,
        draw: bool,
    ) -> usize {
        let u = self.unit;
        let h = 40.0 * u;
        let px = 14.0 * u;
        let s = 18.0 * u;
        let gap = 6.0 * u;
        let sized: Vec<_> = choices
            .into_iter()
            .map(|(name, icon, chosen, hit)| {
                let w =
                    14.0 * u + icon.map_or(0.0, |_| s + 6.0 * u) + o.measure(name, px) + 14.0 * u;
                (name, icon, w, chosen, hit)
            })
            .collect();
        // Fill rows from the bottom up so the pills stay near the thumb.
        let mut rows: Vec<Vec<_>> = vec![Vec::new()];
        let mut used = 0.0;
        for p in sized {
            if used > 0.0 && used + p.2 > width {
                rows.push(Vec::new());
                used = 0.0;
            }
            used += p.2 + gap;
            rows.last_mut().expect("one row").push(p);
        }
        let count = rows.len();
        if !draw {
            return count;
        }
        let mut y = at[1] - h;
        for row in rows {
            let mut x = at[0];
            for (name, icon, w, chosen, hit) in row {
                let (fill, ink) = if chosen {
                    (palette::ACCENT, palette::GROUND)
                } else {
                    (RAISED, TEXT)
                };
                o.panel([x, y], [w, h], h / 2.0, fill);
                let mut tx = x + 14.0 * u;
                if let Some((sprite, t)) = icon {
                    o.icon(sprite, [tx, y + (h - s) / 2.0], s, t);
                    tx += s + 6.0 * u;
                }
                o.text(name, [tx, y + (h + o.cap_height(px)) / 2.0], px, ink);
                self.hits.push(([x, y, w, h], hit));
                x += w + gap;
            }
            y -= h + gap;
        }
        count
    }

    /// The inspect sheet for `e`, bottom-left at `at`.
    fn sheet(&mut self, view: &View, o: &mut Overlay, e: &EntityView, at: [f32; 2], width: f32) {
        let u = self.unit;
        let pad = 16.0 * u;
        let def = view.building(&e.building);
        let hub = def.is_some_and(|d| d.role == BuildingRole::Hub);
        // Belts, splitters and underground belts only carry: nothing to show but the controls.
        let belt = def.is_some_and(|d| {
            matches!(
                d.role,
                BuildingRole::Belt { .. }
                    | BuildingRole::Splitter
                    | BuildingRole::Underground { .. }
            )
        });
        let pole = def.is_some_and(|d| matches!(d.role, BuildingRole::Pole { .. }));
        let generator = def.and_then(|d| match d.role {
            BuildingRole::Generator { kw } => Some(kw),
            _ => None,
        });
        // Poles and generators have no state of their own to show.
        let wired = pole || generator.is_some();
        let grid = e.grid.and_then(|g| view.snapshot.grids.get(g as usize));
        let recipes = view.recipes_for(&e.building);
        let recipe = e.recipe.as_ref().and_then(|r| view.recipe_def(r));

        // Measure the sheet from its rows, bottom up.
        let row = 22.0 * u;
        let choices: Option<Vec<_>> = recipes.as_ref().map(|rs| {
            std::iter::once(None)
                .chain(rs.iter().map(|r| Some(*r)))
                .map(|r| {
                    let id = r.map(|r| r.id.clone());
                    let chosen = id == e.recipe;
                    recipe_pill(view, r, chosen, Hit::SetRecipe(e.id, id))
                })
                .collect()
        });
        let pill_rows = choices.clone().map_or(0.0, |c| {
            self.pills(o, c, [0.0, 0.0], width - 2.0 * pad, false) as f32
        });
        let height = pad
            + 30.0 * u // title
            + if belt { 0.0 } else { row + 8.0 * u + 6.0 * u } // recipe line and progress
            + pill_rows * 46.0 * u
            + 44.0 * u // buttons
            + pad;
        let r = [at[0], at[1] - height, width, height];
        o.panel([r[0], r[1]], [r[2], r[3]], 14.0 * u, PANEL);
        self.hits.push((r, Hit::Block));
        let left = r[0] + pad;
        let mut y = r[1] + pad;

        // Title, its state, and a close button.
        let name = def.map_or(e.building.as_str(), |d| d.name.as_str());
        let px = 20.0 * u;
        o.text(name, [left, y + o.cap_height(px) + 4.0 * u], px, TEXT);
        let close = 32.0 * u;
        let c = [r[0] + r[2] - pad - close, y - 2.0 * u, close, close];
        o.panel([c[0], c[1]], [close, close], close / 2.0, RAISED);
        let xw = o.measure("×", 22.0 * u);
        o.text(
            "×",
            [c[0] + (close - xw) / 2.0, c[1] + close * 0.72],
            22.0 * u,
            TEXT,
        );
        self.hits.push((c, Hit::Close));
        if !belt && !hub && !wired {
            let (label, color) = state(e.state);
            let spx = 12.0 * u;
            let sw = o.measure(label, spx) + 16.0 * u;
            let sx = c[0] - 8.0 * u - sw;
            o.panel(
                [sx, y + 3.0 * u],
                [sw, 22.0 * u],
                11.0 * u,
                [color[0], color[1], color[2], 0x40],
            );
            o.text(
                label,
                [
                    sx + 8.0 * u,
                    y + 3.0 * u + (22.0 * u + o.cap_height(spx)) / 2.0,
                ],
                spx,
                color,
            );
        }
        y += 30.0 * u;

        // What it makes, and how far along.
        if !belt {
            let px = 14.0 * u;
            let s = 18.0 * u;
            let base = y + (row + o.cap_height(px)) / 2.0;
            let mut x = left;
            if hub {
                o.text(
                    "Everything delivered here joins your stock.",
                    [x, base],
                    px,
                    MUTED,
                );
            } else if wired {
                let text = match (grid, generator) {
                    (Some(g), _) => format!(
                        "Grid: {:.0} of {:.0} kW asked for · {:.0}% met",
                        g.demand_kw,
                        g.supply_kw,
                        g.satisfaction * 100.0
                    ),
                    (None, Some(kw)) => format!("Gives {kw:.0} kW once a pole covers it"),
                    (None, None) => "Not linked to anything".to_string(),
                };
                let px = fit(o, &text, px, r[2] - 2.0 * pad);
                o.text(&text, [x, base], px, MUTED);
            } else if let Some(rd) = recipe {
                let stacks = |o: &mut Overlay, x: &mut f32, list: &[Stack]| {
                    for st in list {
                        if let Some((sprite, t)) = view.look.item(&st.item) {
                            o.icon(sprite, [*x, y + (row - s) / 2.0], s, t);
                        }
                        *x += s + 2.0 * u;
                        *x += o.text(&format!("{}", st.count), [*x, base], px, TEXT) + 8.0 * u;
                    }
                };
                stacks(o, &mut x, &rd.inputs);
                x += o.text("→", [x, base], px, MUTED) + 8.0 * u;
                stacks(o, &mut x, &rd.outputs);
                o.text(
                    &format!("{} · {:.1}s", rd.name, rd.seconds),
                    [x + 4.0 * u, base],
                    px,
                    MUTED,
                );
            } else if recipes.is_some() {
                o.text("Picks its recipe from what arrives", [x, base], px, MUTED);
            } else {
                o.text("Mines the ore under it", [x, base], px, MUTED);
            }
            y += row + 4.0 * u;
            let bw = r[2] - 2.0 * pad;
            o.panel([left, y], [bw, 6.0 * u], 3.0 * u, RAISED);
            if !hub && !wired && e.progress > 0.0 {
                let fill = state(e.state).1;
                o.panel(
                    [left, y],
                    [bw * e.progress.clamp(0.0, 1.0), 6.0 * u],
                    3.0 * u,
                    fill,
                );
            }
            y += 6.0 * u + 8.0 * u;
        }

        // Recipe pills for crafters.
        if let Some(choices) = choices {
            // Pills fill upwards from a bottom edge; aim them at this band.
            let band = pill_rows * 46.0 * u;
            self.pills(
                o,
                choices,
                [left, y + band - 6.0 * u],
                r[2] - 2.0 * pad,
                true,
            );
            y += band;
        }

        // Rotate and remove.
        if !hub {
            let bh = 40.0 * u;
            let bw = (r[2] - 2.0 * pad - 8.0 * u) / 2.0;
            let buttons = [
                ("Rotate", Hit::Turn(e.id, e.dir), TEXT),
                ("Remove", Hit::Remove(e.id), palette::STARVE),
            ];
            for (i, (label, hit, ink)) in buttons.into_iter().enumerate() {
                let b = [left + i as f32 * (bw + 8.0 * u), y, bw, bh];
                o.panel([b[0], b[1]], [bw, bh], 10.0 * u, RAISED);
                let px = 15.0 * u;
                let lw = o.measure(label, px);
                o.text(
                    label,
                    [b[0] + (bw - lw) / 2.0, b[1] + (bh + o.cap_height(px)) / 2.0],
                    px,
                    ink,
                );
                self.hits.push((b, hit));
            }
        }
    }
}

/// A pill: its label, icon, whether it is the one chosen, and what a tap does.
type Pill<'a> = (&'a str, Option<(&'a str, Rgba)>, bool, Hit);

/// A pill for a recipe, showing its first output; `None` is Auto.
fn recipe_pill<'a>(view: &View<'a>, r: Option<&'a RecipeDef>, chosen: bool, hit: Hit) -> Pill<'a> {
    let name = r.map_or("Auto", |r| r.name.as_str());
    let icon = r
        .and_then(|r| r.outputs.first())
        .and_then(|s| view.look.item(&s.item));
    (name, icon, chosen, hit)
}

/// A machine state's label and colour.
fn state(s: MachineState) -> (&'static str, Rgba) {
    match s {
        MachineState::Idle => ("Idle", MUTED),
        MachineState::Working => ("Working", WORKING),
        MachineState::Starved => ("Starved", palette::STARVE),
        MachineState::Blocked => ("Output full", palette::ACCENT),
        MachineState::NoPower => ("No power", palette::STARVE),
    }
}

/// The tip for the tool in hand, from the prototype.
fn tip(tool: &Tool) -> &'static str {
    match tool {
        Tool::Hand => "Drag to look around, pinch or scroll to zoom. Tap a machine to inspect it.",
        Tool::Belt => "Drag to draw a belt along your path. Two fingers pan and zoom.",
        Tool::Place(b) if b.as_str() == "drill" => {
            "Tap ore to place a drill. It mines onto the tile it faces."
        }
        Tool::Place(b) if b.as_str() == "furnace" => {
            "Tap to place. Ore goes in any side, plates come out of the arrow."
        }
        Tool::Place(b) if b.as_str() == "lab" => {
            "Tap to place. Belt research packs into it, and give it power."
        }
        Tool::Place(b) if b.as_str() == "pole" => {
            "Tap to place. Poles link up to 7 tiles apart and power what's next to them."
        }
        Tool::Place(b) if b.as_str() == "splitter" => {
            "Tap to place facing along a belt. Items go out left and right in turn."
        }
        Tool::Place(b) if b.as_str() == "underground-belt" => {
            "Place an entrance, then an exit up to 5 tiles on facing the same way."
        }
        Tool::Place(b) if b.as_str() == "wind-turbine" => {
            "Tap to place next to a pole. Each turbine powers one lab."
        }
        Tool::Place(_) => "Tap to place. Pick a recipe, or let it pick from what arrives.",
        Tool::Remove => "Tap or drag over things to remove them. You get the plates back.",
    }
}

/// The largest size up to `px` at which `text` fits in `width`.
fn fit(o: &Overlay, text: &str, px: f32, width: f32) -> f32 {
    let w = o.measure(text, px);
    if w <= width {
        px
    } else {
        (px * width / w).floor().max(6.0)
    }
}

/// A chip of text at `at`, wrapped to `width`. Returns its bottom edge.
fn chip(o: &mut Overlay, text: &str, at: [f32; 2], width: f32, u: f32, ink: Rgba) -> f32 {
    let px = 14.0 * u;
    let pad = 10.0 * u;
    let line = 20.0 * u;
    let lines = wrap(o, text, px, (width - 2.0 * pad).max(px * 4.0));
    let w = lines.iter().map(|l| o.measure(l, px)).fold(0.0, f32::max) + 2.0 * pad;
    let h = lines.len() as f32 * line + 2.0 * pad - (line - o.cap_height(px)) + 2.0 * u;
    o.panel(at, [w, h], 8.0 * u, PANEL);
    for (i, l) in lines.iter().enumerate() {
        let base = at[1] + pad + o.cap_height(px) + i as f32 * line;
        o.text(l, [at[0] + pad, base], px, ink);
    }
    at[1] + h
}

/// Break `text` into lines no wider than `width` at `px`.
fn wrap(o: &Overlay, text: &str, px: f32, width: f32) -> Vec<String> {
    let mut lines = vec![String::new()];
    for word in text.split_whitespace() {
        let last = lines.last_mut().expect("one line");
        let candidate = if last.is_empty() {
            word.to_string()
        } else {
            format!("{last} {word}")
        };
        if !last.is_empty() && o.measure(&candidate, px) > width {
            lines.push(word.to_string());
        } else {
            *last = candidate;
        }
    }
    lines
}

#[cfg(test)]
mod tests;
