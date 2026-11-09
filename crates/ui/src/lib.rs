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
use mofact_core::content::{
    Bonus, BuildingDef, BuildingRole, Content, RecipeDef, ResearchDef, SplitMode, Stack,
};
use mofact_core::geom::Dir;
use mofact_core::ids::{BuildingId, EntityId, ItemId, RecipeId, ResearchId};
use mofact_core::snapshot::{EntityView, MachineState, Snapshot};
use mofact_input::{Pointer, Tool, MOUSE_PAN};
use mofact_render::look;
use mofact_render::palette::{self, Rgba};
use mofact_render::{Look, Overlay};
use web_time::{Duration, Instant};

pub use i18n::{tr, trf};

pub use mofact_core as core;

const PANEL: Rgba = [0x1d, 0x23, 0x2a, 0xeb];
const RAISED: Rgba = [0x2a, 0x31, 0x39, 0xff];
const PRESSED: Rgba = [0x3a, 0x44, 0x4f, 0xff];
const TEXT: Rgba = [0xe9, 0xed, 0xf0, 0xff];
const MUTED: Rgba = [0x8a, 0x95, 0xa1, 0xff];
const WORKING: Rgba = [0x63, 0xd4, 0x8a, 0xff];
/// What the start menu lays over the factory behind it.
const TITLE_DIM: Rgba = [0x10, 0x14, 0x18, 0xc8];
/// The least a control's tap target spans, in points (R30): Apple's and
/// Google's guidance for a fingertip. Smaller controls take taps from a
/// finger-sized area round them.
pub const FINGER: f32 = 44.0;

/// How much bigger large text draws the HUD.
pub const LARGE_TEXT: f32 = 1.2;
/// The height of the status chips under the stock, in points.
const CHIP_H: f32 = 36.0;
/// A finger that moves further than this (in points) is dragging, not tapping.
const TAP_SLOP: f32 = 12.0;
const TOAST_FOR: Duration = Duration::from_millis(2500);
/// A finger held still on the world this long opens the radial build menu.
pub const LONG_PRESS: Duration = Duration::from_millis(450);

/// The toolbar, left to right (top to bottom in landscape): the label's
/// string key and the building it places.
const TOOLS: [(&str, Option<&str>); 8] = [
    ("tool.move", None),
    ("tool.belt", Some("belt")),
    ("tool.drill", Some("drill")),
    ("tool.furnace", Some("furnace")),
    ("tool.assembler", Some("assembler")),
    ("tool.more", None),
    ("tool.select", None),
    ("tool.remove", None),
];
/// The slot that opens the build sheet with every other building, and
/// shows the one in hand when it is one of those.
const MORE: usize = 5;
/// The select tool (T51), which also stands for a blueprint in hand.
const SELECT: usize = 6;
/// The last tool, Remove.
const REMOVE: usize = TOOLS.len() - 1;

fn tool_at(i: usize) -> Tool {
    match i {
        0 => Tool::Hand,
        1 => Tool::Belt,
        SELECT => Tool::Select,
        REMOVE => Tool::Remove,
        _ => Tool::Place(TOOLS[i].1.unwrap_or_default().into()),
    }
}

/// One clock for everything that pulses.
fn pulse_clock() -> Instant {
    static AT: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    *AT.get_or_init(Instant::now)
}

fn tool_index(tool: &Tool) -> Option<usize> {
    match tool {
        Tool::Hand => Some(0),
        Tool::Belt => Some(1),
        Tool::Remove => Some(REMOVE),
        Tool::Select | Tool::Paste => Some(SELECT),
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
    /// Whether there is an action to undo, and one to redo.
    pub history: (bool, bool),
    /// The edges the display covers or rounds off (a notch, the home
    /// indicator), in pixels: top, right, bottom, left.
    pub safe: [f32; 4],
    /// A toolbar tool to point the player at: the coach's next step.
    pub nudge: Option<Tool>,
    /// The save slots, for the game menu (T63).
    pub slots: &'a [SlotView],
    /// The demo and the full game's purchase (T61).
    pub store: &'a StoreView,
    /// Whether a crash report from last time is waiting to be sent.
    pub crash: bool,
    /// How many buildings the select tool has selected (T51).
    pub selection: usize,
    /// The blueprint library.
    pub blueprints: &'a [mofact_input::Blueprint],
}

/// Whether this is the demo, and how buying the full game is going.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StoreView {
    /// The demo's last open tier; none in the full game.
    pub demo: Option<u8>,
    /// The full game's price, as the store words it, once it is known.
    pub price: Option<String>,
    /// A purchase or restore is under way.
    pub busy: bool,
    /// What happened last, such as a cancelled or failed purchase.
    pub note: Option<String>,
}

/// One save slot as the game menu shows it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SlotView {
    /// Whether a game is saved there.
    pub used: bool,
    /// Whether it is the game being played.
    pub current: bool,
    /// What it holds: the goal it is on, and how long it has run.
    pub title: String,
    pub detail: String,
}

/// How many save slots there are.
pub const SLOTS: u8 = 3;

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

    /// Whether `r` is past the demo's last tier.
    fn gated(&self, r: &ResearchDef) -> bool {
        self.store.demo.is_some_and(|t| r.tier > t)
    }

    /// Whether `r` can be picked now: not done, and its prerequisites are.
    fn research_ready(&self, r: &ResearchDef) -> bool {
        (!self.done(&r.id) || r.repeat.is_some()) && r.requires.iter().all(|q| self.done(q))
    }

    /// The levels done of a repeatable research (S2).
    fn level(&self, id: &ResearchId) -> u32 {
        let levels = &self.snapshot.research.levels;
        levels.iter().find(|(r, _)| r == id).map_or(0, |l| l.1)
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
    /// A long press opened the radial menu: drop the gesture its finger started.
    Cancel,
    /// Take back the latest action, or put it back.
    Undo,
    Redo,
    /// Centre the camera on this point, in tiles: a problem's whereabouts.
    Focus([f32; 2]),
    /// Turn the game's sound off (true) or back on.
    Mute(bool),
    /// Open the world map (true): zoom far out. Or close it and zoom back in.
    Map(bool),
    /// Turn low-power mode on (true) or off: 30 fps and calmer ground.
    LowPower(bool),
    /// Turn colour-blind mode on (true) or off (T58).
    ColourBlind(bool),
    /// Turn large text on (true) or off.
    LargeText(bool),
    /// Turn reduce motion on (true) or off.
    ReduceMotion(bool),
    /// Set how loud the game is, in percent (0 to 100).
    Volume(u8),
    /// Turn the land's ambient sound on (true) or off.
    Ambience(bool),
    /// Turn vibration on (true) or off.
    Vibration(bool),
    /// The player picked a language, or to follow the device's (T65).
    Language(i18n::Choice),
    /// The game menu opened: the slots it lists want refreshing.
    MenuOpened,
    /// Save the game being played and switch to the one in this slot.
    LoadSlot(u8),
    /// Wipe the game in this slot.
    DeleteSlot(u8),
    /// Save the game being played and start a new one in this slot, on
    /// the map from this seed and preset (by number, see `mofact_world::PRESETS`),
    /// under these rules: whether ore runs out, and the pace of the hub's
    /// orders by number (see [`PACE_NAMES`]).
    NewGame {
        slot: u8,
        seed: u64,
        preset: u8,
        ore_runs_out: bool,
        orders: u8,
    },
    /// Buy the full game.
    Buy,
    /// Look for a full game bought before, on this account.
    Restore,
    /// Copy last time's crash report, to paste into an email or a bug.
    CopyCrash,
    /// Take a copy of the selection in hand, to paste (T51).
    Copy,
    /// Take the selection in hand to move it: building it moves the originals.
    Cut,
    /// Remove everything selected, as one undo step.
    DeleteSelection,
    /// Keep the selection in the blueprint library.
    SaveBlueprint,
    /// Build the blueprint in hand where it is.
    PlacePaste,
    /// Take the library's blueprint at this place in it in hand.
    Paste(usize),
    /// Delete the library's blueprint at this place in it.
    ForgetBlueprint(usize),
}

/// A control on screen, and what tapping it does.
#[derive(Clone, Debug, PartialEq)]
enum Hit {
    Tool(usize),
    /// The stock's "+N" pill: show every item, or one row again.
    StockMore,
    Rotate,
    Recipe(Option<RecipeId>),
    Belt(BuildingId),
    SetRecipe(EntityId, Option<RecipeId>),
    /// What a requester chest asks drones for (T49).
    SetRequest(EntityId, Option<(mofact_core::ids::ItemId, u32)>),
    /// What a filter splitter sends ahead.
    SetFilter(EntityId, Option<mofact_core::ids::ItemId>),
    Turn(EntityId, Dir),
    Remove(EntityId),
    /// The research button: opens or closes the research sheet.
    Research,
    /// Set what labs work on.
    Study(Option<ResearchId>),
    /// A research row past the demo: opens the full game sheet.
    Unlock,
    /// The full game sheet's buttons.
    Buy,
    Restore,
    /// Pick a building from the build sheet.
    Build(BuildingId),
    /// An item's icon: opens the recipe browser on it.
    Item(ItemId),
    /// The ratio calculator's rate buttons: up or down a step.
    Rate(bool),
    /// A sheet's page arrows: back or on a page.
    Page(bool),
    Close,
    /// Undo and redo buttons.
    Undo,
    Redo,
    /// Put the tool away and go back to Move.
    Done,
    /// The problems button: opens or closes the problems list.
    Problems,
    /// The goal card: shows or hides how to meet it.
    Goal,
    /// A line of the problems list, by its place in it.
    Problem(usize),
    /// The sound button: turns sound off and on.
    Sound,
    /// Give a train this schedule.
    Schedule(EntityId, Vec<mofact_core::command::TrainStop>),
    /// Start or stop picking a stop on the map for a train's schedule.
    PickStop(EntityId),
    /// The map button: opens and closes the world map.
    Map,
    /// The low-power toggle: turns low-power mode on and off.
    Power,
    /// The colour-blind toggle in the game menu.
    ColourBlind,
    /// The large text toggle in the game menu.
    LargeText,
    /// The reduce motion toggle in the game menu.
    Motion,
    /// The game menu's crash report button.
    CopyCrash,
    /// The game menu's settings button, and the settings sheet's close.
    Settings,
    SettingsClose,
    /// The settings sheet's volume, ambience and vibration.
    Volume,
    Ambience,
    Vibration,
    /// The menu button: opens and closes the game menu.
    Menu,
    /// The language button in the game menu: steps to the next language.
    Language,
    /// A slot's buttons in the game menu.
    SlotLoad(u8),
    SlotDelete(u8),
    SlotNew(u8),
    /// The new-game form: a keypad digit, backspace, a random seed, a
    /// preset, cancel and start.
    Digit(u8),
    Backspace,
    Dice,
    Preset(u8),
    /// The new-game rules: whether ore runs out, and the orders' pace.
    RuleOre,
    RuleOrders,
    CancelNew,
    StartNew,
    /// The selection's and blueprint's buttons (T51).
    Copy,
    Cut,
    Erase,
    Keep,
    Place,
    /// Open or close the blueprint library.
    Library,
    /// A blueprint in the library: take it in hand, or delete it (a
    /// second tap deletes).
    Pick(usize),
    Forget(usize),
    /// The start menu's buttons: play on, a new game, the saves, the
    /// settings, and the full game.
    TitleContinue,
    TitleNew,
    TitleLoad,
    TitleSettings,
    TitleBuy,
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

/// A new game being set up in the game menu.
#[derive(Clone, Debug, Default)]
struct NewGame {
    slot: u8,
    seed: String,
    preset: u8,
    ore_runs_out: bool,
    orders: u8,
}

impl NewGame {
    /// A new game in `slot` on `seed`, by the standard map and rules.
    fn new(slot: u8, seed: String) -> Self {
        Self {
            slot,
            seed,
            preset: 0,
            ore_runs_out: true,
            orders: PACE_NORMAL,
        }
    }
}

/// A setting on the settings sheet: its words, whether it shows as on,
/// and what tapping it does.
type Setting = (String, bool, Hit);

/// The names of the hub orders' paces, by number: off, relaxed, normal
/// and hard (see `mofact_sim::OrderPace`).
pub const PACE_NAMES: [&str; 4] = ["setting.off", "pace.relaxed", "pace.normal", "pace.hard"];
/// The pace a new game starts with.
const PACE_NORMAL: u8 = 2;
/// The steps the volume button goes through, loudest first.
const VOLUMES: [u8; 5] = [100, 75, 50, 25, 0];

thread_local! {
    static START: Instant = Instant::now();
}

/// A random-looking seed for dice roll `n`: the clock and the count mixed.
fn roll(n: u64) -> u64 {
    let since = START.with(|t| t.elapsed().as_nanos() as u64);
    let mut x = n.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ since;
    x ^= x >> 31;
    x = x.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x ^= x >> 29;
    x % 1_000_000_000
}

/// Seeds are typed up to this many digits.
const SEED_DIGITS: usize = 9;

/// The names of the world presets, in `mofact_world::PRESETS` order.
pub const PRESET_NAMES: [&str; 4] = [
    "preset.standard",
    "preset.rich",
    "preset.lakes",
    "preset.dry",
];

/// The name of map preset `n`, in the current language.
pub fn preset_name(n: u8) -> &'static str {
    tr(PRESET_NAMES
        .get(usize::from(n))
        .copied()
        .unwrap_or(PRESET_NAMES[0]))
}

/// A touch on the world that may turn out to be a tap.
#[derive(Debug)]
struct Tap {
    id: u64,
    start: [f32; 2],
    at: Instant,
}

/// The heads-up display: toolbar, rotate button, recipe pills, stock, tips and
/// the inspect sheet.
#[derive(Debug, Default)]
pub struct Hud {
    /// Whether the player turned the sound off.
    muted: bool,
    /// Whether the camera is out on the world map.
    map: bool,
    /// Whether the stock shows every item rather than one row.
    stock_open: bool,
    /// Whether low-power mode is on.
    low_power: bool,
    /// Whether colour-blind mode is on (T58).
    colour_blind: bool,
    /// Whether the player wants the HUD's words and buttons bigger.
    large_text: bool,
    /// Whether the player wants the world to hold still: no swaying trees,
    /// rippling water or pulsing hints.
    reduce_motion: bool,
    /// Volume steps down from full (see [`VOLUMES`]), and whether the
    /// land's ambient sound and vibration are turned off.
    quieter: u8,
    ambience_off: bool,
    vibration_off: bool,
    /// Whether the settings sheet is open, and whether it was opened from
    /// the game menu, so closing it goes back there.
    settings: bool,
    settings_from_menu: bool,
    /// The language the player chose, and the device's (T65).
    language: i18n::Choice,
    device: i18n::Lang,
    /// Whether the game menu is open, the slot asked to be deleted (a
    /// second tap deletes it), and the new game being set up.
    menu: bool,
    confirm_delete: Option<u8>,
    /// Whether the start menu covers the screen, and whether the sheet
    /// open now was opened from it, so closing that sheet goes back to it.
    title: bool,
    from_title: bool,
    /// The first empty save slot, as of the last frame drawn.
    empty_slot: Option<u8>,
    new_game: Option<NewGame>,
    /// Dice rolls so far, so each roll gives another seed.
    rolls: u64,
    /// The entity the inspect sheet shows.
    selected: Option<EntityId>,
    /// The train waiting for a stop to be tapped on the map.
    picking: Option<EntityId>,
    /// Whether the research sheet is open.
    research: bool,
    /// Whether the full game sheet is open.
    unlock: bool,
    /// Whether the build sheet is open.
    more: bool,
    /// The page the build or research sheet shows, when it has more rows
    /// than fit on the screen (a phone held sideways fits one row).
    page: usize,
    /// Whether the problems list is open.
    problems: bool,
    /// Whether the blueprint library is open, and the blueprint asked to
    /// be deleted (a second tap deletes it).
    library: bool,
    confirm_forget: Option<usize>,
    /// Whether the goal card shows how to meet it, and the goal it last showed.
    hint: bool,
    goal_seen: Option<u16>,
    /// Orders shipped and missed as last drawn, to toast when one ends.
    orders_seen: Option<(u32, u32)>,
    /// What's stuck, as of the last frame drawn.
    found: Vec<Problem>,
    /// What [`Hud::found`] was worked out from: the time in quarter seconds,
    /// how much is built, and a digest of every machine's state.
    found_for: Option<(u64, usize, u64, usize, i18n::Lang)>,
    /// The line last jumped to and which of its places, so another tap
    /// on it goes to the next one.
    jumped: Option<(problems::Trouble, Option<BuildingId>, usize)>,
    /// Where the radial build menu is open, in pixels.
    radial: Option<[f32; 2]>,
    /// The item the recipe browser shows.
    browse: Option<ItemId>,
    /// The ratio calculator's rate, as an index into [`plan::RATES`].
    rate: Option<usize>,
    /// Where each control went in the last frame, topmost last.
    hits: Vec<(Rect, Hit)>,
    press: Option<Press>,
    tap: Option<Tap>,
    /// A short message, when it went up, and its colour.
    toast: Option<(String, Instant, Rgba)>,
    /// Pixels per point in the last frame.
    unit: f32,
    /// The least a tap target spans, in pixels, and the screen it is on.
    finger: f32,
    screen: [f32; 2],
    /// The toolbar slot of the tool in hand in the last frame.
    current: Option<usize>,
    /// Whether a long press may open the radial menu: only with the hand
    /// or a building in hand, since with belts or remove a finger held
    /// still is the start of a drag.
    radial_ok: bool,
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
            self.problems = false;
        }
    }

    /// Whether the research sheet is open.
    pub fn researching(&self) -> bool {
        self.research
    }

    /// The ratio calculator's rate, as an index into [`plan::RATES`].
    fn rate(&self) -> usize {
        self.rate.unwrap_or(plan::START).min(plan::RATES.len() - 1)
    }

    /// The ratio calculator's rate in items a minute.
    pub fn rate_per_minute(&self) -> u32 {
        plan::RATES[self.rate()]
    }

    /// Open the recipe browser on `item`, as tapping its icon does.
    pub fn browse(&mut self, item: ItemId) {
        self.tapped(Hit::Item(item));
    }

    /// Whether the player turned the sound off.
    pub fn muted(&self) -> bool {
        self.muted
    }

    /// Turn the sound off or on, as its button does (for the M key, and to
    /// restore what the player chose last time).
    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
    }

    pub fn low_power(&self) -> bool {
        self.low_power
    }

    /// Turn low-power mode on or off, as a saved setting does at start.
    pub fn set_low_power(&mut self, on: bool) {
        self.low_power = on;
    }

    /// Whether colour-blind mode is on (T58).
    pub fn colour_blind(&self) -> bool {
        self.colour_blind
    }

    /// Turn colour-blind mode on or off, as a platform does from a saved setting.
    pub fn set_colour_blind(&mut self, on: bool) {
        self.colour_blind = on;
    }

    /// Whether large text is on.
    pub fn large_text(&self) -> bool {
        self.large_text
    }

    /// Turn large text on or off, as a platform does from a saved setting.
    pub fn set_large_text(&mut self, on: bool) {
        self.large_text = on;
    }

    /// Whether reduce motion is on.
    pub fn reduce_motion(&self) -> bool {
        self.reduce_motion
    }

    /// Turn reduce motion on or off, as a platform does from a saved setting.
    pub fn set_reduce_motion(&mut self, on: bool) {
        self.reduce_motion = on;
    }

    /// The language the player chose: the device's, or one of their own.
    pub fn language(&self) -> i18n::Choice {
        self.language
    }

    /// Choose the language, as a saved setting does at start, and switch to it.
    pub fn set_language(&mut self, choice: i18n::Choice) {
        self.language = choice;
        i18n::set_lang(choice.resolve(self.device));
    }

    /// Tell the HUD the device's language, from its preferred locales.
    pub fn set_device_lang(&mut self, lang: i18n::Lang) {
        self.device = lang;
        i18n::set_lang(self.language.resolve(lang));
    }

    /// Whether the game menu is open.
    pub fn menu_open(&self) -> bool {
        self.menu
    }

    /// How loud the game is, in percent.
    pub fn volume(&self) -> u8 {
        VOLUMES[usize::from(self.quieter).min(VOLUMES.len() - 1)]
    }

    /// Set the volume, as the settings sheet does, to the nearest of its
    /// steps (to restore what the player chose last time).
    pub fn set_volume(&mut self, percent: u8) {
        self.quieter = VOLUMES
            .iter()
            .enumerate()
            .min_by_key(|(_, v)| v.abs_diff(percent))
            .map_or(0, |(i, _)| i as u8);
    }

    /// Whether the land's ambient sound plays.
    pub fn ambience(&self) -> bool {
        !self.ambience_off
    }

    pub fn set_ambience(&mut self, on: bool) {
        self.ambience_off = !on;
    }

    /// Whether the game may vibrate the phone.
    pub fn vibration(&self) -> bool {
        !self.vibration_off
    }

    pub fn set_vibration(&mut self, on: bool) {
        self.vibration_off = !on;
    }

    /// Whether the settings sheet is open.
    pub fn settings_open(&self) -> bool {
        self.settings
    }

    /// Open the settings sheet, as the start menu's and the game menu's
    /// settings buttons do.
    pub fn open_settings(&mut self) {
        self.title = false;
        self.menu = false;
        self.settings = true;
    }

    /// Whether the start menu is up.
    pub fn title_open(&self) -> bool {
        self.title
    }

    /// Put up the start menu, as the game does when it starts.
    pub fn open_title(&mut self) {
        self.title = true;
        self.from_title = false;
        self.menu = false;
        self.settings = false;
        self.settings_from_menu = false;
        self.new_game = None;
        self.confirm_delete = None;
        self.selected = None;
        self.research = false;
        self.unlock = false;
        self.problems = false;
        self.more = false;
        self.library = false;
        self.browse = None;
        self.radial = None;
    }

    /// Close the start menu and play, as its Continue button does.
    pub fn close_title(&mut self) {
        self.title = false;
        self.from_title = false;
    }

    /// Open the game menu, or with `new_game` its new-game form for that
    /// slot and seed, as a screenshot or a test does.
    pub fn open_menu(&mut self, new_game: Option<(u8, u64)>) {
        self.menu = true;
        self.new_game = new_game.map(|(slot, seed)| NewGame::new(slot, seed.to_string()));
    }

    /// Tell the HUD whether the camera is out on the world map.
    /// Fold the card for goal `index` down to its title, as a tap on it
    /// does, for a screenshot that shows the factory rather than the hint.
    pub fn fold_goal(&mut self, index: u16) {
        self.goal_seen = Some(index);
        self.hint = false;
    }

    pub fn set_map(&mut self, map: bool) {
        self.map = map;
    }

    /// Open or close the problems list, as its button does.
    pub fn toggle_problems(&mut self) {
        self.tapped(Hit::Problems);
    }

    /// Open the full game sheet, as a research row past the demo does.
    pub fn open_unlock(&mut self) {
        self.tapped(Hit::Unlock);
    }

    /// Open or close the research sheet, as its button does.
    pub fn toggle_research(&mut self) {
        self.tapped(Hit::Research);
    }

    /// Call once a frame before drawing. A finger held still on the world
    /// for [`LONG_PRESS`] by `now` opens the radial build menu where it is,
    /// if the hand or a building was in hand in the last frame drawn;
    /// the game must then drop the gesture that finger started.
    pub fn poll(&mut self, now: Instant) -> Vec<Action> {
        match &self.tap {
            Some(tap) if self.radial_ok && now.saturating_duration_since(tap.at) >= LONG_PRESS => {
                self.radial = Some(tap.start);
                self.tap = None;
                self.selected = None;
                self.more = false;
                self.research = false;
                vec![Action::Cancel]
            }
            _ => Vec::new(),
        }
    }

    /// Whether the radial build menu is open.
    pub fn radial_open(&self) -> bool {
        self.radial.is_some()
    }

    /// Open or close the build sheet, as the More slot does.
    pub fn toggle_build_sheet(&mut self) {
        self.tapped(Hit::Tool(MORE));
    }

    /// Show a short message, such as why a build was refused.
    pub fn toast(&mut self, text: impl Into<String>) {
        self.toast = Some((text.into(), Instant::now(), palette::STARVE));
    }

    /// Where a tap on the control drawn at `r` counts: `r`, grown about
    /// its centre to a finger across, kept on screen.
    fn target(&self, r: &Rect) -> Rect {
        let grow = |at: f32, len: f32, screen: f32| {
            let want = len.max(self.finger).min(screen);
            let at = (at - (want - len) / 2.0).clamp(0.0, (screen - want).max(0.0));
            (at, want)
        };
        let (x, w) = grow(r[0], r[2], self.screen[0]);
        let (y, h) = grow(r[1], r[3], self.screen[1]);
        [x, y, w, h]
    }

    /// The control a tap at `p` is for: the topmost drawn under it, or
    /// failing a button there, the nearest whose finger-sized target holds
    /// it, among those drawn over whatever panel is under it.
    fn hit(&self, p: [f32; 2]) -> Option<&Hit> {
        let under = self.hits.iter().rposition(|(r, _)| inside(r, p));
        if let Some(at) = under.filter(|&at| self.hits[at].1 != Hit::Block) {
            return Some(&self.hits[at].1);
        }
        let above = under.map_or(0, |at| at + 1);
        let dist = |r: &Rect| {
            let dx = (r[0] - p[0]).max(p[0] - r[0] - r[2]).max(0.0);
            let dy = (r[1] - p[1]).max(p[1] - r[1] - r[3]).max(0.0);
            dx * dx + dy * dy
        };
        self.hits[above..]
            .iter()
            .filter(|(r, h)| *h != Hit::Block && inside(&self.target(r), p))
            .min_by(|a, b| dist(&a.0).total_cmp(&dist(&b.0)))
            .map(|(_, h)| h)
            .or(under.map(|at| &self.hits[at].1))
    }

    /// The screen rectangles (x, y, width, height) its controls and cards
    /// took last frame, so the map can keep its own labels out from under them.
    pub fn panels(&self) -> Vec<[f32; 4]> {
        self.hits
            .iter()
            .filter(|(_, h)| *h != Hit::Block)
            .map(|(r, _)| *r)
            .collect()
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
                    None if id != MOUSE_PAN => Some(Tap {
                        id,
                        start: [x, y],
                        at: Instant::now(),
                    }),
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
                self.page = 0;
                self.research = false;
                self.problems = false;
                self.selected = None;
                Vec::new()
            }
            // Tapping the tool in hand again puts it away.
            Hit::Tool(i) if i != 0 && self.current == Some(i) => {
                self.more = false;
                vec![Action::Tool(Tool::Hand)]
            }
            Hit::Tool(i) => {
                if i != 0 {
                    self.selected = None;
                }
                self.more = false;
                self.library = false;
                vec![Action::Tool(tool_at(i))]
            }
            Hit::Done => vec![Action::Tool(Tool::Hand)],
            Hit::Undo => vec![Action::Undo],
            Hit::Redo => vec![Action::Redo],
            Hit::Build(b) => {
                self.more = false;
                self.radial = None;
                self.selected = None;
                vec![Action::Tool(Tool::Place(b))]
            }
            Hit::Rotate => vec![Action::Rotate],
            Hit::Recipe(r) => vec![Action::Recipe(r)],
            Hit::Belt(b) => vec![Action::Belt(b)],
            Hit::SetRequest(entity, request) => {
                vec![Action::Send(Command::SetRequest { entity, request })]
            }
            Hit::SetFilter(entity, filter) => {
                vec![Action::Send(Command::SetFilter { entity, filter })]
            }
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
                self.page = 0;
                self.more = false;
                self.problems = false;
                self.selected = None;
                Vec::new()
            }
            Hit::Study(research) => vec![Action::Send(Command::SetResearch { research })],
            Hit::Unlock => {
                self.unlock = true;
                self.research = false;
                Vec::new()
            }
            Hit::Buy => vec![Action::Buy],
            Hit::CopyCrash => vec![Action::CopyCrash],
            Hit::Restore => vec![Action::Restore],
            Hit::Item(item) => {
                self.browse = Some(item);
                self.selected = None;
                self.research = false;
                self.problems = false;
                self.more = false;
                Vec::new()
            }
            Hit::Rate(up) => {
                let at = self.rate();
                self.rate = Some(if up {
                    (at + 1).min(plan::RATES.len() - 1)
                } else {
                    at.saturating_sub(1)
                });
                Vec::new()
            }
            Hit::Close if self.from_title => {
                self.open_title();
                Vec::new()
            }
            Hit::Close => {
                self.library = false;
                self.selected = None;
                self.research = false;
                self.unlock = false;
                self.problems = false;
                self.more = false;
                self.radial = None;
                self.browse = None;
                Vec::new()
            }
            Hit::Page(on) => {
                // Drawing the sheet keeps the page in range.
                self.page = if on {
                    self.page + 1
                } else {
                    self.page.saturating_sub(1)
                };
                Vec::new()
            }
            Hit::StockMore => {
                self.stock_open = !self.stock_open;
                Vec::new()
            }
            Hit::Goal => {
                self.hint = !self.hint;
                Vec::new()
            }
            Hit::Sound => {
                self.muted = !self.muted;
                vec![Action::Mute(self.muted)]
            }
            Hit::TitleContinue => {
                self.close_title();
                Vec::new()
            }
            Hit::TitleSettings => {
                self.title = false;
                self.from_title = true;
                self.settings = true;
                Vec::new()
            }
            Hit::TitleNew | Hit::TitleLoad => {
                self.title = false;
                self.from_title = true;
                self.menu = true;
                self.confirm_delete = None;
                self.new_game = None;
                // A new game goes straight to its form in the first empty
                // slot; with none empty, the slots show, to make room.
                if hit == Hit::TitleNew {
                    if let Some(n) = self.empty_slot.filter(|n| *n < SLOTS) {
                        self.rolls += 1;
                        self.new_game = Some(NewGame::new(n, roll(self.rolls).to_string()));
                    }
                }
                vec![Action::MenuOpened]
            }
            Hit::TitleBuy => {
                self.title = false;
                self.from_title = true;
                self.unlock = true;
                Vec::new()
            }
            Hit::Menu if self.menu && self.from_title => {
                // Closing a menu opened from the start menu goes back to it.
                self.open_title();
                Vec::new()
            }
            Hit::Menu if self.settings => {
                // The menu button on the settings sheet closes it.
                self.settings = false;
                self.settings_from_menu = false;
                Vec::new()
            }
            Hit::Menu => {
                self.menu = !self.menu;
                self.confirm_delete = None;
                self.new_game = None;
                if self.menu {
                    self.selected = None;
                    self.research = false;
                    self.unlock = false;
                    self.problems = false;
                    self.more = false;
                    self.browse = None;
                    return vec![Action::MenuOpened];
                }
                Vec::new()
            }
            Hit::SlotLoad(n) => {
                self.menu = false;
                self.from_title = false;
                vec![Action::LoadSlot(n)]
            }
            Hit::SlotDelete(n) => {
                if self.confirm_delete == Some(n) {
                    self.confirm_delete = None;
                    vec![Action::DeleteSlot(n)]
                } else {
                    self.confirm_delete = Some(n);
                    Vec::new()
                }
            }
            Hit::SlotNew(n) => {
                self.confirm_delete = None;
                self.rolls += 1;
                self.new_game = Some(NewGame::new(n, roll(self.rolls).to_string()));
                Vec::new()
            }
            Hit::Digit(d) => {
                if let Some(g) = &mut self.new_game {
                    if g.seed == "0" {
                        g.seed.clear();
                    }
                    if g.seed.len() < SEED_DIGITS {
                        g.seed.push(char::from(b'0' + d.min(9)));
                    }
                }
                Vec::new()
            }
            Hit::Backspace => {
                if let Some(g) = &mut self.new_game {
                    g.seed.pop();
                }
                Vec::new()
            }
            Hit::Dice => {
                self.rolls += 1;
                let seed = roll(self.rolls).to_string();
                if let Some(g) = &mut self.new_game {
                    g.seed = seed;
                }
                Vec::new()
            }
            Hit::Preset(p) => {
                if let Some(g) = &mut self.new_game {
                    g.preset = p;
                }
                Vec::new()
            }
            Hit::RuleOre => {
                if let Some(g) = &mut self.new_game {
                    g.ore_runs_out = !g.ore_runs_out;
                }
                Vec::new()
            }
            Hit::RuleOrders => {
                if let Some(g) = &mut self.new_game {
                    g.orders = (g.orders + 1) % PACE_NAMES.len() as u8;
                }
                Vec::new()
            }
            Hit::Settings => {
                self.menu = false;
                self.confirm_delete = None;
                self.new_game = None;
                self.settings = true;
                self.settings_from_menu = true;
                Vec::new()
            }
            Hit::SettingsClose => {
                self.settings = false;
                if self.from_title {
                    self.open_title();
                } else if self.settings_from_menu {
                    self.menu = true;
                    self.settings_from_menu = false;
                    return vec![Action::MenuOpened];
                }
                Vec::new()
            }
            Hit::Volume => {
                let at = VOLUMES
                    .iter()
                    .position(|v| *v == self.volume())
                    .unwrap_or(0);
                self.quieter = ((at + 1) % VOLUMES.len()) as u8;
                vec![Action::Volume(self.volume())]
            }
            Hit::Ambience => {
                self.ambience_off = !self.ambience_off;
                vec![Action::Ambience(!self.ambience_off)]
            }
            Hit::Vibration => {
                self.vibration_off = !self.vibration_off;
                vec![Action::Vibration(!self.vibration_off)]
            }
            Hit::CancelNew => {
                self.new_game = None;
                Vec::new()
            }
            Hit::StartNew => {
                let Some(g) = self.new_game.take() else {
                    return Vec::new();
                };
                self.menu = false;
                self.from_title = false;
                vec![Action::NewGame {
                    slot: g.slot,
                    seed: g.seed.parse().unwrap_or(0),
                    preset: g.preset,
                    ore_runs_out: g.ore_runs_out,
                    orders: g.orders,
                }]
            }
            Hit::ColourBlind => {
                self.colour_blind = !self.colour_blind;
                self.toast(tr(if self.colour_blind {
                    "toast.colour_blind_on"
                } else {
                    "toast.colour_blind_off"
                }));
                vec![Action::ColourBlind(self.colour_blind)]
            }
            Hit::LargeText => {
                self.large_text = !self.large_text;
                self.toast(tr(if self.large_text {
                    "toast.large_text_on"
                } else {
                    "toast.large_text_off"
                }));
                vec![Action::LargeText(self.large_text)]
            }
            Hit::Motion => {
                self.reduce_motion = !self.reduce_motion;
                self.toast(tr(if self.reduce_motion {
                    "toast.reduce_motion_on"
                } else {
                    "toast.reduce_motion_off"
                }));
                vec![Action::ReduceMotion(self.reduce_motion)]
            }
            Hit::Language => {
                self.set_language(self.language.next());
                vec![Action::Language(self.language)]
            }
            Hit::Power => {
                self.low_power = !self.low_power;
                self.toast(tr(if self.low_power {
                    "toast.low_power_on"
                } else {
                    "toast.low_power_off"
                }));
                vec![Action::LowPower(self.low_power)]
            }
            Hit::Map => {
                self.map = !self.map;
                self.selected = None;
                vec![Action::Map(self.map)]
            }
            Hit::Problems => {
                self.problems = !self.problems;
                self.research = false;
                self.more = false;
                self.selected = None;
                Vec::new()
            }
            Hit::Problem(i) => {
                let Some(p) = self.found.get(i) else {
                    return Vec::new();
                };
                // Another tap on the same line goes on to the next one.
                let k = match &self.jumped {
                    Some((t, b, k)) if *t == p.trouble && *b == p.building => (k + 1) % p.at.len(),
                    _ => 0,
                };
                self.jumped = Some((p.trouble, p.building.clone(), k));
                let (entity, at) = p.at[k];
                self.problems = false;
                self.selected = Some(entity);
                vec![Action::Focus(at)]
            }
            Hit::Schedule(entity, stops) => {
                vec![Action::Send(Command::SetSchedule { entity, stops })]
            }
            Hit::PickStop(train) => {
                self.picking = match self.picking {
                    Some(t) if t == train => None,
                    _ => Some(train),
                };
                Vec::new()
            }
            Hit::Copy => vec![Action::Copy],
            Hit::Cut => vec![Action::Cut],
            Hit::Erase => vec![Action::DeleteSelection],
            Hit::Keep => vec![Action::SaveBlueprint],
            Hit::Place => vec![Action::PlacePaste],
            Hit::Library => {
                self.library = !self.library;
                self.page = 0;
                self.confirm_forget = None;
                Vec::new()
            }
            Hit::Pick(i) => {
                self.library = false;
                vec![Action::Paste(i)]
            }
            Hit::Forget(i) => {
                if self.confirm_forget == Some(i) {
                    self.confirm_forget = None;
                    vec![Action::ForgetBlueprint(i)]
                } else {
                    self.confirm_forget = Some(i);
                    Vec::new()
                }
            }
            Hit::Block => Vec::new(),
        }
    }

    /// Lay out and paint the HUD for this frame.
    pub fn draw(&mut self, view: &View, o: &mut Overlay) {
        self.hits.clear();
        o.prefer_chinese(i18n::lang() == i18n::Lang::ZhHans);
        let (w, h) = (view.size.0 as f32, view.size.1 as f32);
        let portrait = h > w;
        // Points to pixels: the display's own, shrunk a little on the
        // narrowest phones so the toolbar's labels still fit, and grown on
        // a tablet, held further away. Tap targets stay a finger wide
        // whatever this is (see `Hud::target`).
        let scale = view.scale.max(0.5);
        let across = if portrait { w } else { h } / scale;
        let n = TOOLS.len() as f32;
        let u = scale * (across / 380.0).clamp(0.85, 1.0) * (across / 700.0).clamp(1.0, 1.25);
        // Large text grows everything up to a fifth, as far as the HUD's
        // layout still fits across the screen: 350 points upright, 376 on
        // its side, where the top bar's buttons crowd each other. A small
        // phone gains a little, a big phone or a tablet the whole fifth.
        let u = if self.large_text {
            (u * LARGE_TEXT).min(u.max(across * scale / if portrait { 350.0 } else { 376.0 }))
        } else {
            u
        };
        self.unit = u;
        self.finger = FINGER * scale;
        self.screen = [w, h];
        let m = 12.0 * u;
        // Margins clear of notches, rounded corners and the home indicator.
        let [st, sr, sb, sl] = view.safe;
        let (mt, mr, mb, ml) = (m + st, m + sr, m + sb, m + sl);
        let gap = 10.0 * u;
        self.empty_slot = view
            .slots
            .iter()
            .position(|s| !s.used)
            .and_then(|n| u8::try_from(n).ok());
        if self.title {
            self.title_screen(view, o, [w, h], [mt, mr, mb, ml]);
            return;
        }
        let selected = self
            .selected
            .and_then(|id| view.snapshot.entities.iter().find(|e| e.id == id));
        let train = self
            .selected
            .and_then(|id| view.snapshot.trains.iter().find(|t| t.id == id));
        if selected.is_none() && train.is_none() {
            self.selected = None;
        }
        if self.picking != self.selected {
            self.picking = None;
        }

        // The toolbar, and the space left beside it for everything else.
        let bar = if portrait {
            // On a tablet the bar stops growing and sits in the middle.
            let bh = 76.0 * u;
            let bw = (w - ml - mr).min(n * 96.0 * u);
            [ml + (w - ml - mr - bw) / 2.0, h - mb - bh, bw, bh]
        } else {
            let bw = 92.0 * u;
            let bh = (h - mt - mb).min(n * 80.0 * u);
            [w - mr - bw, h - mb - bh, bw, bh]
        };
        self.current = tool_index(view.tool);
        self.radial_ok = matches!(view.tool, Tool::Hand | Tool::Place(_));
        self.toolbar(view, o, bar, portrait);
        // The bottom-right corner of the free area, where the thumb is.
        let corner = if portrait {
            [w - mr, bar[1] - gap]
        } else {
            [bar[0] - gap, h - mb]
        };

        // The top of the screen stays small on a phone: the stock, one row
        // of status chips (research, problems), the goal card, then a tip.
        // The sound button keeps the top-right corner, clear of the thumb.
        let right = corner[0] - 36.0 * u;
        self.sound_button(o, [right, mt]);
        // The map and menu buttons sit beside it.
        let right = right - gap - 36.0 * u;
        self.menu_button(o, [right, mt]);
        let right = right - gap - 36.0 * u;
        self.map_button(o, [right, mt]);
        let right = right - gap;
        // The stock keeps to one row beside the buttons, with a "+N" pill for the rest.
        let top = self.stock(view, o, [ml, mt], right);
        // Looking for problems walks the whole factory: on a big one, do
        // it four times a second, or at once when a machine's state or the
        // number of things built changes (T65).
        let snap = view.snapshot;
        let key = (
            snap.tick / 15,
            snap.entities.len(),
            snap.entities
                .iter()
                .map(|e| e.state as u64 + 1)
                .fold(0u64, |h, s| h.wrapping_mul(31).wrapping_add(s)),
            snap.grids.len(),
            // Its words are in the language they were found in.
            i18n::lang(),
        );
        if self.found_for != Some(key) {
            self.found_for = Some(key);
            self.found = problems::find(view);
        }
        if self.found.is_empty() {
            self.problems = false;
        }
        // The chips share the stock's row while there is room beside it.
        let stock_end = self
            .hits
            .iter()
            .filter(|(_, h)| matches!(h, Hit::Item(_) | Hit::StockMore))
            .map(|(r, _)| r[0] + r[2])
            .fold(ml, f32::max);
        let beside = stock_end + 8.0 * u;
        let room = right - beside;
        let (row, start, end) = if room >= 220.0 * u {
            (mt, beside, right)
        } else {
            (top.max(mt + 36.0 * u) + gap, ml, corner[0])
        };
        let x = self.research_button(view, o, [start, row], end - start);
        let x = self.problems_button(o, [x, row], end - x);
        let top = if x > start {
            top.max(row + CHIP_H * u)
        } else {
            top
        };
        // Cards and tips read as a column, not a banner, on a wide screen.
        let column = (corner[0] - ml).min(560.0 * u);
        // The map and the game menu want the whole screen: the goal waits
        // until they close. So does the recipe browser on a screen held
        // sideways, where it needs the height for its calculator.
        let top = if self.map || self.menu || self.settings || (self.browse.is_some() && !portrait)
        {
            top
        } else {
            self.goal_card(view, o, [ml, top + gap], column)
        };
        let mut tip_y = top + gap;
        if let Some((text, at, ink)) = &self.toast {
            if at.elapsed() < TOAST_FOR {
                let (text, ink) = (text.clone(), *ink);
                let r = chip(o, &text, [ml, tip_y], column, u, ink);
                // Cards take their taps, so nothing is built unseen under them.
                self.hits.push((r, Hit::Block));
                tip_y = r[1] + r[3] + gap;
            } else {
                self.toast = None;
            }
        }
        // With the hand and a goal on screen, the goal card already says
        // what to do next; the general tip would only push the map down.
        let hand_with_goal = *view.tool == Tool::Hand && view.snapshot.goal.is_some();
        if selected.is_none()
            && train.is_none()
            && !self.research
            && !self.unlock
            && !self.more
            && !self.problems
            && !self.library
            && self.browse.is_none()
            && !hand_with_goal
            && !self.map
            && !self.menu
            && !self.settings
        {
            self.tip(view, o, [ml, tip_y], column);
        }
        let width = if portrait {
            (w - ml - mr).min(560.0 * u)
        } else {
            (360.0 * u).min(corner[0] - ml)
        };

        if let Some(at) = self.radial {
            self.radial_menu(view, o, at, [w, h]);
            return;
        }
        if let Some(item) = self.browse.clone() {
            let room = corner[1] - tip_y;
            self.browser(view, o, &item, [corner[0] - width, corner[1]], width, room);
            return;
        }
        if self.more {
            let room = corner[1] - tip_y;
            self.build_sheet(view, o, [corner[0] - width, corner[1]], width, room);
            return;
        }
        if self.unlock {
            if view.store.demo.is_none() {
                // Bought: the sheet has done its job.
                self.unlock = false;
            } else {
                let room = corner[1] - tip_y;
                self.unlock_sheet(view, o, [corner[0] - width, corner[1]], width, room);
                return;
            }
        }
        if self.research {
            let room = corner[1] - tip_y;
            self.research_sheet(view, o, [corner[0] - width, corner[1]], width, room);
            return;
        }
        if self.settings {
            let room = corner[1] - tip_y;
            self.settings_sheet(view, o, corner, width, corner[0] - ml, room);
            return;
        }
        if self.menu {
            let room = corner[1] - tip_y;
            self.menu_sheet(view, o, corner, width, corner[0] - ml, room);
            return;
        }
        if self.problems {
            let room = corner[1] - tip_y;
            self.problems_sheet(view, o, [corner[0] - width, corner[1]], width, room);
            return;
        }
        if !matches!(view.tool, Tool::Select | Tool::Paste) {
            self.library = false;
        }
        if self.library {
            let room = corner[1] - tip_y;
            self.library_sheet(view, o, [corner[0] - width, corner[1]], width, room);
            return;
        }

        if let Some(e) = selected {
            // A machine with a long list of recipes takes a wider sheet
            // held sideways, so its picker stays on a short screen.
            let many = view.recipes_for(&e.building).is_some_and(|r| r.len() > 24);
            let width = if many && !portrait {
                width.max((520.0 * u).min(corner[0] - ml))
            } else {
                width
            };
            self.sheet(view, o, e, [corner[0] - width, corner[1]], width);
            return;
        }
        if let Some(t) = train {
            self.train_sheet(view, o, t, [corner[0] - width, corner[1]], width);
            return;
        }

        // The rotate button and recipe pills for the tool in hand.
        let placing = match view.tool {
            Tool::Place(b) => Some(b),
            _ => None,
        };
        let mut right = corner[0];
        let left = self.undo_buttons(view, o, [ml, corner[1]]);
        let pasting = *view.tool == Tool::Paste;
        if placing.is_some() || *view.tool == Tool::Belt || pasting {
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
            self.recipe_picker(
                o,
                view,
                &recipes,
                view.recipe,
                Hit::Recipe,
                [left, corner[1]],
                right - left,
                true,
            );
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
                self.pills(o, tiers, [left, corner[1]], right - left, true);
            }
        }
        // The selection's buttons, and the library's (T51).
        if *view.tool == Tool::Select || pasting {
            let mut row: Vec<Pill> = Vec::new();
            if pasting {
                row.push((tr("button.place"), None, true, Hit::Place));
            } else if view.selection > 0 {
                row.push((tr("button.copy"), None, false, Hit::Copy));
                row.push((tr("button.cut"), None, false, Hit::Cut));
                row.push((tr("button.delete"), None, false, Hit::Erase));
                row.push((tr("button.save"), None, false, Hit::Keep));
            }
            row.push((tr("button.blueprints"), None, false, Hit::Library));
            self.pills(o, row, [left, corner[1]], right - left, true);
        }
    }

    /// The blueprint library, bottom-left at `at`, at most `room` tall:
    /// a row per blueprint with a sketch of it. A tap takes one in hand.
    fn library_sheet(&mut self, view: &View, o: &mut Overlay, at: [f32; 2], width: f32, room: f32) {
        let u = self.unit;
        let pad = 16.0 * u;
        let gap = 8.0 * u;
        let title = 30.0 * u;
        let row = 64.0 * u;
        let list = view.blueprints;
        let fits = ((room - 2.0 * pad - title + gap) / (row + gap))
            .floor()
            .max(1.0) as usize;
        let rows = list.len().clamp(1, fits);
        // What doesn't fit goes on further pages.
        let pages = list.len().div_ceil(rows).max(1);
        self.page = self.page.min(pages - 1);
        let height = pad + title + rows as f32 * (row + gap) - gap + pad;
        let r = [at[0], at[1] - height, width, height];
        o.panel([r[0], r[1]], [r[2], r[3]], 14.0 * u, PANEL);
        self.hits.push((r, Hit::Block));
        let (left, top) = (r[0] + pad, r[1] + pad);
        let px = 20.0 * u;
        o.text(
            tr("blueprint.title"),
            [left, top + o.cap_height(px) + 4.0 * u],
            px,
            TEXT,
        );
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
        self.pager(o, c, pages);
        let inner = width - 2.0 * pad;
        if list.is_empty() {
            let lines = wrap(o, tr("blueprint.empty"), 14.0 * u, inner);
            for (k, l) in lines.iter().enumerate().take(3) {
                let y = top + title + 20.0 * u + k as f32 * 20.0 * u;
                o.text(l, [left, y], 14.0 * u, MUTED);
            }
            return;
        }
        let first = self.page * rows;
        for (i, bp) in list.iter().enumerate().skip(first).take(rows) {
            let y = top + title + (i - first) as f32 * (row + gap);
            let b = [left, y, inner, row];
            o.panel([b[0], b[1]], [b[2], b[3]], 10.0 * u, RAISED);
            self.hits.push((b, Hit::Pick(i)));
            // A sketch: each building's icon where it stands.
            let sk = row - 12.0 * u;
            let (sx, sy) = (b[0] + 6.0 * u, b[1] + 6.0 * u);
            o.panel([sx, sy], [sk, sk], 6.0 * u, PANEL);
            let span = bp.size.0.max(bp.size.1).max(1) as f32;
            let tile = (sk - 4.0 * u) / span;
            let (ox, oy) = (
                sx + (sk - tile * bp.size.0 as f32) / 2.0,
                sy + (sk - tile * bp.size.1 as f32) / 2.0,
            );
            for p in &bp.pieces {
                let at = [ox + p.at.0 as f32 * tile, oy + p.at.1 as f32 * tile];
                let s = tile * f32::from(p.size);
                match view.look.building(&p.building) {
                    Some((sprite, t)) if s >= 6.0 => o.icon(sprite, at, s, t),
                    Some((_, t)) => o.panel(at, [s, s], 0.0, t),
                    None => o.panel(at, [s, s], 0.0, MUTED),
                }
            }
            let tx = sx + sk + 10.0 * u;
            let tw = b[0] + b[2] - tx - 48.0 * u;
            let npx = fit(o, &bp.name, 15.0 * u, tw);
            o.text(&bp.name, [tx, b[1] + 26.0 * u], npx, TEXT);
            let detail = trf(
                "blueprint.detail",
                &[&bp.size.0, &bp.size.1, &bp.pieces.len()],
            );
            let dpx = fit(o, &detail, 12.0 * u, tw);
            o.text(&detail, [tx, b[1] + 46.0 * u], dpx, MUTED);
            // Delete, with a second tap to be sure.
            let d = 36.0 * u;
            let x = [b[0] + b[2] - d - 8.0 * u, b[1] + (row - d) / 2.0, d, d];
            let asked = self.confirm_forget == Some(i);
            let fill = if asked { palette::STARVE } else { PANEL };
            o.panel([x[0], x[1]], [d, d], d / 2.0, fill);
            let ink = if asked { TEXT } else { palette::STARVE };
            let xw = o.measure("×", 20.0 * u);
            o.text("×", [x[0] + (d - xw) / 2.0, x[1] + d * 0.72], 20.0 * u, ink);
            self.hits.push((x, Hit::Forget(i)));
        }
    }

    /// The tip for the tool in hand. With a tool other than Move it names
    /// the tool and carries a Done button, so it is always clear what a tap
    /// will do and how to stop.
    fn tip(&mut self, view: &View, o: &mut Overlay, at: [f32; 2], width: f32) {
        let u = self.unit;
        let Some(slot) = self.current.filter(|_| *view.tool != Tool::Hand) else {
            let r = chip(o, tip(view.tool), at, width, u, TEXT);
            self.hits.push((r, Hit::Block));
            return;
        };
        let name = match view.tool {
            Tool::Place(b) => view
                .building(b)
                .map_or(tr("tip.build"), |d| d.name.as_str()),
            _ => tr(TOOLS[slot].0),
        };
        let what = match view.tool {
            Tool::Select if view.selection > 0 => trf("tip.selected", &[&view.selection]),
            _ => tip(view.tool).to_string(),
        };
        let text = trf("tip.named", &[&name, &what]);
        let ink = if *view.tool == Tool::Remove {
            palette::STARVE
        } else {
            TEXT
        };
        let px = 15.0 * u;
        let done = tr("button.done");
        let bw = o.measure(done, px) + 28.0 * u;
        let gap = 8.0 * u;
        let card = chip(o, &text, at, width - bw - gap, u, ink);
        self.hits.push((card, Hit::Block));
        let bottom = card[1] + card[3];
        let h = (bottom - at[1]).max(40.0 * u).min(52.0 * u);
        let r = [at[0] + width - bw, at[1], bw, h];
        o.panel([r[0], r[1]], [r[2], r[3]], 8.0 * u, RAISED);
        o.text(
            done,
            [r[0] + 14.0 * u, r[1] + (h + o.cap_height(px)) / 2.0],
            px,
            TEXT,
        );
        self.hits.push((r, Hit::Done));
    }

    /// Undo and redo buttons in the bottom-left corner of the free area, when
    /// there is something to take back. Returns where the next control may start.
    fn undo_buttons(&mut self, view: &View, o: &mut Overlay, at: [f32; 2]) -> f32 {
        let (undo, redo) = view.history;
        let u = self.unit;
        let h = 48.0 * u;
        let px = 15.0 * u;
        let mut x = at[0];
        for (show, live, hit, label) in [
            (undo || redo, undo, Hit::Undo, tr("button.undo")),
            (redo, redo, Hit::Redo, tr("button.redo")),
        ] {
            if !show {
                continue;
            }
            let w = o.measure(label, px) + 28.0 * u;
            let r = [x, at[1] - h, w, h];
            o.panel([r[0], r[1]], [w, h], h / 2.0, PANEL);
            let ink = if live {
                TEXT
            } else {
                [TEXT[0], TEXT[1], TEXT[2], 0x50]
            };
            o.text(
                label,
                [r[0] + 14.0 * u, r[1] + (h + o.cap_height(px)) / 2.0],
                px,
                ink,
            );
            self.hits.push((r, if live { hit } else { Hit::Block }));
            x += w + 8.0 * u;
        }
        x
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
                    (def.map_or(tr(label), |d| d.name.as_str()), def)
                }
                _ => (tr(label), building.and_then(|b| view.building(&b.into()))),
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
            // The coach's next tool glows until it is picked.
            // Held at half glow when the player asked for less motion.
            if current != Some(i) && view.nudge.as_ref().and_then(tool_index) == Some(i) {
                let t = pulse_clock().elapsed().as_secs_f32();
                let beat = if self.reduce_motion {
                    0.5
                } else {
                    (t * 4.0).sin() * 0.5 + 0.5
                };
                let a = 64.0 + beat * 80.0;
                let [cr, cg, cb, _] = palette::ACCENT;
                o.panel([r[0], r[1]], [r[2], r[3]], 10.0 * u, [cr, cg, cb, a as u8]);
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
    fn stock(&mut self, view: &View, o: &mut Overlay, at: [f32; 2], right: f32) -> f32 {
        let u = self.unit;
        let h = 30.0 * u;
        let px = 14.0 * u;
        let s = 18.0 * u;
        let items: Vec<_> = view.snapshot.stock.iter().filter(|(_, n)| *n > 0).collect();
        let width =
            |o: &Overlay, text: &str| 8.0 * u + s + 5.0 * u + o.measure(text, px) + 10.0 * u;
        // One row, with a "+N" pill for the rest; open, every item on as
        // many rows as it takes.
        let more_w = |o: &Overlay, n: usize| width(o, &format!("+{n}")) - s - 5.0 * u;
        let mut shown = items.len();
        if !self.stock_open {
            let mut x = at[0];
            for (k, (_, count)) in items.iter().enumerate() {
                let w = width(o, &count.to_string());
                let rest = items.len() - k - 1;
                let need = if rest > 0 {
                    more_w(o, rest) + 6.0 * u
                } else {
                    0.0
                };
                if k > 0 && x + w + need > right {
                    shown = k;
                    break;
                }
                x += w + 6.0 * u;
            }
        }
        let mut x = at[0];
        let mut y = at[1];
        for (item, count) in &items[..shown] {
            let text = count.to_string();
            let w = width(o, &text);
            if x > at[0] && x + w > right {
                x = at[0];
                y += h + 6.0 * u;
            }
            o.panel([x, y], [w, h], 8.0 * u, PANEL);
            if let Some((sprite, t)) = view.look.item(item) {
                o.icon(sprite, [x + 8.0 * u, y + (h - s) / 2.0], s, t);
            }
            let base = y + (h + o.cap_height(px)) / 2.0;
            o.text(&text, [x + 8.0 * u + s + 5.0 * u, base], px, TEXT);
            self.hits.push(([x, y, w, h], Hit::Item(item.clone())));
            x += w + 6.0 * u;
        }
        let hidden = items.len() - shown;
        if hidden > 0 || self.stock_open && items.len() > 1 {
            let text = if self.stock_open {
                tr("stock.less").to_string()
            } else {
                format!("+{hidden}")
            };
            let w = more_w(o, 0).max(o.measure(&text, px) + 18.0 * u);
            if x > at[0] && x + w > right {
                x = at[0];
                y += h + 6.0 * u;
            }
            o.panel([x, y], [w, h], 8.0 * u, RAISED);
            let base = y + (h + o.cap_height(px)) / 2.0;
            o.text(
                &text,
                [x + (w - o.measure(&text, px)) / 2.0, base],
                px,
                MUTED,
            );
            // A finger-sized target, taller than the pill.
            self.hits.push((
                [x, y - 7.0 * u, w.max(44.0 * u), h + 14.0 * u],
                Hit::StockMore,
            ));
        }
        y + h
    }

    /// A round button that opens the game menu: three bars.
    fn menu_button(&mut self, o: &mut Overlay, at: [f32; 2]) {
        let u = self.unit;
        let d = 36.0 * u;
        o.panel(
            at,
            [d, d],
            d / 2.0,
            if self.menu { PRESSED } else { RAISED },
        );
        let (cx, cy) = (at[0] + d / 2.0, at[1] + d / 2.0);
        for k in [-1.0, 0.0, 1.0] {
            o.panel(
                [cx - 8.0 * u, cy + k * 5.5 * u - 1.5 * u],
                [16.0 * u, 3.0 * u],
                1.5 * u,
                TEXT,
            );
        }
        let pad = 4.0 * u;
        self.hits.push((
            [at[0] - pad, at[1] - pad, d + 2.0 * pad, d + 2.0 * pad],
            Hit::Menu,
        ));
    }

    /// A rounded button with a centred label.
    #[allow(clippy::too_many_arguments)]
    fn button(
        &mut self,
        o: &mut Overlay,
        r: Rect,
        label: &str,
        px: f32,
        fill: Rgba,
        ink: Rgba,
        hit: Hit,
    ) {
        o.panel([r[0], r[1]], [r[2], r[3]], r[3].min(r[2]) * 0.3, fill);
        let px = fit(o, label, px, r[2] - 8.0 * self.unit);
        let w = o.measure(label, px);
        o.text(
            label,
            [
                r[0] + (r[2] - w) / 2.0,
                r[1] + (r[3] + o.cap_height(px)) / 2.0,
            ],
            px,
            ink,
        );
        self.hits.push((r, hit));
    }

    /// The game menu (T63, S4): the save slots, a new game with its seed,
    /// preset and rules, and a button for the settings. Bottom-right at `corner`, `width`
    /// wide, or up to `wide` when the new-game form needs two columns to
    /// fit a short screen.
    #[allow(clippy::too_many_arguments)]
    fn menu_sheet(
        &mut self,
        view: &View,
        o: &mut Overlay,
        corner: [f32; 2],
        width: f32,
        wide: f32,
        room: f32,
    ) {
        let u = self.unit;
        let pad = 16.0 * u;
        let bh = 44.0 * u;
        let gap = 8.0 * u;
        let new_game = self.new_game.clone();
        // The form in one column: a note, the seed, three rows of keys,
        // the presets, the rules, then cancel and start.
        let note = 24.0 * u;
        let tall = 50.0 * u + note + 4.0 * (bh + gap) + 3.0 * (bh + gap) - gap;
        // On a short screen it takes two columns and drops its heading:
        // the keypad on the left, the rest on the right.
        let two = new_game.is_some() && 2.0 * pad + tall > room && wide > width;
        let title = if two { 0.0 } else { 50.0 * u };
        let width = if two { wide.min(620.0 * u) } else { width };
        // Slot rows shrink on a short screen, down to a button and a gap.
        // Under the slots, the settings button, and a crash report waiting
        // adds a row for sending it.
        let extra = if view.crash { bh + gap } else { 0.0 };
        let settings = bh + extra;
        let row =
            ((room - 2.0 * pad - title - settings) / f32::from(SLOTS)).clamp(bh + gap, 64.0 * u);
        // The keys grow into spare room on a tall screen.
        let key = ((room - 2.0 * pad - tall) / 3.0 + bh).clamp(bh, 52.0 * u);
        let body = match &new_game {
            Some(_) if two => 5.0 * bh + 4.0 * gap,
            Some(_) => note + 4.0 * (bh + gap) + 3.0 * (key + gap) - gap,
            None => f32::from(SLOTS) * row + settings,
        };
        let height = pad + title + body + pad;
        let r = [corner[0] - width, corner[1] - height, width, height];
        o.panel([r[0], r[1]], [r[2], r[3]], 14.0 * u, PANEL);
        self.hits.push((r, Hit::Block));
        let left = r[0] + pad;
        let inner = r[2] - 2.0 * pad;
        let mut y = r[1] + pad;
        if !two {
            let px = 20.0 * u;
            let heading = match &new_game {
                Some(g) => trf("menu.new_game_in_slot", &[&(g.slot + 1)]),
                None => tr("menu.title").to_string(),
            };
            let close = 44.0 * u;
            o.text(
                &heading,
                [left, y + (close + o.cap_height(px)) / 2.0],
                px,
                TEXT,
            );
            let c = [r[0] + r[2] - pad - close, y, close, close];
            o.panel([c[0], c[1]], [close, close], close / 2.0, RAISED);
            let xw = o.measure("×", 22.0 * u);
            o.text(
                "×",
                [c[0] + (close - xw) / 2.0, c[1] + close * 0.72],
                22.0 * u,
                TEXT,
            );
            // Closing the form goes back to the slots; closing those shuts the menu.
            let back = if new_game.is_some() {
                Hit::CancelNew
            } else {
                Hit::Menu
            };
            self.hits.push((c, back));
            y += title;
        }

        if let Some(g) = new_game {
            // Where each part goes: the seed row, the keypad (columns and
            // rows of keys) and the presets (columns), the rules and the last row.
            let (seed_at, pad_at, cols, presets_at, pcols, rules_at, last_at);
            if two {
                let half = (inner - gap) / 2.0;
                let right = left + half + gap;
                pad_at = [left, y, half];
                cols = 3u8;
                seed_at = [right, y, half];
                presets_at = [right, y + bh + gap, half];
                pcols = 2u8;
                rules_at = [right, y + 3.0 * (bh + gap), half];
                last_at = [right, y + 4.0 * (bh + gap), half];
            } else {
                let text = tr("menu.seed_note");
                let mpx = fit(o, text, 14.0 * u, inner);
                o.text(text, [left, y + o.cap_height(mpx)], mpx, MUTED);
                y += note;
                seed_at = [left, y, inner];
                pad_at = [left, y + bh + gap, inner];
                cols = 4;
                presets_at = [left, y + bh + gap + 3.0 * (key + gap), inner];
                pcols = 4;
                rules_at = [presets_at[0], presets_at[1] + bh + gap, inner];
                last_at = [rules_at[0], rules_at[1] + bh + gap, inner];
            }
            let key = if two { bh } else { key };
            // The seed, and a die for a random one.
            let die = bh;
            let field = [seed_at[0], seed_at[1], seed_at[2] - die - gap, bh];
            o.panel([field[0], field[1]], [field[2], field[3]], 10.0 * u, RAISED);
            let lpx = 14.0 * u;
            o.text(
                tr("menu.seed"),
                [
                    field[0] + 12.0 * u,
                    field[1] + (bh + o.cap_height(lpx)) / 2.0,
                ],
                lpx,
                MUTED,
            );
            let spx = 18.0 * u;
            let shown = if g.seed.is_empty() {
                "0"
            } else {
                g.seed.as_str()
            };
            let spx = fit(o, shown, spx, field[2] - 60.0 * u);
            let sw = o.measure(shown, spx);
            o.text(
                shown,
                [
                    field[0] + field[2] - 12.0 * u - sw,
                    field[1] + (bh + o.cap_height(spx)) / 2.0,
                ],
                spx,
                TEXT,
            );
            self.hits.push((field, Hit::Block));
            self.dice(o, [seed_at[0] + seed_at[2] - die, seed_at[1], die, bh]);
            // The keypad: 1 to 9, then 0 and delete; in three columns the
            // last row reads like a phone's, with 0 in the middle.
            let fc = f32::from(cols);
            let kw = (pad_at[2] - (fc - 1.0) * gap) / fc;
            for k in 0..11u8 {
                let slot = if cols == 3 && k >= 9 { k + 1 } else { k };
                let (cx, cy) = (f32::from(slot % cols), f32::from(slot / cols));
                let kr = [
                    pad_at[0] + cx * (kw + gap),
                    pad_at[1] + cy * (key + gap),
                    kw,
                    key,
                ];
                let (label, hit) = match k {
                    0..=8 => ((k + 1).to_string(), Hit::Digit(k + 1)),
                    9 => ("0".to_string(), Hit::Digit(0)),
                    _ => (tr("key.delete").to_string(), Hit::Backspace),
                };
                let lpx = if k == 10 { 14.0 } else { 20.0 } * u;
                self.button(o, kr, &label, lpx, RAISED, TEXT, hit);
            }
            // The presets.
            let fp = f32::from(pcols);
            let pw = (presets_at[2] - (fp - 1.0) * gap) / fp;
            for (i, name) in PRESET_NAMES.iter().enumerate() {
                let chosen = usize::from(g.preset) == i;
                let (fill, ink) = if chosen {
                    (palette::ACCENT, palette::GROUND)
                } else {
                    (RAISED, TEXT)
                };
                let (cx, cy) = (
                    (i % usize::from(pcols)) as f32,
                    (i / usize::from(pcols)) as f32,
                );
                let pr = [
                    presets_at[0] + cx * (pw + gap),
                    presets_at[1] + cy * (bh + gap),
                    pw,
                    bh,
                ];
                self.button(o, pr, tr(name), 14.0 * u, fill, ink, Hit::Preset(i as u8));
            }
            // The rules: whether ore runs out, and the orders' pace.
            let half = (rules_at[2] - gap) / 2.0;
            let state = |on: bool| tr(if on { "setting.on" } else { "setting.off" });
            let pace = PACE_NAMES[usize::from(g.orders).min(PACE_NAMES.len() - 1)];
            for (i, label, on, hit) in [
                (
                    0.0,
                    trf(
                        "setting.state",
                        &[&tr("rules.ore_runs_out"), &state(g.ore_runs_out)],
                    ),
                    g.ore_runs_out,
                    Hit::RuleOre,
                ),
                (
                    1.0,
                    trf("setting.state", &[&tr("rules.orders"), &tr(pace)]),
                    g.orders != 0,
                    Hit::RuleOrders,
                ),
            ] {
                let (fill, ink) = if on {
                    (PRESSED, view.look.status(palette::Status::Working))
                } else {
                    (RAISED, TEXT)
                };
                let b = [rules_at[0] + i * (half + gap), rules_at[1], half, bh];
                self.button(o, b, &label, 14.0 * u, fill, ink, hit);
            }
            let half = (last_at[2] - gap) / 2.0;
            let [lx, ly, _] = last_at;
            self.button(
                o,
                [lx, ly, half, bh],
                tr("button.cancel"),
                15.0 * u,
                RAISED,
                TEXT,
                Hit::CancelNew,
            );
            self.button(
                o,
                [lx + half + gap, ly, half, bh],
                tr("menu.start"),
                15.0 * u,
                palette::ACCENT,
                palette::GROUND,
                Hit::StartNew,
            );
            return;
        }

        for n in 0..SLOTS {
            let slot = view.slots.get(usize::from(n)).cloned().unwrap_or_default();
            let b = [left, y, inner, row - gap];
            let fill = if slot.current { PRESSED } else { RAISED };
            o.panel([b[0], b[1]], [b[2], b[3]], 10.0 * u, fill);
            self.hits.push((b, Hit::Block));
            // The buttons on the right, then the words in what is left.
            let mut bx = b[0] + b[2] - 6.0 * u;
            let by = b[1] + (b[3] - bh) / 2.0;
            if slot.current {
                let tag = tr("menu.playing");
                let tpx = 13.0 * u;
                bx -= o.measure(tag, tpx) + 8.0 * u;
                o.text(
                    tag,
                    [bx, b[1] + (b[3] + o.cap_height(tpx)) / 2.0],
                    tpx,
                    WORKING,
                );
            } else if slot.used {
                let asking = self.confirm_delete == Some(n);
                let (label, fill, w) = if asking {
                    (tr("menu.delete_ask"), palette::STARVE, 84.0 * u)
                } else {
                    ("×", PANEL, 44.0 * u)
                };
                bx -= w;
                self.button(
                    o,
                    [bx, by, w, bh],
                    label,
                    15.0 * u,
                    fill,
                    TEXT,
                    Hit::SlotDelete(n),
                );
                bx -= 72.0 * u + 6.0 * u;
                self.button(
                    o,
                    [bx, by, 72.0 * u, bh],
                    tr("menu.load"),
                    15.0 * u,
                    palette::ACCENT,
                    palette::GROUND,
                    Hit::SlotLoad(n),
                );
            } else {
                bx -= 96.0 * u;
                self.button(
                    o,
                    [bx, by, 96.0 * u, bh],
                    tr("menu.new_game"),
                    15.0 * u,
                    palette::ACCENT,
                    palette::GROUND,
                    Hit::SlotNew(n),
                );
            }
            let tx = b[0] + 12.0 * u;
            let words = (bx - 8.0 * u - tx).max(20.0 * u);
            let name = if slot.used {
                trf("menu.slot_used", &[&(n + 1), &slot.title])
            } else {
                trf("menu.slot_empty", &[&(n + 1)])
            };
            let npx = fit(o, &name, 15.0 * u, words);
            if slot.used {
                o.text(&name, [tx, b[1] + b[3] * 0.42], npx, TEXT);
                let dpx = fit(o, &slot.detail, 13.0 * u, words);
                o.text(&slot.detail, [tx, b[1] + b[3] * 0.78], dpx, MUTED);
            } else {
                let y = b[1] + (b[3] + o.cap_height(npx)) / 2.0;
                o.text(&name, [tx, y], npx, TEXT);
            }
            y += row;
        }
        // The settings have a sheet of their own.
        self.button(
            o,
            [left, y, inner, bh],
            tr("title.settings"),
            15.0 * u,
            RAISED,
            TEXT,
            Hit::Settings,
        );
        if view.crash {
            self.button(
                o,
                [left, y + bh + gap, inner, bh],
                tr("menu.copy_crash"),
                15.0 * u,
                RAISED,
                TEXT,
                Hit::CopyCrash,
            );
        }
    }

    /// The settings sheet: sound, display, accessibility and general
    /// settings, two to a row under a heading each. Bottom-right at
    /// `corner`, `width` wide. A short screen drops the headings, and one
    /// shorter still, as a small phone held sideways, lays the settings
    /// four to a row, up to `wide` across.
    #[allow(clippy::too_many_arguments)]
    fn settings_sheet(
        &mut self,
        view: &View,
        o: &mut Overlay,
        corner: [f32; 2],
        width: f32,
        wide: f32,
        room: f32,
    ) {
        let u = self.unit;
        let pad = 16.0 * u;
        let bh = 44.0 * u;
        let gap = 8.0 * u;
        let title = 50.0 * u;
        let label = 26.0 * u;
        let groups = 4.0;
        let labelled = groups * (label + bh) + (groups - 1.0) * gap;
        let plain = groups * bh + (groups - 1.0) * gap;
        let (cols, heads) = if 2.0 * pad + title + labelled <= room {
            (2usize, true)
        } else if 2.0 * pad + title + plain <= room || wide <= width {
            (2, false)
        } else {
            (4, false)
        };
        let width = if cols == 4 {
            wide.min(720.0 * u)
        } else {
            width
        };
        let rows = (8 / cols) as f32;
        let body = if heads {
            labelled
        } else {
            rows * bh + (rows - 1.0) * gap
        };
        let height = pad + title + body + pad;
        let r = [corner[0] - width, corner[1] - height, width, height];
        o.panel([r[0], r[1]], [r[2], r[3]], 14.0 * u, PANEL);
        self.hits.push((r, Hit::Block));
        let left = r[0] + pad;
        let inner = r[2] - 2.0 * pad;
        let mut y = r[1] + pad;
        // The heading and its close button.
        let px = 20.0 * u;
        let close = 44.0 * u;
        o.text(
            tr("title.settings"),
            [left, y + (close + o.cap_height(px)) / 2.0],
            px,
            TEXT,
        );
        let c = [r[0] + r[2] - pad - close, y, close, close];
        o.panel([c[0], c[1]], [close, close], close / 2.0, RAISED);
        let xw = o.measure("×", 22.0 * u);
        o.text(
            "×",
            [c[0] + (close - xw) / 2.0, c[1] + close * 0.72],
            22.0 * u,
            TEXT,
        );
        self.hits.push((c, Hit::SettingsClose));
        y += title;
        // Each setting: its words, whether it shows as on, and its hit.
        let toggle = |name: &str, on: bool| {
            let state = tr(if on { "setting.on" } else { "setting.off" });
            trf("setting.state", &[&tr(name), &state])
        };
        let volume = trf(
            "setting.state",
            &[
                &tr("setting.volume"),
                &trf("setting.percent", &[&self.volume()]),
            ],
        );
        let groups: [(&str, [Setting; 2]); 4] = [
            (
                "settings.sound",
                [
                    (volume, self.volume() > 0, Hit::Volume),
                    (
                        toggle("setting.ambience", self.ambience()),
                        self.ambience(),
                        Hit::Ambience,
                    ),
                ],
            ),
            (
                "settings.display",
                [
                    (
                        toggle("setting.low_power", self.low_power),
                        self.low_power,
                        Hit::Power,
                    ),
                    (
                        toggle("setting.reduce_motion", self.reduce_motion),
                        self.reduce_motion,
                        Hit::Motion,
                    ),
                ],
            ),
            (
                "settings.access",
                [
                    (
                        toggle("setting.colour_blind", self.colour_blind),
                        self.colour_blind,
                        Hit::ColourBlind,
                    ),
                    (
                        toggle("setting.large_text", self.large_text),
                        self.large_text,
                        Hit::LargeText,
                    ),
                ],
            ),
            (
                "settings.general",
                [
                    (
                        toggle("setting.vibration", self.vibration()),
                        self.vibration(),
                        Hit::Vibration,
                    ),
                    (self.language_label(), false, Hit::Language),
                ],
            ),
        ];
        let cw = (inner - (cols as f32 - 1.0) * gap) / cols as f32;
        let mut k = 0usize;
        for (head, items) in groups {
            if heads {
                let hpx = 13.0 * u;
                o.text(tr(head), [left, y + label - 9.0 * u], hpx, MUTED);
                y += label;
            }
            for (words, on, hit) in items {
                let col = k % cols;
                let (fill, ink) = if on {
                    (PRESSED, view.look.status(palette::Status::Working))
                } else {
                    (RAISED, TEXT)
                };
                let b = [left + col as f32 * (cw + gap), y, cw, bh];
                self.button(o, b, &words, 15.0 * u, fill, ink, hit);
                k += 1;
                if k % cols == 0 {
                    y += bh + gap;
                }
            }
        }
    }

    /// The start menu: the game's name over the factory, dimmed, and a
    /// button each to play on, start a new game, load one, change the
    /// settings and, in the demo, buy the full game. It takes the whole
    /// screen, so nothing builds underneath. `margins` are top, right,
    /// bottom and left, clear of notches and the home indicator.
    fn title_screen(&mut self, view: &View, o: &mut Overlay, [w, h]: [f32; 2], margins: [f32; 4]) {
        let u = self.unit;
        let [mt, mr, mb, ml] = margins;
        o.panel([0.0, 0.0], [w, h], 0.0, TITLE_DIM);
        self.hits.push(([0.0, 0.0, w, h], Hit::Block));
        let mut buttons: Vec<(String, Hit)> = vec![
            (tr("title.continue").to_string(), Hit::TitleContinue),
            (tr("menu.new_game").to_string(), Hit::TitleNew),
            (tr("title.load").to_string(), Hit::TitleLoad),
            (tr("title.settings").to_string(), Hit::TitleSettings),
        ];
        if view.store.demo.is_some() {
            buttons.push((tr("unlock.buy").to_string(), Hit::TitleBuy));
        }
        let (room_w, room_h) = (w - ml - mr, h - mt - mb);
        let bh = 52.0 * u;
        let gap = 10.0 * u;
        let name_px = 48.0 * u;
        let tag_px = 15.0 * u;
        let head = name_px + 12.0 * u + tag_px + 28.0 * u;
        let n = buttons.len() as f32;
        let one = head + n * bh + (n - 1.0) * gap;
        // A short screen, as a phone held sideways, puts the name on the
        // left and the buttons in a column on the right.
        let side = one > room_h && room_w > room_h;
        let col = if side {
            (room_w / 2.0 - gap).min(340.0 * u)
        } else {
            room_w.min(360.0 * u)
        };
        let bh = if side {
            ((room_h - (n - 1.0) * gap) / n).clamp(FINGER * view.scale.max(0.5), bh)
        } else {
            bh
        };
        let list = n * bh + (n - 1.0) * gap;
        let (name_at, list_at) = if side {
            let cx = ml + room_w / 4.0;
            let top = mt + (room_h - (name_px + 12.0 * u + tag_px)) / 2.0;
            (
                [cx, top],
                [ml + room_w / 2.0 + gap, mt + (room_h - list) / 2.0],
            )
        } else {
            let top = mt + ((room_h - one) / 2.0).max(0.0);
            let cx = ml + room_w / 2.0;
            ([cx, top], [cx - col / 2.0, top + head])
        };
        let name = "Mofact";
        let npx = fit(o, name, name_px, col);
        let nw = o.measure(name, npx);
        let y = name_at[1] + o.cap_height(npx);
        o.text(name, [name_at[0] - nw / 2.0, y], npx, TEXT);
        let tag = tr("title.tagline");
        let tpx = fit(o, tag, tag_px, col);
        let tw = o.measure(tag, tpx);
        o.text(
            tag,
            [name_at[0] - tw / 2.0, y + 12.0 * u + o.cap_height(tpx)],
            tpx,
            MUTED,
        );
        let mut y = list_at[1];
        for (i, (label, hit)) in buttons.into_iter().enumerate() {
            let (fill, ink) = if i == 0 {
                (palette::ACCENT, palette::GROUND)
            } else {
                (RAISED, TEXT)
            };
            self.button(
                o,
                [list_at[0], y, col, bh],
                &label,
                18.0 * u,
                fill,
                ink,
                hit,
            );
            y += bh + gap;
        }
    }

    /// The language button's words: the device's language until the
    /// player picks one.
    fn language_label(&self) -> String {
        let name = match self.language {
            i18n::Choice::Device => trf("setting.language_auto", &[&self.device.native()]),
            i18n::Choice::Fixed(l) => l.native().to_string(),
        };
        trf("setting.language", &[&name])
    }

    /// The die for a random seed: a rounded square with five pips.
    fn dice(&mut self, o: &mut Overlay, r: Rect) {
        let u = self.unit;
        o.panel([r[0], r[1]], [r[2], r[3]], 10.0 * u, RAISED);
        let s = 22.0 * u;
        let (x, y) = (r[0] + (r[2] - s) / 2.0, r[1] + (r[3] - s) / 2.0);
        o.panel([x, y], [s, s], 5.0 * u, TEXT);
        let p = 4.0 * u;
        for (dx, dy) in [
            (0.25, 0.25),
            (0.75, 0.25),
            (0.5, 0.5),
            (0.25, 0.75),
            (0.75, 0.75),
        ] {
            o.panel(
                [x + s * dx - p / 2.0, y + s * dy - p / 2.0],
                [p, p],
                p / 2.0,
                PANEL,
            );
        }
        self.hits.push((r, Hit::Dice));
    }

    /// A round button that opens the world map: a map folded in three. Lit
    /// while the map is open.
    fn map_button(&mut self, o: &mut Overlay, at: [f32; 2]) {
        let u = self.unit;
        let d = 36.0 * u;
        o.panel(at, [d, d], d / 2.0, if self.map { PRESSED } else { RAISED });
        let ink = if self.map { WORKING } else { TEXT };
        let (cx, cy) = (at[0] + d / 2.0, at[1] + d / 2.0);
        let (pw, ph) = (5.0 * u, 15.0 * u);
        for k in 0..3 {
            let x = cx - 8.5 * u + k as f32 * 6.0 * u;
            let y = cy - ph / 2.0 + if k == 1 { 1.5 * u } else { -1.5 * u };
            o.panel([x, y], [pw, ph], 1.5 * u, ink);
        }
        let pad = 4.0 * u;
        self.hits.push((
            [at[0] - pad, at[1] - pad, d + 2.0 * pad, d + 2.0 * pad],
            Hit::Map,
        ));
    }

    /// A round button that turns the sound off and on: a speaker's three
    /// bars, or one bar and a cross when off. Returns its right edge.
    fn sound_button(&mut self, o: &mut Overlay, at: [f32; 2]) -> f32 {
        let u = self.unit;
        let d = 36.0 * u;
        o.panel(at, [d, d], d / 2.0, RAISED);
        let (cx, cy) = (at[0] + d / 2.0, at[1] + d / 2.0);
        let bar = 4.0 * u;
        if self.muted {
            o.panel(
                [cx - 9.0 * u, cy - 3.0 * u],
                [bar, 6.0 * u],
                bar / 2.0,
                MUTED,
            );
            let px = 20.0 * u;
            let xw = o.measure("×", px);
            o.text(
                "×",
                [cx - xw / 2.0 + 4.0 * u, cy + o.cap_height(px) / 2.0],
                px,
                MUTED,
            );
        } else {
            for (k, tall) in [6.0, 11.0, 16.0].into_iter().enumerate() {
                let x = cx - 9.0 * u + k as f32 * 7.0 * u;
                o.panel(
                    [x, cy + 8.0 * u - tall * u],
                    [bar, tall * u],
                    bar / 2.0,
                    TEXT,
                );
            }
        }
        // The touch target reaches 44 points, past the drawn circle.
        let pad = 4.0 * u;
        self.hits.push((
            [at[0] - pad, at[1] - pad, d + 2.0 * pad, d + 2.0 * pad],
            Hit::Sound,
        ));
        at[0] + d
    }

    /// The research button under the stock: what labs study and how far
    /// along. Returns the bottom edge.
    /// The goal the player is on, under the research button: what it wants,
    /// how far along it is and, until tapped away, how to do it. A new goal
    /// says so in a toast. Returns its bottom edge.
    fn goal_card(&mut self, view: &View, o: &mut Overlay, at: [f32; 2], width: f32) -> f32 {
        let u = self.unit;
        let Some(g) = &view.snapshot.goal else {
            return self.order_card(view, o, at, width);
        };
        let Some(def) = view.content.goals.get(g.index as usize) else {
            return at[1] - 10.0 * u;
        };
        if self.goal_seen != Some(g.index) {
            if self.goal_seen.is_some_and(|seen| seen < g.index) {
                let text = trf("goal.met", &[&def.title]);
                self.toast = Some((text, Instant::now(), WORKING));
            }
            self.goal_seen = Some(g.index);
            self.hint = true;
        }
        let h = 44.0 * u;
        let px = 14.0 * u;
        let count = format!("{}/{}", g.have, g.need);
        let cw = o.measure(&count, px);
        let text = trf(
            "goal.progress",
            &[&(g.index + 1), &view.content.goals.len(), &def.title],
        );
        let px = fit(o, &text, px, width - 40.0 * u - cw);
        // The hint sits inside the card, so goal and hint read as one thing.
        let hint_px = 14.0 * u;
        let line = 20.0 * u;
        let lines = if self.hint {
            wrap(o, &def.hint, hint_px, width - 28.0 * u)
        } else {
            Vec::new()
        };
        let w = if lines.is_empty() {
            (o.measure(&text, px) + cw + 40.0 * u).min(width)
        } else {
            width
        };
        let total = h + lines.len() as f32 * line + if lines.is_empty() { 0.0 } else { 8.0 * u };
        o.panel(at, [w, total], 10.0 * u, RAISED);
        let base = at[1] + (h - 6.0 * u + o.cap_height(px)) / 2.0;
        o.text(&text, [at[0] + 14.0 * u, base], px, palette::ACCENT);
        o.text(&count, [at[0] + w - 14.0 * u - cw, base], 14.0 * u, MUTED);
        let bar = w - 28.0 * u;
        let done = g.have as f32 / g.need.max(1) as f32;
        o.panel(
            [at[0] + 14.0 * u, at[1] + h - 9.0 * u],
            [bar, 3.0 * u],
            1.5 * u,
            PANEL,
        );
        o.panel(
            [at[0] + 14.0 * u, at[1] + h - 9.0 * u],
            [bar * done.clamp(0.0, 1.0), 3.0 * u],
            1.5 * u,
            WORKING,
        );
        for (i, l) in lines.iter().enumerate() {
            let y = at[1] + h + 4.0 * u + o.cap_height(hint_px) + i as f32 * line;
            o.text(l, [at[0] + 14.0 * u, y], hint_px, TEXT);
        }
        // A tap anywhere on the card folds the hint away or brings it back.
        self.hits.push(([at[0], at[1], w, total], Hit::Goal));
        at[1] + total
    }

    /// The hub's order, where the goal card was once the goals are done:
    /// what it wants and the clock, what it pays and the streak. A toast
    /// says when one ships or is missed. Returns its bottom edge.
    fn order_card(&mut self, view: &View, o: &mut Overlay, at: [f32; 2], width: f32) -> f32 {
        let u = self.unit;
        let snap = view.snapshot;
        let ended = (snap.orders_shipped, snap.orders_missed);
        if let Some((shipped, missed)) = self.orders_seen {
            if ended.0 > shipped {
                self.toast = Some((tr("toast.order_shipped").into(), Instant::now(), WORKING));
            } else if ended.1 > missed {
                let ink = palette::status(palette::Status::Starved, self.colour_blind);
                self.toast = Some((tr("toast.order_missed").into(), Instant::now(), ink));
            }
        }
        self.orders_seen = Some(ended);
        let Some(order) = &snap.order else {
            return at[1] - 10.0 * u;
        };
        let name = |id: &mofact_core::ids::ItemId| {
            view.content
                .items
                .iter()
                .find(|i| &i.id == id)
                .map_or(id.as_str().to_owned(), |i| i.name.clone())
        };
        let h = 44.0 * u;
        let px = 14.0 * u;
        let left = order.seconds_left;
        let clock = format!("{}:{:02}", left / 60, left % 60);
        let count = format!("{}/{} · {clock}", order.have, order.need);
        let cw = o.measure(&count, px);
        let text = trf("order.title", &[&order.need, &name(&order.item)]);
        let px = fit(o, &text, px, width - 40.0 * u - cw);
        let pays: Vec<String> = order
            .reward
            .iter()
            .map(|(i, n)| format!("{n} × {}", name(i)))
            .collect();
        let pays = if order.streak > 0 {
            trf("order.pays_streak", &[&pays.join(", "), &order.streak])
        } else {
            trf("order.pays", &[&pays.join(", ")])
        };
        let small = 13.0 * u;
        let line = 20.0 * u;
        let lines = wrap(o, &pays, small, width - 28.0 * u);
        let total = h + lines.len() as f32 * line + 8.0 * u;
        o.panel(at, [width, total], 10.0 * u, RAISED);
        let base = at[1] + (h - 6.0 * u + o.cap_height(px)) / 2.0;
        o.text(&text, [at[0] + 14.0 * u, base], px, palette::ACCENT);
        // The last minute shows in the colour of trouble.
        let ink = if left < 60 {
            palette::status(palette::Status::Starved, self.colour_blind)
        } else {
            MUTED
        };
        o.text(&count, [at[0] + width - 14.0 * u - cw, base], 14.0 * u, ink);
        let bar = width - 28.0 * u;
        let done = order.have as f32 / order.need.max(1) as f32;
        let y = at[1] + h - 9.0 * u;
        o.panel([at[0] + 14.0 * u, y], [bar, 3.0 * u], 1.5 * u, PANEL);
        o.panel(
            [at[0] + 14.0 * u, y],
            [bar * done.clamp(0.0, 1.0), 3.0 * u],
            1.5 * u,
            WORKING,
        );
        for (i, l) in lines.iter().enumerate() {
            let y = at[1] + h + 4.0 * u + o.cap_height(small) + i as f32 * line;
            o.text(l, [at[0] + 14.0 * u, y], small, MUTED);
        }
        // Cards take their taps, so nothing is built unseen under them.
        self.hits.push(([at[0], at[1], width, total], Hit::Block));
        at[1] + total
    }

    /// The problems button, under the research button, when anything is
    /// stuck. Returns its bottom edge.
    fn problems_button(&mut self, o: &mut Overlay, at: [f32; 2], width: f32) -> f32 {
        if self.found.is_empty() {
            return at[0];
        }
        let u = self.unit;
        let h = CHIP_H * u;
        let n: usize = self.found.iter().map(|p| p.at.len()).sum();
        let text = if n == 1 {
            tr("problems.one").to_string()
        } else {
            trf("problems.many", &[&n])
        };
        let px = fit(o, &text, 14.0 * u, width - 32.0 * u);
        let w = (o.measure(&text, px) + 38.0 * u).min(width);
        let fill = if self.problems { PRESSED } else { RAISED };
        o.panel(at, [w, h], h / 2.0, fill);
        o.panel(
            [at[0] + 12.0 * u, at[1] + (h - 9.0 * u) / 2.0],
            [9.0 * u, 9.0 * u],
            4.5 * u,
            palette::status(palette::Status::Starved, self.colour_blind),
        );
        o.text(
            &text,
            [at[0] + 28.0 * u, at[1] + (h + o.cap_height(px)) / 2.0],
            px,
            TEXT,
        );
        self.hits.push(([at[0], at[1], w, h], Hit::Problems));
        at[0] + w + 8.0 * u
    }

    /// The problems list, bottom-left at `at`, at most `room` tall: one line
    /// per kind of trouble and building. A tap goes to one of them.
    fn problems_sheet(
        &mut self,
        view: &View,
        o: &mut Overlay,
        at: [f32; 2],
        width: f32,
        room: f32,
    ) {
        let u = self.unit;
        let pad = 16.0 * u;
        let row = 48.0 * u;
        let title = 30.0 * u;
        let fits = ((room - 2.0 * pad - title) / row).floor().max(1.0) as usize;
        let shown = self.found.len().min(fits);
        let height = pad + title + shown as f32 * row - 6.0 * u + pad;
        let r = [at[0], at[1] - height, width, height];
        o.panel([r[0], r[1]], [r[2], r[3]], 14.0 * u, PANEL);
        self.hits.push((r, Hit::Block));
        let left = r[0] + pad;
        let mut y = r[1] + pad;

        let px = 20.0 * u;
        o.text(
            tr("problems.title"),
            [left, y + o.cap_height(px) + 4.0 * u],
            px,
            TEXT,
        );
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

        for i in 0..shown {
            let p = &self.found[i];
            let b = [left, y, r[2] - 2.0 * pad, row - 6.0 * u];
            o.panel([b[0], b[1]], [b[2], b[3]], 10.0 * u, RAISED);
            let s = 26.0 * u;
            let mut x = b[0] + 10.0 * u;
            let icon = p.building.as_ref().and_then(|id| view.look.building(id));
            if let Some((sprite, tint)) = icon {
                o.icon(sprite, [x, b[1] + (b[3] - s) / 2.0], s, tint);
            } else {
                // A power grid: a lightning-yellow dot.
                o.panel(
                    [x + s * 0.25, b[1] + (b[3] - s * 0.5) / 2.0],
                    [s * 0.5, s * 0.5],
                    s * 0.25,
                    palette::ACCENT,
                );
            }
            x += s + 10.0 * u;
            let count = format!("×{}", p.at.len());
            let cpx = 14.0 * u;
            let cw = o.measure(&count, cpx);
            let lpx = fit(o, &p.label, 15.0 * u, b[0] + b[2] - 22.0 * u - cw - x);
            let base = b[1] + (b[3] + o.cap_height(lpx)) / 2.0;
            o.text(&p.label, [x, base], lpx, TEXT);
            if p.building.is_some() {
                o.text(&count, [b[0] + b[2] - 12.0 * u - cw, base], cpx, MUTED);
            }
            self.hits.push((b, Hit::Problem(i)));
            y += row;
        }
    }

    fn research_button(&mut self, view: &View, o: &mut Overlay, at: [f32; 2], width: f32) -> f32 {
        let r = &view.snapshot.research;
        // Research means nothing until there is a lab to do it, so the
        // button waits for the first lab (or research already under way).
        let has_lab = view.snapshot.entities.iter().any(|e| {
            view.building(&e.building)
                .is_some_and(|b| matches!(b.role, BuildingRole::Lab { .. }))
        });
        if view.content.research.is_empty() || !(has_lab || r.current.is_some() || self.research) {
            return at[0];
        }
        let u = self.unit;
        let h = CHIP_H * u;
        let current = r
            .current
            .as_ref()
            .and_then(|id| view.content.research.iter().find(|d| &d.id == id));
        let (text, ink) = match current {
            Some(d) => (
                format!("{} {}/{}", d.name, r.progress.0, r.progress.1),
                TEXT,
            ),
            None if view.content.research.iter().any(|d| view.research_ready(d)) => {
                (tr("research.pick").to_string(), palette::ACCENT)
            }
            None => (tr("research.all_done").to_string(), MUTED),
        };
        let px = fit(o, &text, 14.0 * u, width - 28.0 * u);
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
        at[0] + w + 8.0 * u
    }

    /// The recipe browser for `item`, bottom-left at `at`, at most `room`
    /// tall: the recipes that make it, then those that use it. Every other
    /// item's icon in it opens the browser on that item.
    fn browser(
        &mut self,
        view: &View,
        o: &mut Overlay,
        item: &ItemId,
        at: [f32; 2],
        width: f32,
        room: f32,
    ) {
        let u = self.unit;
        let pad = 16.0 * u;
        let title = 34.0 * u;
        let head = 24.0 * u;
        let row = 50.0 * u;
        let has = |list: &[Stack]| list.iter().any(|s| &s.item == item);
        let made: Vec<&RecipeDef> = view
            .content
            .recipes
            .iter()
            .filter(|r| has(&r.outputs))
            .collect();
        let used: Vec<&RecipeDef> = view
            .content
            .recipes
            .iter()
            .filter(|r| has(&r.inputs))
            .collect();
        let research: Vec<&ResearchDef> = view
            .content
            .research
            .iter()
            .filter(|r| has(&r.cost))
            .collect();
        // Fit what we can, makers first.
        let mut budget = ((room - 2.0 * pad - title) / row).floor().max(1.0) as usize;
        let take = |budget: &mut usize, n: usize| {
            let k = n.min(budget.saturating_sub(usize::from(n > 0)));
            *budget -= k + usize::from(k > 0);
            k
        };
        // The ratio calculator, for what something makes: a chip for each
        // machine of the chain, wrapped into lines.
        let chip = 34.0 * u;
        let line = chip + 6.0 * u;
        let cpx = 13.0 * u;
        let chips: Vec<(BuildingId, u32)> = if made.is_empty() {
            Vec::new()
        } else {
            let p = plan::plan(
                view.content,
                item,
                self.rate_per_minute() as f32,
                |r| view.recipe_open(r),
                |b| view.building_open(b),
            );
            let mut c = p.machines();
            c.extend(p.belts);
            c
        };
        let chip_w = |o: &Overlay, n: u32| 24.0 * u + o.measure(&format!("×{n}"), cpx) + 18.0 * u;
        let lines = {
            let (mut lines, mut x) = (0usize, f32::INFINITY);
            for (_, n) in &chips {
                let w = chip_w(o, *n);
                if x + w > width - 2.0 * pad {
                    lines += 1;
                    x = 0.0;
                }
                x += w + 6.0 * u;
            }
            lines
        };
        // The calculator's lines, in browser rows, if there is room for them
        // and one recipe that makes the item: the plan matters more than a
        // second way to make it.
        let plan_rows = (lines as f32 * line / row).ceil() as usize;
        let np = if chips.is_empty() || budget < plan_rows + 3 {
            0
        } else {
            take(&mut budget, plan_rows)
        };
        let nm = take(&mut budget, made.len());
        let nu = take(&mut budget, used.len());
        let nr = usize::from(!research.is_empty() && take(&mut budget, 1) > 0);
        let sections = [nm, np, nu, nr].iter().filter(|&&n| n > 0).count() as f32;
        let rows = (nm + nu + nr) as f32
            + if np > 0 {
                lines as f32 * line / row
            } else {
                0.0
            };
        let empty = rows == 0.0;
        let height =
            pad + title + sections * head + rows * row + if empty { row } else { 0.0 } + pad;
        let r = [at[0], at[1] - height, width, height];
        o.panel([r[0], r[1]], [r[2], r[3]], 14.0 * u, PANEL);
        self.hits.push((r, Hit::Block));
        let left = r[0] + pad;
        let mut y = r[1] + pad;

        // Title: the item's icon and name, and a close button.
        let name = view
            .content
            .items
            .iter()
            .find(|i| &i.id == item)
            .map_or(item.as_str(), |i| i.name.as_str());
        let s = 26.0 * u;
        let mut x = left;
        if let Some((sprite, t)) = view.look.item(item) {
            o.icon(sprite, [x, y], s, t);
            x += s + 8.0 * u;
        }
        let px = 20.0 * u;
        o.text(name, [x, y + (s + o.cap_height(px)) / 2.0], px, TEXT);
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

        let hpx = 12.0 * u;
        let header = |o: &mut Overlay, y: &mut f32, text: &str| {
            o.text(text, [left, *y + head * 0.7], hpx, MUTED);
            *y += head;
        };
        if empty {
            o.text(tr("browser.none"), [left, y + row / 2.0], 14.0 * u, MUTED);
        }
        for (label, list, n) in [
            ("browser.made_by", &made, nm),
            ("browser.used_in", &used, nu),
        ] {
            if n == 0 {
                continue;
            }
            header(o, &mut y, tr(label));
            for rd in &list[..n] {
                self.recipe_row(
                    view,
                    o,
                    rd,
                    item,
                    [left, y, r[2] - 2.0 * pad, row - 6.0 * u],
                );
                y += row;
            }
            if label == "browser.made_by" && np > 0 {
                y = self.plan_lines(view, o, &chips, [left, y, r[2] - 2.0 * pad], head, line);
            }
        }
        if nr > 0 {
            header(o, &mut y, tr("browser.research"));
            let names: Vec<&str> = research.iter().map(|r| r.name.as_str()).collect();
            let text = i18n::join(&names);
            let px = fit(o, &text, 14.0 * u, r[2] - 2.0 * pad);
            o.text(&text, [left, y + row / 2.0], px, TEXT);
        }
    }

    /// The ratio calculator in the browser at `b` (left, top, width): a
    /// header with the rate and its buttons, then a chip for each machine.
    /// Returns where it ends.
    fn plan_lines(
        &mut self,
        view: &View,
        o: &mut Overlay,
        chips: &[(BuildingId, u32)],
        b: [f32; 3],
        head: f32,
        line: f32,
    ) -> f32 {
        let u = self.unit;
        let [left, mut y, width] = b;
        o.text(
            tr("browser.to_make"),
            [left, y + head * 0.7],
            12.0 * u,
            MUTED,
        );
        // − rate +, on the right of the header.
        let btn = 30.0 * u;
        let rate = trf("unit.per_min", &[&self.rate_per_minute()]);
        let rpx = 14.0 * u;
        let rw = o.measure(&trf("unit.per_min", &[&1200]), rpx);
        let right = left + width;
        let by = y + (head - btn) / 2.0;
        let plus = [right - btn, by, btn, btn];
        let minus = [right - btn * 2.0 - rw - 12.0 * u, by, btn, btn];
        for (r, label, up) in [(minus, "−", false), (plus, "+", true)] {
            o.panel([r[0], r[1]], [btn, btn], btn / 2.0, RAISED);
            let w = o.measure(label, 20.0 * u);
            o.text(
                label,
                [r[0] + (btn - w) / 2.0, r[1] + btn * 0.72],
                20.0 * u,
                TEXT,
            );
            // A finger's width to hit, though drawn smaller.
            let f = 44.0 * u;
            let hit = [r[0] - (f - btn) / 2.0, r[1] - (f - btn) / 2.0, f, f];
            self.hits.push((hit, Hit::Rate(up)));
        }
        let tw = o.measure(&rate, rpx);
        let mid = minus[0] + btn + (plus[0] - minus[0] - btn - tw) / 2.0;
        o.text(
            &rate,
            [mid, by + (btn + o.cap_height(rpx)) / 2.0],
            rpx,
            TEXT,
        );
        y += head;
        let chip = line - 6.0 * u;
        let cpx = 13.0 * u;
        let s = 24.0 * u;
        let mut x = f32::INFINITY;
        let mut top = y - line;
        for (building, n) in chips {
            let text = format!("×{n}");
            let w = s + o.measure(&text, cpx) + 18.0 * u;
            if x + w > right {
                x = left;
                top += line;
            }
            o.panel([x, top], [w, chip], 8.0 * u, RAISED);
            if let Some((sprite, t)) = view.look.building(building) {
                o.icon(sprite, [x + 6.0 * u, top + (chip - s) / 2.0], s, t);
            }
            o.text(
                &text,
                [x + s + 10.0 * u, top + (chip + o.cap_height(cpx)) / 2.0],
                cpx,
                TEXT,
            );
            x += w + 6.0 * u;
        }
        top + line
    }

    /// One recipe in the browser: inputs, an arrow, outputs, then its
    /// name, time and the buildings that make it. Icons other than `item`
    /// open the browser on theirs.
    fn recipe_row(&mut self, view: &View, o: &mut Overlay, rd: &RecipeDef, item: &ItemId, b: Rect) {
        let u = self.unit;
        let open = view.recipe_open(&rd.id);
        o.panel([b[0], b[1]], [b[2], b[3]], 10.0 * u, RAISED);
        let s = 20.0 * u;
        let px = 13.0 * u;
        let top = b[1] + 4.0 * u;
        let mut x = b[0] + 10.0 * u;
        let stacks = |hud: &mut Hud, o: &mut Overlay, x: &mut f32, list: &[Stack]| {
            for st in list {
                let start = *x;
                if let Some((sprite, t)) = view.look.item(&st.item) {
                    o.icon(sprite, [*x, top], s, t);
                }
                *x += s + 2.0 * u;
                *x += o.text(&st.count.to_string(), [*x, top + s * 0.8], px, TEXT) + 8.0 * u;
                if &st.item != item {
                    hud.hits.push((
                        [start - 2.0 * u, b[1], *x - start, b[3]],
                        Hit::Item(st.item.clone()),
                    ));
                }
            }
        };
        stacks(self, o, &mut x, &rd.inputs);
        x += o.text("→", [x, top + s * 0.8], px, MUTED) + 8.0 * u;
        stacks(self, o, &mut x, &rd.outputs);
        let makers: Vec<&str> = view
            .content
            .buildings
            .iter()
            .filter(|bd| match &bd.role {
                BuildingRole::Crafter { categories, .. } => categories.contains(&rd.category),
                _ => false,
            })
            .map(|bd| bd.name.as_str())
            .collect();
        let seconds = trf("unit.seconds", &[&format!("{:.1}", rd.seconds)]);
        let mut text = format!("{} · {seconds} · {}", rd.name, i18n::join(&makers));
        if !open {
            text = format!("{text} · {}", tr("state.locked"));
        }
        let spx = fit(o, &text, 11.0 * u, b[2] - 20.0 * u);
        let ink = if open { MUTED } else { palette::STARVE };
        o.text(&text, [b[0] + 10.0 * u, b[1] + b[3] - 7.0 * u], spx, ink);
    }

    /// The radial build menu around `at`: every open building but the hub
    /// and belts in a ring, kept on a `screen`-sized screen. A tap on one
    /// picks it; a tap anywhere else closes the menu.
    fn radial_menu(&mut self, view: &View, o: &mut Overlay, at: [f32; 2], screen: [f32; 2]) {
        let u = self.unit;
        let list: Vec<&BuildingDef> = view
            .content
            .buildings
            .iter()
            .filter(|b| {
                !matches!(b.role, BuildingRole::Hub | BuildingRole::Belt { .. })
                    && view.building_open(&b.id)
            })
            .collect();
        // Everywhere else closes it; a scrim says so and keeps the ring readable.
        o.panel([0.0, 0.0], screen, 0.0, [0, 0, 0, 0x70]);
        self.hits
            .push(([0.0, 0.0, screen[0], screen[1]], Hit::Close));
        let d = 60.0 * u;
        let n = list.len().max(1) as f32;
        // Room on the ring for every button with a little gap.
        let r = ((d + 10.0 * u) * n / std::f32::consts::TAU).max(80.0 * u);
        let edge = r + d / 2.0 + 18.0 * u;
        let c = [
            at[0].clamp(edge, (screen[0] - edge).max(edge)),
            at[1].clamp(edge, (screen[1] - edge).max(edge)),
        ];
        o.panel(
            [c[0] - 6.0 * u, c[1] - 6.0 * u],
            [12.0 * u, 12.0 * u],
            6.0 * u,
            TEXT,
        );
        for (k, b) in list.iter().enumerate() {
            let a = -std::f32::consts::FRAC_PI_2 + k as f32 * std::f32::consts::TAU / n;
            let (x, y) = (c[0] + r * a.cos() - d / 2.0, c[1] + r * a.sin() - d / 2.0);
            let afford = view.affordable(&b.cost);
            let ring = 2.0 * u;
            o.panel(
                [x - ring, y - ring],
                [d + 2.0 * ring, d + 2.0 * ring],
                d / 2.0 + ring,
                PANEL,
            );
            o.panel([x, y], [d, d], d / 2.0, RAISED);
            let icon = 32.0 * u;
            if let Some((sprite, t)) = view.look.building(&b.id) {
                let t = if afford { t } else { [t[0], t[1], t[2], 0x70] };
                o.icon(sprite, [x + (d - icon) / 2.0, y + 6.0 * u], icon, t);
            }
            let px = fit(o, &b.name, 10.0 * u, d - 6.0 * u);
            let lw = o.measure(&b.name, px);
            let ink = if afford { TEXT } else { palette::STARVE };
            o.text(&b.name, [x + (d - lw) / 2.0, y + d - 8.0 * u], px, ink);
            self.hits.push(([x, y, d, d], Hit::Build(b.id.clone())));
        }
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
        // What can be built now comes first; locked buildings follow.
        let mut list = list;
        list.sort_by_key(|b| !view.building_open(&b.id));
        let inner = width - 2.0 * pad;
        let cols = ((inner + gap) / (96.0 * u + gap)).floor().max(1.0) as usize;
        let cw = (inner - gap * (cols - 1) as f32) / cols as f32;
        let ch = 92.0 * u;
        let fits = ((room - 2.0 * pad - title + gap) / (ch + gap))
            .floor()
            .max(1.0) as usize;
        let rows = list.len().div_ceil(cols).clamp(1, fits);
        // What doesn't fit goes on further pages.
        let per = rows * cols;
        let pages = list.len().div_ceil(per).max(1);
        self.page = self.page.min(pages - 1);
        let shown = &list[self.page * per..list.len().min((self.page + 1) * per)];
        let height = pad + title + rows as f32 * (ch + gap) - gap + pad;
        let r = [at[0], at[1] - height, width, height];
        o.panel([r[0], r[1]], [r[2], r[3]], 14.0 * u, PANEL);
        self.hits.push((r, Hit::Block));
        let left = r[0] + pad;
        let top = r[1] + pad;
        let px = 20.0 * u;
        o.text(
            tr("build.title"),
            [left, top + o.cap_height(px) + 4.0 * u],
            px,
            TEXT,
        );
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
        self.pager(o, c, pages);

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
                let locked = tr("state.locked");
                let lpx = fit(o, locked, lpx, cw - 4.0 * u);
                let lw = o.measure(locked, lpx);
                o.text(locked, [b[0] + (cw - lw) / 2.0, y + lpx * 0.85], lpx, MUTED);
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

    /// A sheet's page arrows and "2/3", left of its close button `c`, when
    /// it has more than one page.
    fn pager(&mut self, o: &mut Overlay, c: [f32; 4], pages: usize) {
        if pages < 2 {
            return;
        }
        let u = self.unit;
        let size = c[3];
        let gap = 8.0 * u;
        let px = 15.0 * u;
        let label = format!("{}/{}", self.page + 1, pages);
        let lw = o.measure(&label, px);
        let next = [c[0] - gap - size, c[1], size, size];
        let lx = next[0] - gap - lw;
        let back = [lx - gap - size, c[1], size, size];
        o.text(
            &label,
            [lx, c[1] + size * 0.5 + o.cap_height(px) / 2.0],
            px,
            MUTED,
        );
        for (b, on, glyph) in [(back, false, "‹"), (next, true, "›")] {
            let live = if on {
                self.page + 1 < pages
            } else {
                self.page > 0
            };
            o.panel([b[0], b[1]], [size, size], size / 2.0, RAISED);
            let gpx = 24.0 * u;
            let gw = o.measure(glyph, gpx);
            let ink = if live { TEXT } else { MUTED };
            o.text(
                glyph,
                [b[0] + (size - gw) / 2.0, b[1] + size * 0.74],
                gpx,
                ink,
            );
            if live {
                self.hits.push((b, Hit::Page(on)));
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
        // What can be picked comes first, then what is locked, then what is
        // done, so the rows that fit are the ones worth a tap.
        let current = view.snapshot.research.current.as_ref();
        let mut shown: Vec<&ResearchDef> = view.content.research.iter().collect();
        shown.sort_by_key(|d| {
            if current == Some(&d.id) {
                0
            } else if view.research_ready(d) {
                1
            } else if view.done(&d.id) {
                3
            } else {
                2
            }
        });
        // What doesn't fit goes on further pages, every page as tall.
        let pages = shown.len().div_ceil(fits).max(1);
        self.page = self.page.min(pages - 1);
        let rows = shown.len().min(fits);
        let shown: Vec<&ResearchDef> = shown
            .into_iter()
            .skip(self.page * fits)
            .take(fits)
            .collect();
        let height = pad + title + rows as f32 * row - 6.0 * u + pad;
        let r = [at[0], at[1] - height, width, height];
        o.panel([r[0], r[1]], [r[2], r[3]], 14.0 * u, PANEL);
        self.hits.push((r, Hit::Block));
        let left = r[0] + pad;
        let mut y = r[1] + pad;

        let px = 20.0 * u;
        o.text(
            tr("research.title"),
            [left, y + o.cap_height(px) + 4.0 * u],
            px,
            TEXT,
        );
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
        self.pager(o, c, pages);
        y += title;

        for d in shown {
            let b = [left, y, r[2] - 2.0 * pad, row - 6.0 * u];
            let is_current = current == Some(&d.id);
            let again = d.repeat.is_some();
            let locked = !(view.done(&d.id) && !again)
                && !is_current
                && !view.gated(d)
                && !view.research_ready(d);
            let (label, color, hit) = if view.done(&d.id) && !again {
                (tr("research.done"), WORKING, None)
            } else if is_current {
                (
                    tr("research.studying"),
                    palette::ACCENT,
                    Some(Hit::Study(None)),
                )
            } else if view.gated(d) {
                (tr("research.full_game"), palette::ACCENT, Some(Hit::Unlock))
            } else if view.research_ready(d) {
                (
                    tr("research.study"),
                    TEXT,
                    Some(Hit::Study(Some(d.id.clone()))),
                )
            } else {
                (tr("state.locked"), MUTED, None)
            };
            let fill = if is_current { PRESSED } else { RAISED };
            o.panel([b[0], b[1]], [b[2], b[3]], 10.0 * u, fill);
            let npx = 15.0 * u;
            let ink = if locked { MUTED } else { TEXT };
            // Repeatable research says which level is next, and what it gives.
            let level = view.level(&d.id);
            let name = match d.repeat {
                Some(rep) => trf(
                    "research.repeat",
                    &[
                        &d.name,
                        &(level + 1),
                        &format!("{:.0}", rep.per_level * 100.0),
                        &tr(match rep.bonus {
                            Bonus::Mining => "bonus.mining",
                            Bonus::Crafting => "bonus.crafting",
                            Bonus::Research => "bonus.research",
                        }),
                    ],
                ),
                None => d.name.clone(),
            };
            let name_px = fit(o, &name, npx, b[2] - 100.0 * u);
            o.text(
                &name,
                [b[0] + 12.0 * u, b[1] + 6.0 * u + o.cap_height(npx)],
                name_px,
                ink,
            );
            // Cost per unit, times the units (the next level's, for repeatable).
            let spx = 12.0 * u;
            let s = 14.0 * u;
            let sy = b[1] + b[3] - 8.0 * u - s;
            let mut x = b[0] + 12.0 * u;
            let units = if again {
                d.units * (level + 1)
            } else {
                d.units
            };
            x += o.text(&format!("{units}×"), [x, sy + s * 0.85], spx, MUTED) + 4.0 * u;
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

    /// The full game sheet: what it opens, and buying or restoring it.
    fn unlock_sheet(&mut self, view: &View, o: &mut Overlay, at: [f32; 2], width: f32, room: f32) {
        let u = self.unit;
        let pad = 16.0 * u;
        let gap = 8.0 * u;
        let line = 22.0 * u;
        let first = view.store.demo.unwrap_or(0) + 1;
        let last = view
            .content
            .research
            .iter()
            .map(|r| r.tier)
            .max()
            .unwrap_or(first);
        let note = view.store.note.as_deref();
        let tall = |lines: usize, bh: f32| {
            pad + 44.0 * u
                + lines as f32 * line
                + gap
                + 2.0 * bh
                + gap
                + if note.is_some() { line + gap } else { 0.0 }
                + pad
        };
        // On a short screen the words come down to a line or two.
        let lpx = 15.0 * u;
        let inner = width - 2.0 * pad;
        let long = trf("unlock.long", &[&(first - 1), &first, &last]);
        let long = wrap(o, &long, lpx, inner);
        let (lines, bh) = if tall(long.len(), 48.0 * u) <= room {
            (long, 48.0 * u)
        } else {
            let short = trf("unlock.short", &[&first, &last]);
            (wrap(o, &short, lpx, inner), 44.0 * u)
        };
        let height = tall(lines.len(), bh);
        let r = [at[0], at[1] - height, width, height];
        o.panel([r[0], r[1]], [r[2], r[3]], 14.0 * u, PANEL);
        self.hits.push((r, Hit::Block));
        let left = r[0] + pad;
        let inner = r[2] - 2.0 * pad;
        let mut y = r[1] + pad;
        let px = 20.0 * u;
        let close = 44.0 * u;
        let px = fit(o, tr("unlock.title"), px, inner - close - gap);
        o.text(
            tr("unlock.title"),
            [left, y + (close + o.cap_height(px)) / 2.0],
            px,
            TEXT,
        );
        let c = [r[0] + r[2] - pad - close, y, close, close];
        o.panel([c[0], c[1]], [close, close], close / 2.0, RAISED);
        let xw = o.measure("×", 22.0 * u);
        o.text(
            "×",
            [c[0] + (close - xw) / 2.0, c[1] + close * 0.72],
            22.0 * u,
            TEXT,
        );
        self.hits.push((c, Hit::Close));
        y += 44.0 * u;
        for l in &lines {
            let lpx = fit(o, l, lpx, inner);
            o.text(l, [left, y + o.cap_height(lpx) + 4.0 * u], lpx, MUTED);
            y += line;
        }
        y += gap;
        let busy = view.store.busy;
        let buy = match (&view.store.price, busy) {
            (_, true) => tr("unlock.waiting").to_string(),
            (Some(p), false) => trf("unlock.buy_price", &[p]),
            (None, false) => tr("unlock.buy").to_string(),
        };
        let (fill, hit) = if busy {
            (RAISED, Hit::Block)
        } else {
            (palette::ACCENT, Hit::Buy)
        };
        let ink = if busy { MUTED } else { palette::GROUND };
        let bpx = fit(o, &buy, 16.0 * u, inner - 24.0 * u);
        self.button(o, [left, y, inner, bh], &buy, bpx, fill, ink, hit);
        y += bh + gap;
        let hit = if busy { Hit::Block } else { Hit::Restore };
        self.button(
            o,
            [left, y, inner, bh],
            tr("unlock.restore"),
            15.0 * u,
            RAISED,
            TEXT,
            hit,
        );
        y += bh + gap;
        if let Some(n) = note {
            let npx = fit(o, n, 14.0 * u, inner);
            o.text(n, [left, y + o.cap_height(npx) + 2.0 * u], npx, MUTED);
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
                // An icon with no name sits alone in the middle.
                let space = if name.is_empty() { 0.0 } else { 6.0 * u };
                let w = 14.0 * u + icon.map_or(0.0, |_| s + space) + o.measure(name, px) + 14.0 * u;
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

    /// The recipe pills for a crafter, filling up from `at`: Auto and one
    /// pill per product, and under them, when the chosen product can be
    /// made more than one way, a pill per way. Products lose their names
    /// when they would take more than three rows. Returns the rows.
    #[allow(clippy::too_many_arguments)]
    fn recipe_picker(
        &mut self,
        o: &mut Overlay,
        view: &View,
        recipes: &[&RecipeDef],
        current: Option<&RecipeId>,
        hit: impl Fn(Option<RecipeId>) -> Hit,
        at: [f32; 2],
        width: f32,
        draw: bool,
    ) -> usize {
        let product = |r: &RecipeDef| r.outputs.first().map(|s| s.item.clone());
        let chosen = current.and_then(|c| recipes.iter().find(|r| &r.id == c));
        let made = chosen.and_then(|r| product(r));
        let mut firsts: Vec<&RecipeDef> = Vec::new();
        for r in recipes {
            if !firsts.iter().any(|f| product(f) == product(r)) {
                firsts.push(r);
            }
        }
        let products = |named: bool| -> Vec<Pill> {
            let auto = (tr("recipe.auto"), None, current.is_none(), hit(None));
            std::iter::once(auto)
                .chain(firsts.iter().map(|r| {
                    let item = product(r);
                    let name = match (named, &item) {
                        (true, Some(i)) => view
                            .content
                            .items
                            .iter()
                            .find(|d| &d.id == i)
                            .map_or(r.name.as_str(), |d| d.name.as_str()),
                        _ => "",
                    };
                    let icon = item.as_ref().and_then(|i| view.look.item(i));
                    (
                        name,
                        icon,
                        item.is_some() && item == made,
                        hit(Some(r.id.clone())),
                    )
                }))
                .collect()
        };
        let ways: Vec<Pill> = match &made {
            Some(m)
                if recipes
                    .iter()
                    .filter(|r| product(r).as_ref() == Some(m))
                    .count()
                    > 1 =>
            {
                recipes
                    .iter()
                    .filter(|r| product(r).as_ref() == Some(m))
                    .map(|r| {
                        let this = Some(&r.id) == current;
                        (r.name.as_str(), None, this, hit(Some(r.id.clone())))
                    })
                    .collect()
            }
            _ => Vec::new(),
        };
        let mut rows = 0;
        if !ways.is_empty() {
            rows = self.pills(o, ways, at, width, draw);
        }
        let named = self.pills(o, products(true), [0.0, 0.0], width, false);
        let list = products(named <= 3);
        let pitch = 46.0 * self.unit;
        rows + self.pills(o, list, [at[0], at[1] - rows as f32 * pitch], width, draw)
    }

    /// A requester chest's pills (T49): nothing, or one of the solid items
    /// in stock (most first), and then how many to keep. Returns the rows.
    #[allow(clippy::too_many_arguments)]
    fn request_picker(
        &mut self,
        o: &mut Overlay,
        view: &View,
        e: &EntityView,
        at: [f32; 2],
        width: f32,
        draw: bool,
    ) -> usize {
        let current = e.request.clone();
        let count = current.as_ref().map_or(REQUEST_COUNTS[1], |r| r.1);
        let items = solid_items(view, current.as_ref().map(|c| &c.0));
        let none = (
            tr("request.none"),
            None,
            current.is_none(),
            Hit::SetRequest(e.id, None),
        );
        let list: Vec<Pill> = std::iter::once(none)
            .chain(items.iter().map(|i| {
                let on = current.as_ref().is_some_and(|c| &c.0 == *i);
                (
                    "",
                    view.look.item(i),
                    on,
                    Hit::SetRequest(e.id, Some(((*i).clone(), count))),
                )
            }))
            .collect();
        let mut rows = 0;
        if let Some((item, n)) = &current {
            let labels: Vec<String> = REQUEST_COUNTS.iter().map(|c| c.to_string()).collect();
            let counts: Vec<Pill> = REQUEST_COUNTS
                .iter()
                .zip(&labels)
                .map(|(c, label)| {
                    (
                        label.as_str(),
                        None,
                        c == n,
                        Hit::SetRequest(e.id, Some((item.clone(), *c))),
                    )
                })
                .collect();
            rows = self.pills(o, counts, at, width, draw);
        }
        let pitch = 46.0 * self.unit;
        rows + self.pills(o, list, [at[0], at[1] - rows as f32 * pitch], width, draw)
    }

    /// A filter splitter's pills: nothing, or one of the solid items in
    /// stock (most first) to send ahead. A warehouse's: everything, or one
    /// of the items it holds (most first) to hand out. Returns the rows.
    fn filter_picker(
        &mut self,
        o: &mut Overlay,
        view: &View,
        e: &EntityView,
        at: [f32; 2],
        width: f32,
        draw: bool,
    ) -> usize {
        let current = e.filter.as_ref();
        let warehouse = view
            .building(&e.building)
            .is_some_and(|d| matches!(d.role, BuildingRole::Warehouse { .. }));
        let items = if warehouse {
            held_items(e, current)
        } else {
            solid_items(view, current)
        };
        let none = (
            if warehouse {
                tr("warehouse.all")
            } else {
                tr("request.none")
            },
            None,
            current.is_none(),
            Hit::SetFilter(e.id, None),
        );
        let list: Vec<Pill> = std::iter::once(none)
            .chain(items.iter().map(|i| {
                (
                    "",
                    view.look.item(i),
                    current == Some(*i),
                    Hit::SetFilter(e.id, Some((*i).clone())),
                )
            }))
            .collect();
        self.pills(o, list, at, width, draw)
    }

    /// The inspect sheet for `e`, bottom-left at `at`.
    fn sheet(&mut self, view: &View, o: &mut Overlay, e: &EntityView, at: [f32; 2], width: f32) {
        let u = self.unit;
        let pad = 16.0 * u;
        let def = view.building(&e.building);
        let hub = def.is_some_and(|d| d.role == BuildingRole::Hub);
        // Belts, splitters and underground belts only carry: nothing to show
        // but the controls. Splitters of other modes say what they do.
        let belt = def.is_some_and(|d| {
            matches!(
                d.role,
                BuildingRole::Belt { .. }
                    | BuildingRole::Splitter {
                        mode: SplitMode::Even
                    }
                    | BuildingRole::Underground { .. }
            )
        });
        let splits = def.is_some_and(|d| matches!(d.role, BuildingRole::Splitter { .. }));
        // What a warehouse can hold; it shows what it does, and picks
        // what it hands out like a filter splitter.
        let warehouse = def.and_then(|d| match d.role {
            BuildingRole::Warehouse { capacity } => Some(capacity),
            _ => None,
        });
        let filter = warehouse.is_some()
            || def.is_some_and(|d| {
                d.role
                    == BuildingRole::Splitter {
                        mode: SplitMode::Filter,
                    }
            });
        let pole = def.is_some_and(|d| matches!(d.role, BuildingRole::Pole { .. }));
        let generator = def.and_then(|d| match d.role {
            BuildingRole::Generator { kw } => Some(kw),
            _ => None,
        });
        let engine = def.and_then(|d| match &d.role {
            BuildingRole::Engine { kw, fluid, .. } => Some((*kw, fluid)),
            _ => None,
        });
        // Pipes, tanks and underground pipes show what they hold instead of a state.
        let plumbing = def.is_some_and(|d| {
            matches!(
                d.role,
                BuildingRole::Pipe { .. }
                    | BuildingRole::Tank { .. }
                    | BuildingRole::UndergroundPipe { .. }
            )
        });
        let pump = def.is_some_and(|d| matches!(d.role, BuildingRole::Pump { .. }));
        // A megaproject shows its stage and what that stage still needs.
        let mega = view.snapshot.mega.as_ref().filter(|m| {
            m.id == e.id && def.is_some_and(|d| matches!(d.role, BuildingRole::Megaproject { .. }))
        });
        let well = def.and_then(|d| match &d.role {
            BuildingRole::Well { fluid, .. } => Some(fluid),
            _ => None,
        });
        let fluid_name = |id: &mofact_core::ids::ItemId| {
            view.content
                .items
                .iter()
                .find(|i| &i.id == id)
                .map_or(id.as_str().to_owned(), |i| i.name.clone())
        };
        // Poles and generators have no state of their own to show.
        let wired = pole || generator.is_some();
        let grid = e.grid.and_then(|g| view.snapshot.grids.get(g as usize));
        let recipes = view.recipes_for(&e.building);
        let recipe = e.recipe.as_ref().and_then(|r| view.recipe_def(r));

        // Measure the sheet from its rows, bottom up.
        let row = 22.0 * u;
        let set = |id| Hit::SetRecipe(e.id, id);
        let pill_rows = recipes.as_ref().map_or(0.0, |rs| {
            let current = e.recipe.as_ref();
            self.recipe_picker(
                o,
                view,
                rs,
                current,
                set,
                [0.0, 0.0],
                width - 2.0 * pad,
                false,
            ) as f32
        });
        let requester = def.is_some_and(|d| matches!(d.role, BuildingRole::RequesterChest { .. }));
        let pill_rows = if requester {
            self.request_picker(o, view, e, [0.0, 0.0], width - 2.0 * pad, false) as f32
        } else if filter {
            self.filter_picker(o, view, e, [0.0, 0.0], width - 2.0 * pad, false) as f32
        } else {
            pill_rows
        };
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
        if !belt && !splits && !hub && !wired && !plumbing && mega.is_none() {
            let (label, color) = state(e.state, view.look);
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
                let text = tr("inspect.hub");
                let px = fit(o, text, px, r[2] - 2.0 * pad);
                o.text(text, [x, base], px, MUTED);
            } else if let Some(m) = mega {
                if m.done() {
                    o.text(tr("inspect.won"), [x, base], px, TEXT);
                } else {
                    let head = trf("inspect.stage", &[&m.name, &(m.stage + 1), &m.stages]);
                    x += o.text(&head, [x, base], px, MUTED) + 10.0 * u;
                    for (item, have, need) in &m.needs {
                        let start = x;
                        if let Some((sprite, t)) = view.look.item(item) {
                            o.icon(sprite, [x, y + (row - s) / 2.0], s, t);
                        }
                        x += s + 2.0 * u;
                        let color = if have >= need { MUTED } else { TEXT };
                        x += o.text(&format!("{have}/{need}"), [x, base], px, color) + 8.0 * u;
                        self.hits
                            .push(([start, y, x - start, row], Hit::Item(item.clone())));
                    }
                }
            } else if let Some((kw, fluid)) = engine {
                let text = match grid {
                    Some(_) => trf(
                        "inspect.engine_on",
                        &[
                            &format!("{:.0}", e.progress.clamp(0.0, 1.0) * kw),
                            &format!("{kw:.0}"),
                            &i18n::in_sentence(&fluid_name(fluid)),
                        ],
                    ),
                    None => trf("inspect.engine_off", &[&format!("{kw:.0}")]),
                };
                let px = fit(o, &text, px, r[2] - 2.0 * pad);
                o.text(&text, [x, base], px, MUTED);
            } else if wired {
                let text = match (grid, generator) {
                    (Some(g), _) => trf(
                        "inspect.grid",
                        &[
                            &format!("{:.0}", g.demand_kw),
                            &format!("{:.0}", g.supply_kw),
                            &format!("{:.0}", g.satisfaction * 100.0),
                        ],
                    ),
                    (None, Some(kw)) => trf("inspect.generator_off", &[&format!("{kw:.0}")]),
                    (None, None) => tr("inspect.unlinked").to_string(),
                };
                let px = fit(o, &text, px, r[2] - 2.0 * pad);
                o.text(&text, [x, base], px, MUTED);
            } else if plumbing || pump || well.is_some() {
                let text = match (&e.fluid, well) {
                    (_, Some(f)) => trf("inspect.well", &[&i18n::in_sentence(&fluid_name(f))]),
                    _ if pump => tr("inspect.pump").to_string(),
                    (Some(f), _) => trf(
                        "inspect.fluid",
                        &[
                            &fluid_name(&f.item),
                            &format!("{:.0}", f.fill.clamp(0.0, 1.0) * 100.0),
                        ],
                    ),
                    (None, _) => tr("inspect.empty").to_string(),
                };
                let px = fit(o, &text, px, r[2] - 2.0 * pad);
                o.text(&text, [x, base], px, MUTED);
            } else if let Some(capacity) = warehouse {
                // How full it is, then what it holds, most first, as fits.
                let total: u32 = e.held.iter().map(|h| h.1).sum();
                if total == 0 {
                    let text = tr("inspect.warehouse_empty");
                    let px = fit(o, text, px, r[2] - 2.0 * pad);
                    o.text(text, [x, base], px, MUTED);
                } else {
                    let head = trf("inspect.warehouse", &[&total, &capacity]);
                    x += o.text(&head, [x, base], px, MUTED) + 10.0 * u;
                    let mut held: Vec<_> = e.held.iter().collect();
                    held.sort_by_key(|h| std::cmp::Reverse(h.1));
                    let end = r[0] + r[2] - pad;
                    for (item, n) in held {
                        let label = format!("{n}");
                        let w = s + 2.0 * u + o.measure(&label, px) + 8.0 * u;
                        if x + w > end {
                            break;
                        }
                        let start = x;
                        if let Some((sprite, t)) = view.look.item(item) {
                            o.icon(sprite, [x, y + (row - s) / 2.0], s, t);
                        }
                        x += s + 2.0 * u;
                        x += o.text(&label, [x, base], px, TEXT) + 8.0 * u;
                        self.hits
                            .push(([start, y, x - start, row], Hit::Item(item.clone())));
                    }
                }
            } else if e.state == MachineState::NoPower {
                // Say how to fix it, not just that it's wrong.
                let text = if grid.is_none() {
                    tr("inspect.no_grid")
                } else {
                    tr("inspect.no_turbine")
                };
                let px = fit(o, text, px, r[2] - 2.0 * pad);
                o.text(text, [x, base], px, palette::STARVE);
            } else if let Some(rd) = recipe {
                // Each item opens the recipe browser.
                let stacks =
                    |hits: &mut Vec<(Rect, Hit)>, o: &mut Overlay, x: &mut f32, list: &[Stack]| {
                        for st in list {
                            let start = *x;
                            if let Some((sprite, t)) = view.look.item(&st.item) {
                                o.icon(sprite, [*x, y + (row - s) / 2.0], s, t);
                            }
                            *x += s + 2.0 * u;
                            *x += o.text(&format!("{}", st.count), [*x, base], px, TEXT) + 8.0 * u;
                            hits.push(([start, y, *x - start, row], Hit::Item(st.item.clone())));
                        }
                    };
                stacks(&mut self.hits, o, &mut x, &rd.inputs);
                x += o.text("→", [x, base], px, MUTED) + 8.0 * u;
                stacks(&mut self.hits, o, &mut x, &rd.outputs);
                o.text(
                    &format!(
                        "{} · {}",
                        rd.name,
                        trf("unit.seconds", &[&format!("{:.1}", rd.seconds)])
                    ),
                    [x + 4.0 * u, base],
                    px,
                    MUTED,
                );
            } else if recipes.is_some() {
                let text = tr("inspect.recipe_any");
                let px = fit(o, text, px, r[2] - 2.0 * pad);
                o.text(text, [x, base], px, MUTED);
            } else {
                let text = about(view, def, e);
                let px = fit(o, &text, px, r[2] - 2.0 * pad);
                o.text(&text, [x, base], px, MUTED);
            }
            y += row + 4.0 * u;
            let bw = r[2] - 2.0 * pad;
            o.panel([left, y], [bw, 6.0 * u], 3.0 * u, RAISED);
            if let (true, Some(f)) = (plumbing, &e.fluid) {
                let color = view.look.item_color(&f.item).unwrap_or(TEXT);
                o.panel(
                    [left, y],
                    [bw * f.fill.clamp(0.0, 1.0), 6.0 * u],
                    3.0 * u,
                    color,
                );
            } else if let Some(capacity) = warehouse {
                let total: u32 = e.held.iter().map(|h| h.1).sum();
                let fill = (total as f32 / capacity.max(1) as f32).clamp(0.0, 1.0);
                if fill > 0.0 {
                    o.panel([left, y], [bw * fill, 6.0 * u], 3.0 * u, palette::ACCENT);
                }
            } else if !hub && !wired && !plumbing && e.progress > 0.0 {
                let fill = state(e.state, view.look).1;
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
        if let Some(rs) = &recipes {
            // Pills fill upwards from a bottom edge; aim them at this band.
            let band = pill_rows * 46.0 * u;
            self.recipe_picker(
                o,
                view,
                rs,
                e.recipe.as_ref(),
                set,
                [left, y + band - 6.0 * u],
                r[2] - 2.0 * pad,
                true,
            );
            y += band;
        }

        if filter {
            let band = pill_rows * 46.0 * u;
            self.filter_picker(
                o,
                view,
                e,
                [left, y + band - 6.0 * u],
                r[2] - 2.0 * pad,
                true,
            );
            y += band;
        }

        if requester {
            let band = pill_rows * 46.0 * u;
            self.request_picker(
                o,
                view,
                e,
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
                (tr("button.rotate"), Hit::Turn(e.id, e.dir), TEXT),
                (tr("tool.remove"), Hit::Remove(e.id), palette::STARVE),
            ];
            for (i, (label, hit, ink)) in buttons.into_iter().enumerate() {
                let b = [left + i as f32 * (bw + 8.0 * u), y, bw, bh];
                o.panel([b[0], b[1]], [bw, bh], 10.0 * u, RAISED);
                let px = fit(o, label, 15.0 * u, bw - 12.0 * u);
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

/// Letters the keyboard already uses: M mutes, R rotates, T opens research,
/// X removes.
const RESERVED_KEYS: [char; 4] = ['m', 'r', 't', 'x'];

/// The buildings the number row picks (3 to 5), which need no letter.
pub const NUMBER_KEYED: [&str; 3] = ["drill", "furnace", "assembler"];

/// One key for every building but the hub, belts and [`NUMBER_KEYED`]
/// (which have number keys): the first letter of its name that is free, in
/// content order, then 8, 9 and 0, then the punctuation keys beside them.
pub fn hotkeys(content: &Content) -> Vec<(char, BuildingId)> {
    let mut out: Vec<(char, BuildingId)> = Vec::new();
    for b in &content.buildings {
        if matches!(b.role, BuildingRole::Hub | BuildingRole::Belt { .. })
            || NUMBER_KEYED.contains(&b.id.as_str())
        {
            continue;
        }
        let key = b
            .name
            .to_lowercase()
            .chars()
            .filter(char::is_ascii_lowercase)
            .chain('a'..='z')
            // Once the letters run out, the number keys the toolbar leaves,
            // then the keys beside them on the top three rows.
            .chain([
                '8', '9', '0', '-', '=', '[', ']', ';', ',', '.', '/', '\'', '\\', '`',
            ])
            .find(|c| !RESERVED_KEYS.contains(c) && !out.iter().any(|(k, _)| k == c));
        if let Some(k) = key {
            out.push((k, b.id.clone()));
        }
    }
    out
}

/// A pill: its label, icon, whether it is the one chosen, and what a tap does.
type Pill<'a> = (&'a str, Option<(&'a str, Rgba)>, bool, Hit);

/// The items a requester chest's sheet offers, and the counts.
const REQUEST_ITEMS: usize = 11;
const REQUEST_COUNTS: [u32; 4] = [10, 50, 100, 200];

/// The solid items in stock, most first, up to [`REQUEST_ITEMS`], and
/// `current` after them if it is not among them: what a requester chest or
/// filter splitter can be set to.
fn solid_items<'a>(
    view: &'a View,
    current: Option<&'a mofact_core::ids::ItemId>,
) -> Vec<&'a mofact_core::ids::ItemId> {
    let solid = |i: &mofact_core::ids::ItemId| {
        view.content
            .items
            .iter()
            .any(|d| &d.id == i && d.kind == mofact_core::content::ItemKind::Solid)
    };
    let mut items: Vec<(&mofact_core::ids::ItemId, u32)> = view
        .snapshot
        .stock
        .iter()
        .filter(|(i, n)| *n > 0 && solid(i))
        .map(|(i, n)| (i, *n))
        .collect();
    items.sort_by_key(|p| std::cmp::Reverse(p.1));
    items.truncate(REQUEST_ITEMS);
    let mut items: Vec<_> = items.into_iter().map(|(i, _)| i).collect();
    if let Some(i) = current {
        if !items.contains(&i) {
            items.push(i);
        }
    }
    items
}

/// The items a warehouse holds, most first, up to [`REQUEST_ITEMS`], and
/// `current` after them if it is not among them: what it can be set to
/// hand out.
fn held_items<'a>(
    e: &'a EntityView,
    current: Option<&'a mofact_core::ids::ItemId>,
) -> Vec<&'a mofact_core::ids::ItemId> {
    let mut held: Vec<_> = e.held.iter().filter(|h| h.1 > 0).collect();
    held.sort_by_key(|h| std::cmp::Reverse(h.1));
    held.truncate(REQUEST_ITEMS);
    let mut items: Vec<_> = held.into_iter().map(|(i, _)| i).collect();
    if let Some(i) = current {
        if !items.contains(&i) {
            items.push(i);
        }
    }
    items
}

/// One line on what a building does, for the inspect sheet, when it has no
/// recipe, fluid or power figures to show instead.
fn about(view: &View, def: Option<&BuildingDef>, e: &EntityView) -> String {
    let name = |id: &mofact_core::ids::ItemId| {
        view.content
            .items
            .iter()
            .find(|i| &i.id == id)
            .map_or(id.as_str().to_owned(), |i| i18n::in_sentence(&i.name))
    };
    match def.map(|d| &d.role) {
        Some(BuildingRole::Miner { .. }) => match (&e.ore, e.ore_left) {
            (_, Some(0)) => tr("inspect.mined_out").into(),
            (Some(ore), Some(left)) => trf("inspect.mines_left", &[&name(ore), &left]),
            (Some(ore), None) => trf("inspect.mines", &[&name(ore)]),
            (None, _) => tr("inspect.drill").into(),
        },
        Some(BuildingRole::Lab { .. }) => {
            let research = &view.snapshot.research;
            match research
                .current
                .as_ref()
                .and_then(|c| view.content.research.iter().find(|r| &r.id == c))
            {
                Some(r) => trf(
                    "inspect.lab",
                    &[&r.name, &research.progress.0, &research.progress.1],
                ),
                None => tr("inspect.lab_idle").into(),
            }
        }
        Some(BuildingRole::Splitter { mode }) => match mode {
            SplitMode::Even => tr("inspect.splitter").into(),
            SplitMode::ThreeWay => tr("inspect.three_way").into(),
            SplitMode::Priority => tr("inspect.priority").into(),
            SplitMode::Filter => match &e.filter {
                Some(item) => trf("inspect.filter", &[&name(item)]),
                None => tr("inspect.filter_none").into(),
            },
        },
        Some(BuildingRole::Underground { reach, .. }) => trf("inspect.underground", &[reach]),
        Some(BuildingRole::Rail) => tr("inspect.rail").into(),
        Some(BuildingRole::RailSignal) => tr("inspect.signal").into(),
        Some(BuildingRole::TrainStop) => tr("inspect.train_stop").into(),
        Some(BuildingRole::Locomotive { .. }) => tr("inspect.locomotive").into(),
        Some(BuildingRole::DronePort { drones, reach }) => {
            trf("inspect.port", &[drones, &(u32::from(*reach) * 2)])
        }
        Some(BuildingRole::ProviderChest { .. }) => tr("inspect.provider").into(),
        Some(BuildingRole::RequesterChest { .. }) => match &e.request {
            Some((item, n)) => trf("inspect.requests", &[n, &name(item)]),
            None => tr("inspect.requester").into(),
        },
        _ => String::new(),
    }
}

/// A machine state's label and colour, in `look`'s colours (T58).
fn state(s: MachineState, look: &Look) -> (&'static str, Rgba) {
    use palette::Status;
    match s {
        MachineState::Idle => (tr("state.idle"), MUTED),
        MachineState::Working => (tr("state.working"), look.status(Status::Working)),
        MachineState::Starved => (tr("state.starved"), look.status(Status::Starved)),
        MachineState::Blocked => (tr("state.blocked"), look.status(Status::Blocked)),
        MachineState::NoPower => (tr("state.no_power"), look.status(Status::Starved)),
    }
}

/// The tip for the tool in hand, from the prototype.
fn tip(tool: &Tool) -> &'static str {
    tr(match tool {
        Tool::Hand => "tip.hand",
        Tool::Belt => "tip.belt",
        Tool::Remove => "tip.remove",
        Tool::Select => "tip.select",
        Tool::Paste => "tip.paste",
        Tool::Place(b) => match b.as_str() {
            "drill" => "tip.drill",
            "furnace" => "tip.furnace",
            "lab" => "tip.lab",
            "pole" => "tip.pole",
            "splitter" => "tip.splitter",
            "three-way-splitter" => "tip.three_way_splitter",
            "priority-splitter" => "tip.priority_splitter",
            "filter-splitter" => "tip.filter_splitter",
            "underground-belt" => "tip.underground_belt",
            "rail" => "tip.rail",
            "rail-signal" => "tip.rail_signal",
            "train-stop" => "tip.train_stop",
            "locomotive" => "tip.locomotive",
            "tank-train" => "tip.tank_train",
            "wind-turbine" => "tip.wind_turbine",
            "radar" => "tip.radar",
            "drone-port" => "tip.drone_port",
            "provider-chest" => "tip.provider_chest",
            "requester-chest" => "tip.requester_chest",
            "warehouse" => "tip.warehouse",
            _ => "tip.place",
        },
    })
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
/// A card of words: a tip or a toast. Returns the rect it covers.
fn chip(o: &mut Overlay, text: &str, at: [f32; 2], width: f32, u: f32, ink: Rgba) -> Rect {
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
    [at[0], at[1], w, h]
}

/// Break `text` into lines no wider than `width` at `px`: at spaces, and
/// between Han characters and kana, which are written without spaces.
fn wrap(o: &Overlay, text: &str, px: f32, width: f32) -> Vec<String> {
    let mut lines = vec![String::new()];
    for (word, spaced) in i18n::breaks(text) {
        let last = lines.last_mut().expect("one line");
        let candidate = if last.is_empty() {
            word.to_string()
        } else if spaced {
            format!("{last} {word}")
        } else {
            format!("{last}{word}")
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

pub mod i18n;
pub mod plan;
pub mod problems;
mod train;
use problems::Problem;
pub use train::add_stop;
