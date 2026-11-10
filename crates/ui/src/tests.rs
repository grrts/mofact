use super::*;
use mofact_core::geom::TilePos;
use std::path::Path;

fn content() -> Content {
    mofact_content::load_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data"))
        .expect("the game data loads")
}

fn snapshot() -> Snapshot {
    let entity = |id, building: &str, x| EntityView {
        request: None,
        filter: None,
        held: Vec::new(),
        keep: 0,
        when: None,
        id: EntityId(id),
        building: building.into(),
        pos: TilePos::new(x, 0),
        dir: Dir::East,
        recipe: None,
        ore: None,
        ore_left: None,
        grid: None,
        exit: false,
        fluid: None,
        links: 0,
        state: MachineState::Starved,
        progress: 0.4,
    };
    Snapshot {
        tick: 1,
        entities: vec![
            entity(1, "hub", 0),
            entity(2, "assembler", 4),
            entity(3, "belt", 5),
        ],
        belt_items: Vec::new(),
        stock: vec![("copper-plate".into(), 2), ("iron-plate".into(), 30)],
        research: Default::default(),
        grids: Vec::new(),
        goal: None,
        order: None,
        orders_shipped: 0,
        orders_missed: 0,
        cleared: Default::default(),
        depleted: Default::default(),
        pins: Default::default(),
        trains: vec![train()],
        mega: None,
        explored: Default::default(),
        drones: Vec::new(),
        ghosts: Vec::new(),
        marked: Vec::new(),
    }
}

/// A train part loaded, bound for the second of three stops.
fn train() -> mofact_core::snapshot::TrainView {
    use mofact_core::command::TrainStop;
    let stop = |x, load| TrainStop {
        at: TilePos::new(x, 9),
        load,
    };
    mofact_core::snapshot::TrainView {
        id: EntityId(10),
        building: "locomotive".into(),
        pos: TilePos::new(3, 9),
        dir: Dir::East,
        along: 0.5,
        cargo: vec![("iron-ore".into(), 120)],
        capacity: 200,
        schedule: vec![stop(0, true), stop(20, false), stop(40, false)],
        next: 1,
        state: mofact_core::snapshot::TrainState::Moving,
    }
}

struct Fixture {
    content: Content,
    look: Look,
    snapshot: Snapshot,
    overlay: Overlay,
    hud: Hud,
    belt: BuildingId,
    slots: Vec<SlotView>,
    store: StoreView,
    crash: bool,
    selection: usize,
    blueprints: Vec<mofact_input::Blueprint>,
    home: Option<[f32; 2]>,
}

impl Fixture {
    fn new() -> Self {
        let content = content();
        let look = Look::new(&content);
        Self {
            overlay: Overlay::new(&look),
            look,
            content,
            snapshot: snapshot(),
            hud: Hud::new(),
            belt: "belt".into(),
            slots: vec![
                SlotView {
                    used: true,
                    current: true,
                    title: "Standard · seed 42".into(),
                    detail: "1 h 2 min played · 120 built".into(),
                },
                SlotView {
                    used: true,
                    current: false,
                    title: "Lakes · seed 123456789".into(),
                    detail: "just started · 14 built".into(),
                },
                SlotView::default(),
            ],
            store: StoreView::default(),
            crash: false,
            selection: 0,
            blueprints: Vec::new(),
            home: None,
        }
    }

    fn draw(&mut self, size: (u32, u32), scale: f32, tool: &Tool) {
        let view = View {
            size,
            scale,
            content: &self.content,
            look: &self.look,
            snapshot: &self.snapshot,
            tool,
            dir: Dir::East,
            recipe: None,
            belt: &self.belt,
            history: (true, false),
            safe: [0.0; 4],
            nudge: None,
            slots: &self.slots,
            store: &self.store,
            crash: self.crash,
            selection: self.selection,
            blueprints: &self.blueprints,
            home: self.home,
        };
        self.overlay.clear();
        self.hud.draw(&view, &mut self.overlay);
    }

    fn find(&self, want: &Hit) -> Rect {
        self.hud
            .hits
            .iter()
            .find(|(_, h)| h == want)
            .map(|(r, _)| *r)
            .unwrap_or_else(|| panic!("no {want:?} on screen"))
    }

    fn tap(&mut self, [x, y]: [f32; 2]) -> (bool, Vec<Action>) {
        let (down, _) = self.hud.pointer(Pointer::Down { id: 7, x, y });
        let (up, actions) = self.hud.pointer(Pointer::Up { id: 7 });
        assert_eq!(down, up, "a tap is taken whole or not at all");
        (up, actions)
    }
}

fn centre(r: Rect) -> [f32; 2] {
    [r[0] + r[2] / 2.0, r[1] + r[3] / 2.0]
}

#[test]
fn tapping_a_toolbar_button_picks_its_tool() {
    let mut f = Fixture::new();
    f.draw((1280, 720), 1.0, &Tool::Hand);
    let drill = f.find(&Hit::Tool(2));
    assert_eq!(
        f.tap(centre(drill)),
        (true, vec![Action::Tool(Tool::Place("drill".into()))])
    );
}

#[test]
fn sliding_off_a_button_cancels_it() {
    let mut f = Fixture::new();
    f.draw((1280, 720), 1.0, &Tool::Hand);
    let [x, y] = centre(f.find(&Hit::Tool(1)));
    assert!(f.hud.pointer(Pointer::Down { id: 1, x, y }).0);
    assert!(
        f.hud
            .pointer(Pointer::Move {
                id: 1,
                x: 300.0,
                y: 300.0
            })
            .0
    );
    assert_eq!(f.hud.pointer(Pointer::Up { id: 1 }), (true, vec![]));
}

#[test]
fn a_tap_on_the_world_inspects_but_a_drag_or_pinch_does_not() {
    let mut f = Fixture::new();
    f.draw((1280, 720), 1.0, &Tool::Hand);
    assert_eq!(
        f.tap([400.0, 400.0]),
        (false, vec![Action::Inspect([400.0, 400.0])])
    );

    f.hud.pointer(Pointer::Down {
        id: 1,
        x: 400.0,
        y: 400.0,
    });
    f.hud.pointer(Pointer::Move {
        id: 1,
        x: 460.0,
        y: 400.0,
    });
    assert_eq!(f.hud.pointer(Pointer::Up { id: 1 }), (false, vec![]));

    f.hud.pointer(Pointer::Down {
        id: 1,
        x: 400.0,
        y: 400.0,
    });
    f.hud.pointer(Pointer::Down {
        id: 2,
        x: 500.0,
        y: 400.0,
    });
    assert_eq!(f.hud.pointer(Pointer::Up { id: 1 }), (false, vec![]));
    assert_eq!(f.hud.pointer(Pointer::Up { id: 2 }), (false, vec![]));
}

#[test]
fn placing_a_crafter_offers_its_recipes_and_the_rotate_button() {
    let mut f = Fixture::new();
    f.draw((1280, 720), 1.0, &Tool::Place("assembler".into()));
    let gear = f.find(&Hit::Recipe(Some("gear".into())));
    f.find(&Hit::Recipe(None));
    f.find(&Hit::Recipe(Some("circuit".into())));
    assert_eq!(
        f.tap(centre(gear)),
        (true, vec![Action::Recipe(Some("gear".into()))])
    );
    let rotate = f.find(&Hit::Rotate);
    assert_eq!(f.tap(centre(rotate)), (true, vec![Action::Rotate]));

    // Belts turn too, but have no recipes.
    f.draw((1280, 720), 1.0, &Tool::Belt);
    f.find(&Hit::Rotate);
    assert!(!f.hud.hits.iter().any(|(_, h)| matches!(h, Hit::Recipe(_))));
}

#[test]
fn the_inspect_sheet_sets_recipes_turns_and_removes() {
    let mut f = Fixture::new();
    f.hud.select(Some(EntityId(2)));
    f.draw((1280, 720), 1.0, &Tool::Hand);
    let set = f.find(&Hit::SetRecipe(EntityId(2), Some("wire".into())));
    assert_eq!(
        f.tap(centre(set)).1,
        vec![Action::Send(Command::SetRecipe {
            entity: EntityId(2),
            recipe: Some("wire".into())
        })]
    );
    let turn = f.find(&Hit::Turn(EntityId(2), Dir::East));
    assert_eq!(
        f.tap(centre(turn)).1,
        vec![Action::Send(Command::Rotate {
            entity: EntityId(2),
            dir: Dir::South
        })]
    );
    let remove = f.find(&Hit::Remove(EntityId(2)));
    assert_eq!(
        f.tap(centre(remove)).1,
        vec![Action::Send(Command::Remove {
            entity: EntityId(2)
        })]
    );
    assert_eq!(f.hud.selected(), None);
}

#[test]
fn the_inspect_sheet_takes_another_like_it_in_hand() {
    let mut f = Fixture::new();
    f.snapshot.entities[1].recipe = Some("gear".into());
    f.snapshot.entities[1].dir = Dir::South;
    f.hud.select(Some(EntityId(2)));
    f.draw((1179, 2556), 3.0, &Tool::Hand);
    let another = f.find(&Hit::Pipette(
        "assembler".into(),
        Dir::South,
        Some("gear".into()),
    ));
    assert_eq!(
        f.tap(centre(another)).1,
        vec![Action::Pipette {
            building: "assembler".into(),
            dir: Dir::South,
            recipe: Some("gear".into()),
        }]
    );
    assert_eq!(f.hud.selected(), None, "the sheet makes way for building");

    // There is only one hub.
    f.hud.select(Some(EntityId(1)));
    f.draw((1179, 2556), 3.0, &Tool::Hand);
    assert!(!f
        .hud
        .hits
        .iter()
        .any(|(_, h)| matches!(h, Hit::Pipette(..))));
}

#[test]
fn the_train_sheet_sets_up_a_schedule_by_touch() {
    use mofact_core::command::TrainStop;
    let mut f = Fixture::new();
    f.hud.select(Some(EntityId(10)));
    f.draw((1179, 2556), 3.0, &Tool::Hand);
    let stops = train().schedule;
    // Tapping a stop's pill switches it between loading and unloading.
    let mut flipped = stops.clone();
    flipped[1].load = true;
    let pill = f.find(&Hit::Schedule(EntityId(10), flipped.clone()));
    assert_eq!(
        f.tap(centre(pill)).1,
        vec![Action::Send(Command::SetSchedule {
            entity: EntityId(10),
            stops: flipped
        })]
    );
    // × drops it.
    let mut without = stops.clone();
    without.remove(2);
    let x = f.find(&Hit::Schedule(EntityId(10), without.clone()));
    assert_eq!(
        f.tap(centre(x)).1,
        vec![Action::Send(Command::SetSchedule {
            entity: EntityId(10),
            stops: without
        })]
    );
    // Add stop waits for a stop to be tapped on the map; a second tap gives up.
    let add = f.find(&Hit::PickStop(EntityId(10)));
    f.tap(centre(add));
    assert_eq!(f.hud.picking_stop(), Some(EntityId(10)));
    f.draw((1179, 2556), 3.0, &Tool::Hand);
    let cancel = f.find(&Hit::PickStop(EntityId(10)));
    f.tap(centre(cancel));
    assert_eq!(f.hud.picking_stop(), None);
    // A new stop loads after one that unloads.
    let added = add_stop(&stops, TilePos::new(7, 7));
    assert_eq!(
        added.last(),
        Some(&TrainStop {
            at: TilePos::new(7, 7),
            load: true
        })
    );
    // Closing the sheet drops the pick too.
    f.hud.picking = Some(EntityId(10));
    f.hud.select(None);
    f.draw((1179, 2556), 3.0, &Tool::Hand);
    assert_eq!(f.hud.picking_stop(), None);
    // Remove takes the train away.
    f.hud.select(Some(EntityId(10)));
    f.draw((1179, 2556), 3.0, &Tool::Hand);
    let remove = f.find(&Hit::Remove(EntityId(10)));
    assert_eq!(
        f.tap(centre(remove)).1,
        vec![Action::Send(Command::Remove {
            entity: EntityId(10)
        })]
    );
}

#[test]
fn the_hub_sheet_cannot_remove_the_hub_and_closes() {
    let mut f = Fixture::new();
    f.hud.select(Some(EntityId(1)));
    f.draw((1280, 720), 1.0, &Tool::Hand);
    assert!(!f
        .hud
        .hits
        .iter()
        .any(|(_, h)| matches!(h, Hit::Remove(_) | Hit::Turn(..))));
    let close = f.find(&Hit::Close);
    f.tap(centre(close));
    assert_eq!(f.hud.selected(), None);
}

#[test]
fn a_sheet_for_something_removed_closes_itself() {
    let mut f = Fixture::new();
    f.hud.select(Some(EntityId(99)));
    f.draw((1280, 720), 1.0, &Tool::Hand);
    assert_eq!(f.hud.selected(), None);
}

#[test]
fn the_toolbar_sits_under_the_thumb_in_both_orientations() {
    let mut f = Fixture::new();
    f.draw((1080, 2340), 3.0, &Tool::Hand);
    let first = f.find(&Hit::Tool(0));
    let last = f.find(&Hit::Tool(REMOVE));
    assert!(first[1] > 2000.0, "portrait: along the bottom");
    assert_eq!(first[1], last[1]);
    assert!(last[0] > first[0]);

    f.draw((2340, 1080), 3.0, &Tool::Hand);
    let first = f.find(&Hit::Tool(0));
    let last = f.find(&Hit::Tool(REMOVE));
    assert!(first[0] > 2000.0, "landscape: on the right edge");
    assert_eq!(first[0], last[0]);
    assert!(last[1] > first[1]);
}

#[test]
fn everything_stays_on_screen() {
    let sizes = [
        ((1280, 720), 1.0),
        ((720, 1280), 1.0),
        ((1080, 2340), 3.0),
        ((2340, 1080), 3.0),
        ((750, 1334), 2.0),
        ((640, 480), 1.0),
    ];
    let tools = [
        Tool::Hand,
        Tool::Belt,
        Tool::Place("assembler".into()),
        Tool::Remove,
    ];
    for (size, scale) in sizes {
        for tool in &tools {
            for (selected, research, more, radial) in [
                (None, false, false, None),
                (Some(EntityId(1)), false, false, None),
                (Some(EntityId(2)), false, false, None),
                (None, true, false, None),
                (None, false, true, None),
                (None, false, false, Some([0.0, 0.0])),
                (None, false, false, Some([4000.0, 4000.0])),
                (None, false, false, None),
            ] {
                let mut f = Fixture::new();
                f.hud.select(selected);
                f.hud.research = research;
                f.hud.more = more;
                f.hud.radial = radial;
                if selected.is_none() && !research && !more && radial.is_none() {
                    f.hud.browse = Some("iron-plate".into());
                }
                f.draw(size, scale, tool);
                let (w, h) = (size.0 as f32, size.1 as f32);
                for (r, hit) in &f.hud.hits {
                    assert!(
                        r[0] >= 0.0
                            && r[1] >= 0.0
                            && r[0] + r[2] <= w + 0.5
                            && r[1] + r[3] <= h + 0.5,
                        "{hit:?} at {r:?} is off a {size:?} screen with {tool:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn unaffordable_tools_say_so_in_red() {
    let red = |f: &Fixture| {
        f.overlay
            .instances
            .iter()
            .filter(|i| i.tint == palette::STARVE)
            .count()
    };
    let mut f = Fixture::new();
    f.snapshot.stock = vec![("copper-plate".into(), 10), ("iron-plate".into(), 30)];
    f.draw((1280, 720), 1.0, &Tool::Hand);
    let rich = red(&f);
    // The assembler needs copper too: its cost turns to the warning colour.
    f.snapshot.stock = vec![("iron-plate".into(), 30)];
    f.draw((1280, 720), 1.0, &Tool::Hand);
    assert!(red(&f) > rich);
}

#[test]
fn the_research_sheet_picks_what_labs_study() {
    let mut f = Fixture::new();
    f.draw((1080, 2340), 3.0, &Tool::Hand);
    assert!(
        !f.hud.hits.iter().any(|(_, h)| *h == Hit::Research),
        "no research button before there is a lab"
    );
    let mut lab = f.snapshot.entities[1].clone();
    lab.id = EntityId(90);
    lab.building = "lab".into();
    lab.pos = TilePos::new(20, 20);
    f.snapshot.entities.push(lab);
    f.draw((1080, 2340), 3.0, &Tool::Hand);
    let button = f.find(&Hit::Research);
    assert_eq!(f.tap(centre(button)), (true, vec![]));
    assert!(f.hud.researching());
    f.draw((1080, 2340), 3.0, &Tool::Hand);
    // Steel working is ready; motors need it first, so they can't be tapped.
    let steel = f.find(&Hit::Study(Some("steel-working".into())));
    assert!(!f
        .hud
        .hits
        .iter()
        .any(|(_, h)| *h == Hit::Study(Some("motors".into()))));
    assert_eq!(
        f.tap(centre(steel)).1,
        vec![Action::Send(Command::SetResearch {
            research: Some("steel-working".into())
        })]
    );

    // Once it is under way, tapping it again stops the labs; once done,
    // motors open up.
    f.snapshot.research.current = Some("steel-working".into());
    f.snapshot.research.progress = (3, 15);
    f.draw((1080, 2340), 3.0, &Tool::Hand);
    f.find(&Hit::Study(None));
    f.snapshot.research = mofact_core::snapshot::ResearchView {
        current: None,
        progress: (0, 0),
        done: vec!["steel-working".into()],
        ..Default::default()
    };
    f.draw((1080, 2340), 3.0, &Tool::Hand);
    f.find(&Hit::Study(Some("motors".into())));
    let close = f.find(&Hit::Close);
    f.tap(centre(close));
    assert!(!f.hud.researching());
}

#[test]
fn locked_recipes_stay_off_the_pills_until_researched() {
    let mut f = Fixture::new();
    let motor = Hit::Recipe(Some("motor".into()));
    f.draw((1280, 720), 1.0, &Tool::Place("assembler".into()));
    assert!(!f.hud.hits.iter().any(|(_, h)| *h == motor));
    f.snapshot.research.done = vec!["steel-working".into(), "motors".into()];
    f.draw((1280, 720), 1.0, &Tool::Place("assembler".into()));
    f.find(&motor);
}

#[test]
fn the_more_slot_opens_every_other_building() {
    let mut f = Fixture::new();
    f.draw((1080, 2340), 3.0, &Tool::Hand);
    let more = f.find(&Hit::Tool(MORE));
    assert_eq!(f.tap(centre(more)), (true, vec![]));
    f.draw((1080, 2340), 3.0, &Tool::Hand);
    for b in ["lab", "pole", "wind-turbine"] {
        f.find(&Hit::Build(b.into()));
    }
    // What the toolbar already has, and belts, stay off the sheet.
    assert!(!f.hud.hits.iter().any(|(_, h)| matches!(h,
        Hit::Build(b) if ["drill", "belt", "fast-belt", "hub"].contains(&b.as_str()))));
    let pole = f.find(&Hit::Build("pole".into()));
    assert_eq!(
        f.tap(centre(pole)),
        (true, vec![Action::Tool(Tool::Place("pole".into()))])
    );
    // The sheet closes, and the More slot now stands for the pole.
    f.draw((1080, 2340), 3.0, &Tool::Place("pole".into()));
    assert!(!f.hud.hits.iter().any(|(_, h)| matches!(h, Hit::Build(_))));
    assert_eq!(tool_index(&Tool::Place("pole".into())), Some(MORE));
}

#[test]
fn a_pole_sheet_shows_its_grid() {
    let mut f = Fixture::new();
    f.snapshot.entities.push(EntityView {
        request: None,
        filter: None,
        held: Vec::new(),
        keep: 0,
        when: None,
        id: EntityId(9),
        building: "pole".into(),
        pos: TilePos::new(9, 0),
        dir: Dir::East,
        recipe: None,
        ore: None,
        ore_left: None,
        grid: Some(0),
        exit: false,
        fluid: None,
        links: 0,
        state: MachineState::Working,
        progress: 0.0,
    });
    f.snapshot.grids = vec![mofact_core::snapshot::GridView {
        supply_kw: 60.0,
        demand_kw: 120.0,
        satisfaction: 0.5,
    }];
    f.hud.select(Some(EntityId(9)));
    f.draw((1280, 720), 1.0, &Tool::Hand);
    f.find(&Hit::Remove(EntityId(9)));
}

#[test]
fn the_belt_tool_offers_researched_tiers() {
    let mut f = Fixture::new();
    f.draw((1080, 2340), 3.0, &Tool::Belt);
    assert!(
        !f.hud.hits.iter().any(|(_, h)| matches!(h, Hit::Belt(_))),
        "one tier: nothing to pick"
    );
    f.snapshot.research.done = vec!["fast-belts".into()];
    f.draw((1080, 2340), 3.0, &Tool::Belt);
    f.find(&Hit::Belt("belt".into()));
    assert!(!f
        .hud
        .hits
        .iter()
        .any(|(_, h)| *h == Hit::Belt("express-belt".into())));
    let fast = f.find(&Hit::Belt("fast-belt".into()));
    assert_eq!(
        f.tap(centre(fast)),
        (true, vec![Action::Belt("fast-belt".into())])
    );
}

#[test]
fn every_building_is_one_key_away() {
    let f = Fixture::new();
    let keys = hotkeys(&f.content);
    let others: Vec<_> = f
        .content
        .buildings
        .iter()
        .filter(|b| {
            !matches!(b.role, BuildingRole::Hub | BuildingRole::Belt { .. })
                && !NUMBER_KEYED.contains(&b.id.as_str())
        })
        .collect();
    assert_eq!(keys.len(), others.len());
    for (i, (k, b)) in keys.iter().enumerate() {
        assert!(!RESERVED_KEYS.contains(k), "{b} took {k}");
        assert!(keys[i + 1..].iter().all(|(o, _)| o != k), "{k} twice");
    }
    assert!(keys.contains(&('l', "lab".into())));
    assert!(keys.iter().any(|(_, b)| b.as_str() == "warehouse"));
}

#[test]
fn a_long_press_opens_the_radial_menu_and_any_building_is_two_taps_away() {
    let mut f = Fixture::new();
    f.snapshot.research.done = vec!["logistics".into()];
    f.draw((1080, 2340), 3.0, &Tool::Hand);
    // A finger held still on the world.
    let (taken, _) = f.hud.pointer(Pointer::Down {
        id: 3,
        x: 500.0,
        y: 900.0,
    });
    assert!(!taken);
    assert!(f.hud.poll(Instant::now()).is_empty(), "not yet");
    assert_eq!(
        f.hud.poll(Instant::now() + LONG_PRESS),
        vec![Action::Cancel]
    );
    assert!(f.hud.radial_open());
    // Lifting it is no tap.
    assert_eq!(f.hud.pointer(Pointer::Up { id: 3 }), (false, vec![]));
    f.draw((1080, 2340), 3.0, &Tool::Hand);
    for b in [
        "drill",
        "furnace",
        "assembler",
        "lab",
        "pole",
        "wind-turbine",
        "splitter",
    ] {
        f.find(&Hit::Build(b.into()));
    }
    let splitter = f.find(&Hit::Build("splitter".into()));
    assert_eq!(
        f.tap(centre(splitter)),
        (true, vec![Action::Tool(Tool::Place("splitter".into()))])
    );
    assert!(!f.hud.radial_open());

    // A tap anywhere else closes it; a drag never opens it.
    f.hud.radial = Some([10.0, 10.0]);
    f.draw((1080, 2340), 3.0, &Tool::Hand);
    f.tap([5.0, 2300.0]);
    assert!(!f.hud.radial_open());
    f.hud.pointer(Pointer::Down {
        id: 4,
        x: 500.0,
        y: 900.0,
    });
    f.hud.pointer(Pointer::Move {
        id: 4,
        x: 600.0,
        y: 900.0,
    });
    assert!(f.hud.poll(Instant::now() + LONG_PRESS).is_empty());
    f.hud.pointer(Pointer::Up { id: 4 });

    // With belts or remove in hand, a finger held still starts a drag, so
    // no menu opens over it; with a building in hand, it does.
    for (tool, opens) in [
        (Tool::Belt, false),
        (Tool::Remove, false),
        (Tool::Place("drill".into()), true),
    ] {
        f.hud.radial = None;
        f.draw((1080, 2340), 3.0, &tool);
        f.hud.pointer(Pointer::Down {
            id: 5,
            x: 500.0,
            y: 900.0,
        });
        f.hud.poll(Instant::now() + LONG_PRESS);
        assert_eq!(f.hud.radial_open(), opens, "{tool:?}");
        f.hud.pointer(Pointer::Up { id: 5 });
    }
}

#[test]
fn any_item_icon_opens_its_recipes_in_one_tap() {
    let mut f = Fixture::new();
    f.draw((1080, 2340), 3.0, &Tool::Hand);
    // The stock along the top.
    let plates = f.find(&Hit::Item("iron-plate".into()));
    assert_eq!(f.tap(centre(plates)), (true, vec![]));
    f.draw((1080, 2340), 3.0, &Tool::Hand);
    // Iron plates go into gears; the gear's icon leads on.
    let gear = f.find(&Hit::Item("gear".into()));
    f.find(&Hit::Item("iron-ore".into()));
    let links = |f: &Fixture, item: &str| {
        let want = Hit::Item(item.into());
        f.hud.hits.iter().filter(|(_, h)| *h == want).count()
    };
    assert_eq!(
        links(&f, "iron-plate"),
        1,
        "only the stock chip links to itself"
    );
    f.tap(centre(gear));
    f.draw((1080, 2340), 3.0, &Tool::Hand);
    f.find(&Hit::Item("iron-plate".into()));
    let close = f.find(&Hit::Close);
    f.tap(centre(close));
    f.draw((1080, 2340), 3.0, &Tool::Hand);
    assert_eq!(links(&f, "iron-ore"), 0);

    // A crafter's recipe line in the inspect sheet links its items too.
    f.snapshot.entities[1].recipe = Some("gear".into());
    f.hud.select(Some(EntityId(2)));
    f.draw((1080, 2340), 3.0, &Tool::Hand);
    f.find(&Hit::Item("gear".into()));
}

#[test]
fn the_problems_list_takes_the_camera_to_each_problem_in_turn() {
    use mofact_core::snapshot::GridView;
    let mut f = Fixture::new();
    let at = |id, building: &str, x, y, state| EntityView {
        request: None,
        filter: None,
        held: Vec::new(),
        keep: 0,
        when: None,
        id: EntityId(id),
        building: building.into(),
        pos: TilePos::new(x, y),
        dir: Dir::East,
        recipe: None,
        ore: None,
        ore_left: None,
        grid: None,
        exit: false,
        fluid: None,
        links: 0,
        state,
        progress: 0.0,
    };
    f.snapshot.entities = vec![
        at(1, "hub", 0, 0, MachineState::Working),
        at(2, "furnace", 10, 4, MachineState::Starved),
        at(3, "furnace", 20, 4, MachineState::Starved),
        at(4, "pipe", 7, 7, MachineState::Working),
        EntityView {
            grid: Some(0),
            ..at(5, "lab", 30, 2, MachineState::Working)
        },
        at(6, "belt", 3, 3, MachineState::Starved),
    ];
    f.snapshot.grids = vec![GridView {
        supply_kw: 60.0,
        demand_kw: 120.0,
        satisfaction: 0.5,
    }];
    f.draw((1080, 2340), 3.0, &Tool::Hand);
    let lines: Vec<_> = f
        .hud
        .found
        .iter()
        .map(|p| (p.trouble, p.at.len()))
        .collect();
    use problems::Trouble as T;
    assert_eq!(
        lines,
        vec![(T::ShortPower, 1), (T::Starved, 2), (T::EmptyPipe, 1)],
        "worst first; belts never count"
    );

    // The button opens the list; a line goes to the first of its places
    // and shows it in the inspect sheet.
    f.tap(centre(f.find(&Hit::Problems)));
    f.draw((1080, 2340), 3.0, &Tool::Hand);
    let starved = f.find(&Hit::Problem(1));
    assert_eq!(
        f.tap(centre(starved)),
        (true, vec![Action::Focus([10.5, 4.5])])
    );
    assert_eq!(f.hud.selected(), Some(EntityId(2)));
    // The same line again goes on to the next one.
    f.draw((1080, 2340), 3.0, &Tool::Hand);
    f.tap(centre(f.find(&Hit::Problems)));
    f.draw((1080, 2340), 3.0, &Tool::Hand);
    let starved = f.find(&Hit::Problem(1));
    assert_eq!(
        f.tap(centre(starved)),
        (true, vec![Action::Focus([20.5, 4.5])])
    );
    assert_eq!(f.hud.selected(), Some(EntityId(3)));

    // Nothing stuck, no button.
    f.snapshot.entities.truncate(1);
    f.snapshot.grids.clear();
    f.draw((1080, 2340), 3.0, &Tool::Hand);
    assert!(!f.hud.hits.iter().any(|(_, h)| *h == Hit::Problems));
}

#[test]
fn the_goal_card_shows_how_far_along_and_says_when_a_goal_is_met() {
    use mofact_core::snapshot::GoalView;
    let mut f = Fixture::new();
    f.snapshot.goal = Some(GoalView {
        index: 0,
        have: 12,
        need: 50,
    });
    f.draw((1080, 2340), 3.0, &Tool::Hand);
    let card = f.find(&Hit::Goal);
    assert!(f.hud.hint, "a new goal says how to do it");
    // A tap folds the hint away, another brings it back.
    f.tap(centre(card));
    assert!(!f.hud.hint);
    f.tap(centre(card));
    assert!(f.hud.hint);
    f.tap(centre(card));

    // The next goal: a toast, and its hint shows again.
    f.snapshot.goal = Some(GoalView {
        index: 1,
        have: 1,
        need: 3,
    });
    f.draw((1080, 2340), 3.0, &Tool::Hand);
    let (text, _, _) = f.hud.toast.clone().expect("a toast");
    assert_eq!(text, "Goal met! Next: Have 3 drills");
    assert!(f.hud.hint);

    // Read, the hint folds itself away so the header stays small.
    f.hud.hint_opened = Instant::now().checked_sub(GOAL_HINT_FOR);
    f.draw((1080, 2340), 3.0, &Tool::Hand);
    assert!(!f.hud.hint, "the hint makes room once read");
    f.tap(centre(f.find(&Hit::Goal)));
    assert!(f.hud.hint, "and a tap brings it back");

    // All done: no card.
    f.snapshot.goal = None;
    f.draw((1080, 2340), 3.0, &Tool::Hand);
    assert!(!f.hud.hits.iter().any(|(_, h)| *h == Hit::Goal));
}

#[test]
fn the_sound_button_turns_sound_off_and_on() {
    let mut f = Fixture::new();
    f.draw((1080, 2340), 3.0, &Tool::Hand);
    let button = f.find(&Hit::Sound);
    // A full 44-point target, at whatever scale the toolbar left.
    let pt = 44.0 * f.hud.unit - 0.01;
    assert!(button[2] >= pt && button[3] >= pt, "{button:?}");
    assert_eq!(f.tap(centre(button)), (true, vec![Action::Mute(true)]));
    assert!(f.hud.muted());
    f.draw((1080, 2340), 3.0, &Tool::Hand);
    assert_eq!(
        f.tap(centre(f.find(&Hit::Sound))),
        (true, vec![Action::Mute(false)])
    );
    assert!(!f.hud.muted());
}

/// The screens the HUD is checked on (T40): a small phone, a large phone
/// and a tablet, each held both ways, at their own pixel densities.
const SCREENS: [((u32, u32), f32); 6] = [
    ((640, 1136), 2.0),
    ((1136, 640), 2.0),
    ((1179, 2556), 3.0),
    ((2556, 1179), 3.0),
    ((2048, 2732), 2.0),
    ((2732, 2048), 2.0),
];

/// Every state of the HUD worth a look: each tool, each sheet.
fn every_state(mut check: impl FnMut(&mut Fixture, (u32, u32), f32, &Tool, &str)) {
    let tools = [
        Tool::Hand,
        Tool::Belt,
        Tool::Place("assembler".into()),
        Tool::Remove,
        Tool::Select,
    ];
    for (size, scale) in SCREENS {
        for tool in &tools {
            for state in [
                "plain",
                "inspect",
                "research",
                "build",
                "radial",
                "recipes",
                "problems",
                "train",
                "train-pick",
                "menu",
                "menu-delete",
                "new-game",
                "settings",
                "unlock",
                "selection",
                "library",
                "requester",
            ] {
                let mut f = Fixture::new();
                f.snapshot.goal = Some(mofact_core::snapshot::GoalView {
                    index: 0,
                    have: 3,
                    need: 10,
                });
                match state {
                    "inspect" => f.hud.select(Some(EntityId(2))),
                    "research" => f.hud.research = true,
                    "build" => f.hud.more = true,
                    "radial" => f.hud.radial = Some([size.0 as f32 / 2.0, size.1 as f32 / 2.0]),
                    "recipes" => f.hud.browse = Some("iron-plate".into()),
                    "problems" => f.hud.problems = true,
                    "train" => f.hud.select(Some(EntityId(10))),
                    "menu" => f.hud.open_menu(None),
                    "menu-delete" => {
                        f.hud.open_menu(None);
                        f.hud.confirm_delete = Some(1);
                    }
                    "new-game" => f.hud.open_menu(Some((2, 123_456_789))),
                    "settings" => f.hud.open_settings(),
                    "unlock" => {
                        f.store.demo = Some(3);
                        f.store.price = Some("4,99 €".into());
                        f.hud.open_unlock();
                    }
                    "train-pick" => {
                        f.hud.select(Some(EntityId(10)));
                        f.hud.picking = Some(EntityId(10));
                    }
                    "selection" => f.selection = 12,
                    "requester" => {
                        let mut chest = f.snapshot.entities[2].clone();
                        chest.id = EntityId(7);
                        chest.building = "requester-chest".into();
                        chest.pos = TilePos::new(8, 0);
                        chest.request = Some(("iron-plate".into(), 100));
                        f.snapshot.entities.push(chest);
                        f.hud.select(Some(EntityId(7)));
                    }
                    "library" => {
                        f.hud.library = true;
                        f.blueprints = vec![mofact_input::Blueprint::capture(
                            mofact_input::blueprint::next_name(&[], tr("blueprint.name")),
                            [("assembler".into(), TilePos::new(0, 0), 1, Dir::East, None)],
                        )];
                    }
                    _ => {}
                }
                // Lay out once so the problems list knows what is stuck.
                f.draw(size, scale, tool);
                if state == "problems" {
                    f.hud.problems = true;
                }
                f.draw(size, scale, tool);
                check(&mut f, size, scale, tool, state);
            }
        }
    }
}

#[test]
fn every_control_is_a_finger_wide_on_every_screen() {
    let mut small = Vec::new();
    every_state(|f, size, scale, tool, state| {
        let finger = 44.0 * scale - 0.5;
        for (r, hit) in &f.hud.hits {
            if matches!(hit, Hit::Block) {
                continue;
            }
            let r = f.hud.target(r);
            if r[2] < finger || r[3] < finger {
                let line = format!(
                    "{hit:?} is {:.0}x{:.0} pt on {size:?}@{scale} ({tool:?}, {state})",
                    r[2] / scale,
                    r[3] / scale
                );
                if !small.contains(&line) {
                    small.push(line);
                }
            }
        }
    });
    assert!(
        small.is_empty(),
        "{} too small:\n{}",
        small.len(),
        small.join("\n")
    );
}

#[test]
fn a_tap_anywhere_on_a_control_or_its_target_picks_it() {
    let mut wrong = Vec::new();
    every_state(|f, size, scale, tool, state| {
        let hits = f.hud.hits.clone();
        for (k, (r, hit)) in hits.iter().enumerate() {
            if matches!(hit, Hit::Block) || hits[k + 1..].iter().any(|(o, _)| overlaps(o, r)) {
                // Covered by something drawn later, such as a sheet.
                continue;
            }
            let c = centre(*r);
            if f.hud.hit(c) != Some(hit) {
                wrong.push(format!(
                    "{hit:?}: its centre picks {:?} on {size:?}@{scale} ({tool:?}, {state})",
                    f.hud.hit(c)
                ));
            }
            // Just past a small control's edge, still within a finger of it.
            if r[3] < FINGER * scale - 2.0 {
                let t = f.hud.target(r);
                let below = [c[0], (r[1] + r[3] + t[1] + t[3]) / 2.0];
                let got = f.hud.hit(below);
                let taken = hits
                    .iter()
                    .any(|(o, h)| !matches!(h, Hit::Block) && h != hit && inside(o, below));
                if !taken && got != Some(hit) {
                    wrong.push(format!(
                        "{hit:?}: just below it picks {got:?} on {size:?}@{scale} ({tool:?}, {state})"
                    ));
                }
            }
        }
    });
    wrong.dedup();
    assert!(wrong.is_empty(), "{}:\n{}", wrong.len(), wrong.join("\n"));
}

fn overlaps(a: &Rect, b: &Rect) -> bool {
    a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3]
}

#[test]
fn alternate_recipes_show_under_their_product() {
    let mut f = Fixture::new();
    f.hud.select(Some(EntityId(2)));
    f.draw((1080, 2340), 3.0, &Tool::Hand);
    // Locked away, an alternate shows nowhere.
    let cast = Hit::SetRecipe(EntityId(2), Some("cast-gear".into()));
    assert!(!f.hud.hits.iter().any(|(_, h)| *h == cast));

    // Open, it still waits until gears are chosen, then shows beside the
    // standard way to make them.
    f.snapshot.research.done = vec![
        "steel-working".into(),
        "motors".into(),
        "alternate-assembly".into(),
    ];
    f.draw((1080, 2340), 3.0, &Tool::Hand);
    assert!(!f.hud.hits.iter().any(|(_, h)| *h == cast));
    f.snapshot.entities[1].recipe = Some("gear".into());
    f.draw((1080, 2340), 3.0, &Tool::Hand);
    let gears = Hit::SetRecipe(EntityId(2), Some("gear".into()));
    let ways = f.hud.hits.iter().filter(|(_, h)| *h == gears).count();
    assert_eq!(ways, 2, "the gear product and its standard way");
    let alt = f.find(&cast);
    assert_eq!(
        f.tap(centre(alt)).1,
        vec![Action::Send(Command::SetRecipe {
            entity: EntityId(2),
            recipe: Some("cast-gear".into())
        })]
    );
}

#[test]
fn the_game_menu_loads_deletes_after_asking_and_starts_a_seeded_game() {
    let mut f = Fixture::new();
    let screen = (390, 844);
    f.draw(screen, 1.0, &Tool::Hand);
    // The menu button opens it, and asks for the slots afresh.
    let (_, actions) = f.tap(centre(f.find(&Hit::Menu)));
    assert_eq!(actions, vec![Action::MenuOpened]);
    f.draw(screen, 1.0, &Tool::Hand);
    // Delete asks first; the second tap deletes.
    let (_, actions) = f.tap(centre(f.find(&Hit::SlotDelete(1))));
    assert!(actions.is_empty());
    f.draw(screen, 1.0, &Tool::Hand);
    let (_, actions) = f.tap(centre(f.find(&Hit::SlotDelete(1))));
    assert_eq!(actions, vec![Action::DeleteSlot(1)]);
    f.draw(screen, 1.0, &Tool::Hand);
    // The playing slot can be neither loaded nor deleted.
    assert!(f
        .hud
        .hits
        .iter()
        .all(|(_, h)| *h != Hit::SlotLoad(0) && *h != Hit::SlotDelete(0)));
    let (_, actions) = f.tap(centre(f.find(&Hit::SlotLoad(1))));
    assert_eq!(actions, vec![Action::LoadSlot(1)]);
    assert!(!f.hud.menu_open());

    // A new game: type a seed, pick a preset, start.
    f.hud.open_menu(None);
    f.draw(screen, 1.0, &Tool::Hand);
    f.tap(centre(f.find(&Hit::SlotNew(2))));
    f.draw(screen, 1.0, &Tool::Hand);
    // Clear the rolled seed, then type 7, 0, 7 and take the last back.
    for _ in 0..12 {
        f.tap(centre(f.find(&Hit::Backspace)));
        f.draw(screen, 1.0, &Tool::Hand);
    }
    for d in [7, 0, 7, 3] {
        f.tap(centre(f.find(&Hit::Digit(d))));
        f.draw(screen, 1.0, &Tool::Hand);
    }
    f.tap(centre(f.find(&Hit::Backspace)));
    f.draw(screen, 1.0, &Tool::Hand);
    f.tap(centre(f.find(&Hit::Preset(2))));
    f.draw(screen, 1.0, &Tool::Hand);
    // Endless ore, and the orders stepped from normal to hard.
    f.tap(centre(f.find(&Hit::RuleOre)));
    f.draw(screen, 1.0, &Tool::Hand);
    f.tap(centre(f.find(&Hit::RuleOrders)));
    f.draw(screen, 1.0, &Tool::Hand);
    let (_, actions) = f.tap(centre(f.find(&Hit::StartNew)));
    assert_eq!(
        actions,
        vec![Action::NewGame {
            slot: 2,
            seed: 707,
            preset: 2,
            ore_runs_out: false,
            orders: 3,
        }]
    );
}

#[test]
fn every_recipe_fits_a_small_phone() {
    let mut f = Fixture::new();
    f.snapshot.research.done = f.content.research.iter().map(|r| r.id.clone()).collect();
    f.snapshot.entities[1].recipe = Some("cast-gear".into());
    f.hud.select(Some(EntityId(2)));
    // A small phone held sideways: 568 by 320 points.
    f.draw((1136, 640), 2.0, &Tool::Hand);
    for (r, h) in &f.hud.hits {
        if matches!(h, Hit::SetRecipe(..)) {
            assert!(
                r[1] >= 0.0 && r[1] + r[3] <= 640.0,
                "{h:?} off screen at {r:?}"
            );
        }
    }
}

#[test]
fn the_die_rolls_a_different_seed_each_time() {
    let mut f = Fixture::new();
    f.hud.open_menu(Some((0, 1)));
    let mut seeds = std::collections::HashSet::new();
    for _ in 0..20 {
        f.draw((390, 844), 1.0, &Tool::Hand);
        f.tap(centre(f.find(&Hit::Dice)));
        seeds.insert(f.hud.new_game.as_ref().unwrap().seed.clone());
    }
    assert!(seeds.len() >= 19, "{seeds:?}");
}

#[test]
fn the_recipe_browser_works_out_machines_for_a_rate_even_on_a_small_phone_held_sideways() {
    let mut f = Fixture::new();
    f.hud.browse("circuit".into());
    for (size, scale) in [((1080, 2340), 3.0), ((1334, 750), 2.0)] {
        f.draw(size, scale, &Tool::Hand);
        f.find(&Hit::Rate(true));
        f.find(&Hit::Rate(false));
    }
    assert_eq!(f.hud.rate_per_minute(), 60);
    let up = f.find(&Hit::Rate(true));
    f.tap(centre(up));
    assert_eq!(f.hud.rate_per_minute(), 120);
    // It stops at either end.
    for _ in 0..20 {
        f.draw((1334, 750), 2.0, &Tool::Hand);
        let up = f.find(&Hit::Rate(true));
        f.tap(centre(up));
    }
    assert_eq!(f.hud.rate_per_minute(), *plan::RATES.last().unwrap());
    for _ in 0..20 {
        f.draw((1334, 750), 2.0, &Tool::Hand);
        let down = f.find(&Hit::Rate(false));
        f.tap(centre(down));
    }
    assert_eq!(f.hud.rate_per_minute(), plan::RATES[0]);
    // Something nothing makes has nothing to work out.
    f.hud.browse("copper-ore".into());
    f.draw((1080, 2340), 3.0, &Tool::Hand);
    assert!(!f.hud.hits.iter().any(|(_, h)| matches!(h, Hit::Rate(_))));
}

#[test]
fn a_pack_names_the_research_it_pays_for_at_a_size_that_reads() {
    // Engineering packs pay for dozens of studies: the browser names what
    // is left, what can start now first, and counts the rest, rather than
    // shrinking every name onto one unreadable line.
    let mut f = Fixture::new();
    f.hud.browse("engineering-pack".into());
    for (size, scale) in [((1080, 2340), 3.0), ((1334, 750), 2.0)] {
        f.overlay.keep_words = true;
        f.draw(size, scale, &Tool::Hand);
        let at = f
            .overlay
            .words
            .iter()
            .position(|w| w.contains("Fast belts"))
            .unwrap_or_else(|| {
                panic!(
                    "the first study it pays for is named: {:?}",
                    f.overlay.words
                )
            });
        let line = &f.overlay.words[at];
        assert!(line.contains("more"), "the rest are counted: {line:?}");
        let h = f.overlay.runs[at].0[3];
        assert!(h >= 8.0 * scale, "the line reads: {h} px tall at x{scale}");
    }
}

#[test]
fn the_skyhook_sheet_shows_what_its_stage_still_needs() {
    let mut f = Fixture::new();
    let mut sky = f.snapshot.entities[1].clone();
    sky.id = EntityId(11);
    sky.building = "skyhook".into();
    sky.pos = TilePos::new(20, 0);
    f.snapshot.entities.push(sky);
    f.snapshot.mega = Some(mofact_core::snapshot::MegaView {
        id: EntityId(11),
        stage: 1,
        stages: 4,
        name: "Tether".into(),
        needs: vec![
            ("tether-segment".into(), 40, 300),
            ("superconductor".into(), 200, 200),
        ],
    });
    f.hud.select(Some(EntityId(11)));
    for (size, scale) in [((1280, 720), 1.0), ((1080, 2340), 3.0)] {
        f.draw(size, scale, &Tool::Hand);
        let tether = f.find(&Hit::Item("tether-segment".into()));
        let wire = f.find(&Hit::Item("superconductor".into()));
        assert!(wire[0] > tether[0]);
        assert!(
            wire[0] + wire[2] <= size.0 as f32,
            "the needs fit the screen"
        );
    }
}

#[test]
fn research_past_the_demo_opens_the_full_game_sheet() {
    let mut f = Fixture::new();
    f.snapshot.research.done = [
        "steel-working",
        "motors",
        "fluid-handling",
        "steam-power",
        "oil-processing",
        "sulfur-processing",
        "advanced-electronics",
        "chemistry",
        "silicon-processing",
        "precision-engineering",
    ]
    .map(ResearchId::from)
    .to_vec();
    f.store.demo = Some(4);
    f.store.price = Some("€4.99".into());
    for (size, scale) in [((1280, 720), 1.0), ((1080, 2340), 3.0)] {
        f.hud = Hud::new();
        f.draw(size, scale, &Tool::Hand);
        f.hud.toggle_research();
        f.draw(size, scale, &Tool::Hand);
        assert!(
            f.hud
                .hits
                .iter()
                .any(|(_, h)| h == &Hit::Study(Some("solar-energy".into()))),
            "the demo's own tiers can still be studied"
        );
        f.tap(centre(f.find(&Hit::Unlock)));
        f.draw(size, scale, &Tool::Hand);
        let buy = f.find(&Hit::Buy);
        assert!(buy[0] + buy[2] <= size.0 as f32);
        assert_eq!(f.tap(centre(buy)).1, vec![Action::Buy]);
        assert_eq!(
            f.tap(centre(f.find(&Hit::Restore))).1,
            vec![Action::Restore]
        );
    }
    // Once bought the sheet goes, and the rows say Study.
    f.store.demo = None;
    f.draw((1280, 720), 1.0, &Tool::Hand);
    assert!(!f.hud.hits.iter().any(|(_, h)| h == &Hit::Buy));
    f.hud.toggle_research();
    f.draw((1280, 720), 1.0, &Tool::Hand);
    assert!(f
        .hud
        .hits
        .iter()
        .any(|(_, h)| h == &Hit::Study(Some("titanium-smelting".into()))));
}

#[test]
fn late_and_repeatable_research_shows_on_a_phone_and_can_be_studied_again() {
    let mut f = Fixture::new();
    // Everything done that can be: only the repeatable research is left.
    f.snapshot.research.done = f.content.research.iter().map(|r| r.id.clone()).collect();
    f.snapshot.research.levels = vec![("deep-survey".into(), 2)];
    f.hud.toggle_research();
    f.draw((1080, 2340), 3.0, &Tool::Hand);
    for id in ["deep-survey", "lean-assembly", "lab-automation"] {
        f.find(&Hit::Study(Some(id.into())));
    }
}

#[test]
fn a_waiting_crash_report_can_be_copied_from_the_game_menu() {
    let mut f = Fixture::new();
    for (size, scale) in [((1280, 720), 1.0), ((1080, 2340), 3.0)] {
        f.hud = Hud::new();
        f.crash = false;
        f.hud.open_menu(None);
        f.draw(size, scale, &Tool::Hand);
        assert!(!f.hud.hits.iter().any(|(_, h)| h == &Hit::CopyCrash));
        f.crash = true;
        f.draw(size, scale, &Tool::Hand);
        let b = f.find(&Hit::CopyCrash);
        assert!(b[1] + b[3] <= size.1 as f32, "it fits on screen");
        assert_eq!(f.tap(centre(b)).1, vec![Action::CopyCrash]);
    }
}

/// Every character in `lang`'s strings and content names.
fn texts(lang: i18n::Lang) -> Vec<String> {
    i18n::set_lang(lang);
    let mut out: Vec<String> = i18n::table(i18n::Lang::En)
        .keys()
        .map(|k| tr(k).to_string())
        .collect();
    let c = i18n::localize(&content());
    out.extend(i18n::content_keys(&c).into_iter().map(|(_, v)| v));
    out.extend(i18n::Lang::ALL.iter().map(|l| l.native().to_string()));
    i18n::set_lang(i18n::Lang::En);
    out
}

#[test]
fn every_character_has_a_glyph() {
    let o = Overlay::new(&Look::default());
    let mut missing = Vec::new();
    for lang in i18n::Lang::ALL {
        for text in texts(lang) {
            for c in text.chars().filter(|c| !o.has_glyph(*c)) {
                let line = format!("{}: {c:?} in {text:?}", lang.code());
                if !missing.contains(&line) {
                    missing.push(line);
                }
            }
        }
    }
    assert!(
        missing.is_empty(),
        "no glyph for these: run tools/fonts/subset.py\n{}",
        missing.join("\n")
    );
}

#[test]
fn every_content_name_is_translated() {
    // New content would show in English until translated: translate it in
    // the same pull request, in every crates/ui/lang file.
    let keys = i18n::content_keys(&content());
    for lang in i18n::Lang::ALL.into_iter().skip(1) {
        let t = i18n::table(lang);
        let done = keys
            .iter()
            .filter(|(k, _)| t.contains_key(k.as_str()))
            .count();
        assert!(
            done == keys.len(),
            "{}: only {done} of {} content names translated",
            lang.code(),
            keys.len()
        );
    }
}

/// Draw every state in `lang`, with the content in that language as the
/// app shows it, and say where words run off the screen or out of their
/// button.
fn overflows(lang: i18n::Lang) -> Vec<String> {
    overflows_with(lang, false)
}

fn overflows_with(lang: i18n::Lang, large: bool) -> Vec<String> {
    let mut wrong = Vec::new();
    every_state(|f, size, scale, tool, state| {
        f.hud.set_large_text(large);
        i18n::set_lang(lang);
        f.content = i18n::localize(&f.content);
        f.draw(size, scale, tool);
        let (w, h) = (size.0 as f32, size.1 as f32);
        let o = &f.overlay;
        // A control whose panel goes down after the words lies over them.
        let over_it = |r: &Rect, at: usize| {
            o.panels
                .iter()
                .any(|(p, i)| *i > at && (0..4).all(|k| (p[k] - r[k]).abs() < 1.0))
        };
        for &([x, y, rw, rh], at) in &o.runs {
            let mut why = None;
            if x < -0.5 || y < -0.5 || x + rw > w + 0.5 || y + rh > h + 0.5 {
                why = Some("off the screen".to_string());
            }
            // Words shrink to fit, but not past reading.
            if rh / scale < 7.0 {
                why = Some(format!("shrunk to {:.1} pt", rh / scale));
            }
            // The control the words are centred in, innermost: they start
            // and end in it too.
            let inside = f
                .hud
                .hits
                .iter()
                .filter(|(r, hit)| {
                    // Item icons and the round × and ☰ buttons hold no words.
                    !matches!(hit, Hit::Block | Hit::Item(_) | Hit::Close | Hit::Menu)
                        && x + rw / 2.0 >= r[0]
                        && x + rw / 2.0 <= r[0] + r[2]
                        && y + rh / 2.0 >= r[1]
                        && y + rh / 2.0 <= r[1] + r[3]
                        && !over_it(r, at)
                })
                .min_by(|a, b| (a.0[2] * a.0[3]).total_cmp(&(b.0[2] * b.0[3])));
            if let Some((r, hit)) = inside {
                let over = (r[0] - x).max(x + rw - r[0] - r[2]);
                if over > 1.0 {
                    why = Some(format!("out of {hit:?} by {over:.0} px"));
                }
            }
            if let Some(why) = why {
                let line = format!(
                    "{}: text at {:.0},{:.0} ({:.0} wide) {why} on {size:?}@{scale} ({tool:?}, {state})",
                    lang.code(),
                    x,
                    y,
                    rw
                );
                if !wrong.contains(&line) {
                    wrong.push(line);
                }
            }
        }
        i18n::set_lang(i18n::Lang::En);
    });
    wrong
}

#[test]
fn english_fits_every_screen() {
    let wrong = overflows(i18n::Lang::En);
    assert!(wrong.is_empty(), "{}:\n{}", wrong.len(), wrong.join("\n"));
}

#[test]
fn large_text_fits_every_screen_in_english_and_german() {
    let mut wrong = overflows_with(i18n::Lang::En, true);
    wrong.extend(overflows_with(i18n::Lang::De, true));
    assert!(wrong.is_empty(), "{}:\n{}", wrong.len(), wrong.join("\n"));
}

#[test]
fn large_text_draws_bigger_and_the_settings_toggle_both() {
    let mut f = Fixture::new();
    let size = (1170, 2532);
    f.hud.open_settings();
    f.draw(size, 3.0, &Tool::Hand);
    let small = f.hud.unit;
    let large = centre(f.find(&Hit::LargeText));
    assert_eq!(f.tap(large).1, vec![Action::LargeText(true)]);
    assert!(f.hud.large_text());
    f.draw(size, 3.0, &Tool::Hand);
    assert!(f.hud.unit > small * 1.1, "{} after {small}", f.hud.unit);
    let motion = centre(f.find(&Hit::Motion));
    assert_eq!(f.tap(motion).1, vec![Action::ReduceMotion(true)]);
    assert!(f.hud.reduce_motion());
}

#[test]
fn german_and_french_fit_every_screen() {
    let mut wrong = overflows(i18n::Lang::De);
    wrong.extend(overflows(i18n::Lang::Fr));
    assert!(wrong.is_empty(), "{}:\n{}", wrong.len(), wrong.join("\n"));
}

#[test]
fn spanish_and_portuguese_fit_every_screen() {
    let mut wrong = overflows(i18n::Lang::Es);
    wrong.extend(overflows(i18n::Lang::PtBr));
    assert!(wrong.is_empty(), "{}:\n{}", wrong.len(), wrong.join("\n"));
}

#[test]
fn japanese_and_chinese_fit_every_screen() {
    let mut wrong = overflows(i18n::Lang::Ja);
    wrong.extend(overflows(i18n::Lang::ZhHans));
    assert!(wrong.is_empty(), "{}:\n{}", wrong.len(), wrong.join("\n"));
}

#[test]
fn a_whole_session_of_han_text_fits_the_atlas() {
    // One overlay lives as long as the app: every sheet's glyphs, at every
    // size, on a 3x phone, must find room alongside the icons.
    for lang in [i18n::Lang::Ja, i18n::Lang::ZhHans] {
        let mut f = Fixture::new();
        i18n::set_lang(lang);
        f.content = i18n::localize(&f.content);
        let size = (1179, 2556);
        for open in 0..6 {
            f.hud.research = open == 1;
            f.hud.more = open == 2;
            f.hud
                .select(if open == 3 { Some(EntityId(2)) } else { None });
            if open == 4 {
                f.hud.open_menu(Some((1, 42)));
            }
            if open == 5 {
                f.hud.browse = Some("iron-plate".into());
            }
            f.draw(size, 3.0, &Tool::Place("assembler".into()));
            f.draw((2556, 1179), 3.0, &Tool::Belt);
        }
        // And every string at every size text comes in, a sheetful of
        // characters a frame.
        let all: Vec<char> = texts(lang).concat().chars().collect();
        i18n::set_lang(lang);
        for px in [30.0, 33.0, 36.0, 39.0, 42.0, 45.0] {
            for sheet in all.chunks(150) {
                f.overlay.clear();
                let sheet: String = sheet.iter().collect();
                f.overlay.text(&sheet, [0.0, 100.0], px, [255; 4]);
            }
        }
        i18n::set_lang(i18n::Lang::En);
        assert_eq!(f.overlay.dropped, 0, "{}: glyphs left out", lang.code());
    }
}

#[test]
fn a_tap_on_the_tool_tip_builds_nothing_under_it() {
    let mut f = Fixture::new();
    let size = (1170, 2532);
    f.draw(size, 3.0, &Tool::Belt);
    let done = f.find(&Hit::Done);
    // The card beside Done, in its middle.
    let at = [done[0] / 2.0, done[1] + done[3] / 2.0];
    assert!(f.tap(at).0, "the card takes the tap");
    // Out on the map the tap is the game's.
    assert!(!f.tap([size.0 as f32 / 2.0, size.1 as f32 * 0.6]).0);
}

#[test]
fn a_build_sheet_too_big_for_the_screen_pages_through_every_building() {
    let mut f = Fixture::new();
    // A phone held sideways: the sheet has room for one row.
    let size = (2532, 1170);
    f.draw(size, 3.0, &Tool::Hand);
    let more = f.find(&Hit::Tool(MORE));
    f.tap(centre(more));
    let mut seen = std::collections::HashSet::new();
    let mut pages = 0;
    loop {
        f.draw(size, 3.0, &Tool::Hand);
        pages += 1;
        for (_, h) in &f.hud.hits {
            if let Hit::Build(b) = h {
                seen.insert(b.clone());
            }
        }
        let Some(next) = f.hud.hits.iter().find(|(_, h)| *h == Hit::Page(true)) else {
            break;
        };
        let next = centre(next.0);
        assert_eq!(f.tap(next), (true, vec![]));
        assert!(pages < 50, "the pages never end");
    }
    assert!(pages > 1, "it needed more than one page");
    // Every building open now can be picked from some page.
    for b in ["lab", "pole", "wind-turbine", "radar"] {
        assert!(seen.contains(&BuildingId::from(b)), "{b} on no page");
    }
    // And back to the first.
    for _ in 1..pages {
        let back = centre(f.find(&Hit::Page(false)));
        f.tap(back);
        f.draw(size, 3.0, &Tool::Hand);
    }
    f.find(&Hit::Build("lab".into()));
    assert!(!f.hud.hits.iter().any(|(_, h)| *h == Hit::Page(false)));
    assert!(f.overlay.has_glyph('‹') && f.overlay.has_glyph('›'));
}

#[test]
fn the_research_sheet_pages_too() {
    let mut f = Fixture::new();
    let size = (2532, 1170);
    f.hud.research = true;
    f.draw(size, 3.0, &Tool::Hand);
    let next = centre(f.find(&Hit::Page(true)));
    let first: Vec<_> = f
        .hud
        .hits
        .iter()
        .filter(|(_, h)| matches!(h, Hit::Study(_)))
        .cloned()
        .collect();
    f.tap(next);
    f.draw(size, 3.0, &Tool::Hand);
    let second: Vec<_> = f
        .hud
        .hits
        .iter()
        .filter(|(_, h)| matches!(h, Hit::Study(_)))
        .cloned()
        .collect();
    assert_ne!(first, second, "the next page shows other research");
    f.find(&Hit::Page(false));
}

#[test]
fn the_select_tool_offers_what_to_do_with_a_selection_and_the_library() {
    let mut f = Fixture::new();
    let size = (1170, 2532);
    let select = centre({
        f.draw(size, 3.0, &Tool::Hand);
        f.find(&Hit::Tool(SELECT))
    });
    assert_eq!(f.tap(select).1, vec![Action::Tool(Tool::Select)]);
    f.draw(size, 3.0, &Tool::Select);
    assert!(
        f.hud.hits.iter().all(|(_, h)| *h != Hit::Copy),
        "nothing selected yet"
    );
    f.selection = 3;
    f.draw(size, 3.0, &Tool::Select);
    for (hit, action) in [
        (Hit::Copy, Action::Copy),
        (Hit::Cut, Action::Cut),
        (Hit::Erase, Action::DeleteSelection),
        (Hit::Keep, Action::SaveBlueprint),
    ] {
        let at = centre(f.find(&hit));
        assert_eq!(f.tap(at), (true, vec![action]));
    }
    // A blueprint in hand: turn it, or build it.
    f.draw(size, 3.0, &Tool::Paste);
    assert_eq!(f.tap(centre(f.find(&Hit::Rotate))).1, vec![Action::Rotate]);
    assert_eq!(
        f.tap(centre(f.find(&Hit::Place))).1,
        vec![Action::PlacePaste]
    );
    assert_eq!(
        f.hud.current,
        Some(SELECT),
        "pasting lights the Select slot"
    );

    // The library: empty, then with a blueprint to take or delete.
    f.tap(centre(f.find(&Hit::Library)));
    f.draw(size, 3.0, &Tool::Paste);
    assert!(f.hud.hits.iter().all(|(_, h)| !matches!(h, Hit::Pick(_))));
    f.blueprints = vec![mofact_input::Blueprint::capture(
        "Blueprint 1",
        [("assembler".into(), TilePos::new(0, 0), 1, Dir::East, None)],
    )];
    f.draw(size, 3.0, &Tool::Paste);
    let forget = centre(f.find(&Hit::Forget(0)));
    assert_eq!(f.tap(forget).1, vec![], "the first tap asks");
    f.draw(size, 3.0, &Tool::Paste);
    assert_eq!(f.tap(forget).1, vec![Action::ForgetBlueprint(0)]);
    f.draw(size, 3.0, &Tool::Paste);
    assert_eq!(
        f.tap(centre(f.find(&Hit::Pick(0)))).1,
        vec![Action::Paste(0)]
    );
    f.draw(size, 3.0, &Tool::Paste);
    assert!(
        f.hud.hits.iter().all(|(_, h)| !matches!(h, Hit::Pick(_))),
        "picking one closes the library"
    );
}

#[test]
fn a_requester_chest_sheet_picks_what_drones_bring_and_how_many() {
    let mut f = Fixture::new();
    let mut chest = f.snapshot.entities[2].clone();
    chest.id = EntityId(7);
    chest.building = "requester-chest".into();
    chest.pos = TilePos::new(8, 0);
    f.snapshot.entities.push(chest);
    f.hud.select(Some(EntityId(7)));
    f.draw((1280, 720), 1.0, &Tool::Hand);
    // Nothing asked for yet: pick one of the items in stock.
    let iron = Hit::SetRequest(EntityId(7), Some(("iron-plate".into(), 50)));
    assert_eq!(
        f.tap(centre(f.find(&iron))).1,
        vec![Action::Send(Command::SetRequest {
            entity: EntityId(7),
            request: Some(("iron-plate".into(), 50))
        })]
    );
    assert!(
        f.hud
            .hits
            .iter()
            .all(|(_, h)| *h != Hit::SetRequest(EntityId(7), Some(("iron-plate".into(), 200)))),
        "no counts until it asks for something"
    );
    // Asking for iron: the counts show, and none turns it off.
    f.snapshot.entities[3].request = Some(("iron-plate".into(), 50));
    f.draw((1280, 720), 1.0, &Tool::Hand);
    let more = Hit::SetRequest(EntityId(7), Some(("iron-plate".into(), 200)));
    f.find(&more);
    f.find(&Hit::SetRequest(EntityId(7), None));
}

#[test]
fn a_long_blueprint_library_pages_on_a_sideways_phone() {
    let mut f = Fixture::new();
    let size = (2532, 1170);
    f.blueprints = (0..12)
        .map(|k| {
            mofact_input::Blueprint::capture(
                format!("Blueprint {}", k + 1),
                [("assembler".into(), TilePos::new(0, 0), 1, Dir::East, None)],
            )
        })
        .collect();
    f.hud.library = true;
    let mut picks = std::collections::HashSet::new();
    for _ in 0..20 {
        f.draw(size, 3.0, &Tool::Paste);
        for (_, h) in &f.hud.hits {
            if let Hit::Pick(i) = h {
                picks.insert(*i);
            }
        }
        let Some(next) = f.hud.hits.iter().find(|(_, h)| *h == Hit::Page(true)) else {
            break;
        };
        let next = centre(next.0);
        f.tap(next);
    }
    assert_eq!(picks.len(), 12, "every blueprint on some page: {picks:?}");
    // Each page's rows take the blueprint they show.
    let last = centre(f.find(&Hit::Pick(11)));
    assert_eq!(f.tap(last).1, vec![Action::Paste(11)]);
}

/// Words in `text` that look like English left in by mistake: three or
/// more Latin letters in a row, other than units, names and the seed.
fn english_in(text: &str) -> Vec<String> {
    const FINE: [&str; 8] = ["kW", "fps", "Mofact", "AI", "OK", "Auto", "English", "min"];
    text.split(|c: char| !c.is_ascii_alphabetic())
        .filter(|w| w.len() >= 3 && !FINE.contains(w))
        .map(String::from)
        .collect()
}

#[test]
fn japanese_and_chinese_screens_show_no_english() {
    // Every word on screen in Japanese and Chinese goes through the
    // string tables: a hardcoded English label shows up here as Latin
    // letters, in any HUD state, on any screen.
    let mut found = Vec::new();
    for lang in [i18n::Lang::Ja, i18n::Lang::ZhHans] {
        every_state(|f, size, scale, tool, state| {
            i18n::set_lang(lang);
            f.content = i18n::localize(&f.content);
            // The slots' words come from the app, in the player's language.
            for s in &mut f.slots {
                if s.used {
                    s.title = trf("slot.title", &[&tr("preset.lakes"), &42]);
                    s.detail = trf("slot.detail", &[&tr("slot.just_started"), &14]);
                }
            }
            // So are blueprints' names, as the player saved them.
            for b in &mut f.blueprints {
                b.name = mofact_input::blueprint::next_name(&[], tr("blueprint.name"));
            }
            f.overlay.keep_words = true;
            f.draw(size, scale, tool);
            for w in &f.overlay.words {
                for e in english_in(w) {
                    let line = format!("{}: {e:?} in {w:?} ({state}, {tool:?})", lang.code());
                    if !found.contains(&line) {
                        found.push(line);
                    }
                }
            }
            i18n::set_lang(i18n::Lang::En);
        });
    }
    assert!(found.is_empty(), "English on screen:\n{}", found.join("\n"));
}

#[test]
fn no_words_are_written_into_the_code() {
    // Toasts and labels take their words from the string tables. A literal
    // like `toast("Saved")` would stay English in every language.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut found = Vec::new();
    for dir in ["crates/ui/src", "app/src", "crates/input/src"] {
        for entry in std::fs::read_dir(root.join(dir)).expect("source folder") {
            let path = entry.expect("file").path();
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if !name.ends_with(".rs") || name == "tests.rs" {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("source");
            // Only the code before its tests.
            let code = text.split("#[cfg(test)]").next().unwrap_or_default();
            for (n, line) in code.lines().enumerate() {
                for call in ["toast(\"", ".text(\"", "button(\""] {
                    let Some(at) = line.find(call) else { continue };
                    let word: String = line[at + call.len()..]
                        .chars()
                        .take_while(|c| *c != '"')
                        .collect();
                    if word.chars().filter(char::is_ascii_alphabetic).count() >= 2 {
                        found.push(format!("{dir}/{name}:{}: {}", n + 1, line.trim()));
                    }
                }
            }
        }
    }
    assert!(found.is_empty(), "use tr(\"key\"):\n{}", found.join("\n"));
}

#[test]
fn the_start_menu_plays_on_starts_a_new_game_and_comes_back_from_its_sheets() {
    let mut f = Fixture::new();
    let screen = (390, 844);
    f.hud.open_title();
    f.draw(screen, 1.0, &Tool::Hand);
    // It covers everything: no toolbar, and a tap on the world goes nowhere.
    assert!(f.hud.hits.iter().all(|(_, h)| !matches!(h, Hit::Tool(_))));
    assert!(f.hud.covers([5.0, 5.0]));
    // The full game is offered only in the demo.
    assert!(f.hud.hits.iter().all(|(_, h)| *h != Hit::TitleBuy));

    // New game opens the form in the first empty slot.
    let (_, actions) = f.tap(centre(f.find(&Hit::TitleNew)));
    assert_eq!(actions, vec![Action::MenuOpened]);
    assert!(!f.hud.title_open() && f.hud.menu_open());
    assert_eq!(f.hud.new_game.as_ref().map(|g| g.slot), Some(2));
    f.draw(screen, 1.0, &Tool::Hand);
    // Back out of the form, then close the menu: the start menu is back.
    f.tap(centre(f.find(&Hit::CancelNew)));
    f.draw(screen, 1.0, &Tool::Hand);
    f.tap(centre(f.find(&Hit::Menu)));
    assert!(f.hud.title_open() && !f.hud.menu_open());

    // Load lists the slots; loading one plays it.
    f.draw(screen, 1.0, &Tool::Hand);
    f.tap(centre(f.find(&Hit::TitleLoad)));
    f.draw(screen, 1.0, &Tool::Hand);
    let (_, actions) = f.tap(centre(f.find(&Hit::SlotLoad(1))));
    assert_eq!(actions, vec![Action::LoadSlot(1)]);
    assert!(!f.hud.title_open() && !f.hud.menu_open());
    // Closing the menu now just closes it.
    f.draw(screen, 1.0, &Tool::Hand);
    f.tap(centre(f.find(&Hit::Menu)));
    f.draw(screen, 1.0, &Tool::Hand);
    f.tap(centre(f.find(&Hit::Menu)));
    assert!(!f.hud.title_open() && !f.hud.menu_open());

    // In the demo, the full game's sheet opens from it and closes back to it.
    f.store.demo = Some(4);
    f.hud.open_title();
    f.draw(screen, 1.0, &Tool::Hand);
    f.tap(centre(f.find(&Hit::TitleBuy)));
    f.draw(screen, 1.0, &Tool::Hand);
    f.find(&Hit::Buy);
    f.find(&Hit::Restore);
    f.tap(centre(f.find(&Hit::Close)));
    assert!(f.hud.title_open());

    // Continue plays on.
    f.draw(screen, 1.0, &Tool::Hand);
    let (_, actions) = f.tap(centre(f.find(&Hit::TitleContinue)));
    assert!(actions.is_empty());
    assert!(!f.hud.title_open());
}

#[test]
fn the_start_menu_fits_a_small_phone_held_either_way_in_every_language() {
    for lang in i18n::Lang::ALL {
        i18n::set_lang(lang);
        for (size, scale) in [((640, 1136), 2.0), ((1136, 640), 2.0), ((2048, 2732), 2.0)] {
            let mut f = Fixture::new();
            f.store.demo = Some(4);
            f.hud.open_title();
            f.draw(size, scale, &Tool::Hand);
            let buttons: Vec<Rect> = f
                .hud
                .hits
                .iter()
                .filter(|(_, h)| *h != Hit::Block)
                .map(|(r, _)| *r)
                .collect();
            assert_eq!(buttons.len(), 5);
            for r in &buttons {
                assert!(
                    r[0] >= 0.0
                        && r[1] >= 0.0
                        && r[0] + r[2] <= size.0 as f32
                        && r[1] + r[3] <= size.1 as f32,
                    "{lang:?} {size:?}: {r:?} off screen"
                );
                assert!(
                    r[3] >= FINGER * scale - 0.5,
                    "{lang:?} {size:?}: {r:?} too small"
                );
            }
        }
    }
    i18n::set_lang(i18n::Lang::En);
}

#[test]
fn a_filter_splitter_sheet_picks_the_item_that_goes_ahead() {
    let mut f = Fixture::new();
    let mut splitter = f.snapshot.entities[2].clone();
    splitter.id = EntityId(7);
    splitter.building = "filter-splitter".into();
    splitter.recipe = None;
    splitter.pos = TilePos::new(8, 0);
    f.snapshot.entities.push(splitter);
    let at = f.snapshot.entities.len() - 1;
    f.hud.select(Some(EntityId(7)));
    // On a phone held upright as well as a wide screen.
    for (size, scale) in [((1080, 2340), 3.0), ((1280, 720), 1.0)] {
        f.snapshot.entities[at].filter = None;
        f.draw(size, scale, &Tool::Hand);
        let iron = Hit::SetFilter(EntityId(7), Some("iron-plate".into()));
        let pill = f.find(&iron);
        assert!(pill[3] >= 36.0 * scale, "a pill a finger can hit: {pill:?}");
        assert_eq!(
            f.tap(centre(pill)).1,
            vec![Action::Send(Command::SetFilter {
                entity: EntityId(7),
                filter: Some("iron-plate".into())
            })]
        );
        // Set to iron: nothing turns it off again.
        f.snapshot.entities[at].filter = Some("iron-plate".into());
        f.draw(size, scale, &Tool::Hand);
        assert_eq!(
            f.tap(centre(f.find(&Hit::SetFilter(EntityId(7), None)))).1,
            vec![Action::Send(Command::SetFilter {
                entity: EntityId(7),
                filter: None
            })]
        );
    }
}

#[test]
fn drills_running_dry_and_mined_out_show_as_problems() {
    let mut f = Fixture::new();
    let drill = |id, x, left, state| EntityView {
        request: None,
        filter: None,
        held: Vec::new(),
        keep: 0,
        when: None,
        id: EntityId(id),
        building: "drill".into(),
        pos: TilePos::new(x, 0),
        dir: Dir::East,
        recipe: None,
        ore: (left > 0).then(|| "iron-ore".into()),
        ore_left: Some(left),
        grid: None,
        exit: false,
        fluid: None,
        links: 0,
        state,
        progress: 0.0,
    };
    f.snapshot.entities = vec![
        drill(1, 0, 0, MachineState::Starved),
        drill(2, 2, 0, MachineState::Starved),
        drill(3, 4, 120, MachineState::Working),
        drill(4, 6, 50_000, MachineState::Working),
    ];
    f.draw((1080, 2340), 3.0, &Tool::Hand);
    let lines: Vec<_> = f
        .hud
        .found
        .iter()
        .map(|p| (p.trouble, p.at.len()))
        .collect();
    use problems::Trouble as T;
    assert_eq!(
        lines,
        vec![(T::MinedOut, 2), (T::LowOre, 1)],
        "a drill with plenty left is fine"
    );
}

#[test]
fn a_lab_with_no_research_picked_shows_as_the_cause_of_a_full_assembler() {
    let mut f = Fixture::new();
    let at = |id, x, building: &str, recipe: Option<&str>, state| EntityView {
        when: None,
        request: None,
        filter: None,
        held: Vec::new(),
        keep: 0,
        id: EntityId(id),
        building: building.into(),
        pos: TilePos::new(x, 0),
        dir: Dir::East,
        recipe: recipe.map(Into::into),
        ore: None,
        ore_left: None,
        grid: None,
        exit: false,
        fluid: None,
        links: 0,
        state,
        progress: 0.0,
    };
    f.snapshot.entities = vec![
        at(
            1,
            0,
            "assembler",
            Some("survey-pack"),
            MachineState::Blocked,
        ),
        at(2, 1, "lab", None, MachineState::Idle),
        at(3, 2, "lab", None, MachineState::Working),
    ];
    f.draw((1080, 2340), 3.0, &Tool::Hand);
    let lines: Vec<_> = f
        .hud
        .found
        .iter()
        .map(|p| (p.trouble, p.at.len(), p.label.clone()))
        .collect();
    use problems::Trouble as T;
    assert_eq!(
        lines,
        vec![
            (T::Blocked, 1, "Assembler · output full".to_string()),
            (T::NoResearch, 1, "Lab · no research picked".to_string()),
        ]
    );
}

#[test]
fn the_order_card_counts_down_and_a_shipped_order_says_so() {
    use mofact_core::snapshot::OrderView;
    let mut f = Fixture::new();
    f.snapshot.order = Some(OrderView {
        index: 1,
        item: "gear".into(),
        have: 40,
        need: 150,
        seconds_left: 75,
        streak: 2,
        reward: vec![("iron-plate".into(), 420), ("wire".into(), 168)],
    });
    f.draw((1080, 2340), 3.0, &Tool::Hand);
    assert_eq!(f.hud.orders_seen, Some((0, 0)));
    assert!(f.hud.toast.is_none(), "nothing ended yet");
    let blocks = f.hud.hits.iter().filter(|(_, h)| *h == Hit::Block).count();
    // It ships: the card goes and a toast says so.
    f.snapshot.order = None;
    f.snapshot.orders_shipped = 1;
    f.draw((1080, 2340), 3.0, &Tool::Hand);
    let toast = f.hud.toast.as_ref().map(|t| t.0.clone());
    assert_eq!(toast.as_deref(), Some(tr("toast.order_shipped")));
    let now = f.hud.hits.iter().filter(|(_, h)| *h == Hit::Block).count();
    assert_eq!(now, blocks, "the toast takes the card's place");
}

#[test]
fn a_warehouse_sheet_shows_what_it_holds_and_picks_what_goes_out() {
    let mut f = Fixture::new();
    let mut w = f.snapshot.entities[2].clone();
    w.id = EntityId(7);
    w.building = "warehouse".into();
    w.pos = TilePos::new(8, 0);
    w.held = vec![("copper-plate".into(), 40), ("gear".into(), 300)];
    f.snapshot.entities.push(w);
    f.hud.select(Some(EntityId(7)));
    for (size, scale) in [((1280, 720), 1.0), ((750, 1334), 2.0)] {
        f.draw(size, scale, &Tool::Hand);
        // What it holds opens the recipe browser, as a machine's items do.
        f.find(&Hit::Item("gear".into()));
        f.find(&Hit::Item("copper-plate".into()));
        // Everything, or one of what it holds, most first.
        f.find(&Hit::SetFilter(EntityId(7), None));
        let gear = Hit::SetFilter(EntityId(7), Some("gear".into()));
        let copper = Hit::SetFilter(EntityId(7), Some("copper-plate".into()));
        assert!(f.find(&gear)[0] < f.find(&copper)[0], "most first");
        // Its safety stock: none chosen yet, and a count to keep back.
        f.find(&Hit::SetKeep(EntityId(7), 0));
        assert_eq!(
            f.tap(centre(f.find(&Hit::SetKeep(EntityId(7), 100)))).1,
            vec![Action::Send(Command::SetKeep {
                entity: EntityId(7),
                keep: 100
            })]
        );
        f.hud.select(Some(EntityId(7)));
        f.draw(size, scale, &Tool::Hand);
        assert_eq!(
            f.tap(centre(f.find(&copper))).1,
            vec![Action::Send(Command::SetFilter {
                entity: EntityId(7),
                filter: Some("copper-plate".into())
            })]
        );
        f.hud.select(Some(EntityId(7)));
    }
}

#[test]
fn the_settings_sheet_opens_from_both_menus_and_goes_back_to_where_it_came_from() {
    let mut f = Fixture::new();
    let screen = (390, 844);
    f.hud.open_title();
    f.draw(screen, 1.0, &Tool::Hand);
    f.tap(centre(f.find(&Hit::TitleSettings)));
    assert!(f.hud.settings_open() && !f.hud.title_open());
    f.draw(screen, 1.0, &Tool::Hand);
    // The volume steps down to silent and round to full again.
    let mut steps = Vec::new();
    for _ in 0..5 {
        let (_, actions) = f.tap(centre(f.find(&Hit::Volume)));
        steps.extend(actions);
        f.draw(screen, 1.0, &Tool::Hand);
    }
    assert_eq!(
        steps,
        [75, 50, 25, 0, 100].map(Action::Volume).to_vec(),
        "volume steps"
    );
    assert_eq!(
        f.tap(centre(f.find(&Hit::Ambience))).1,
        vec![Action::Ambience(false)]
    );
    assert!(!f.hud.ambience());
    f.draw(screen, 1.0, &Tool::Hand);
    assert_eq!(
        f.tap(centre(f.find(&Hit::Vibration))).1,
        vec![Action::Vibration(false)]
    );
    assert!(!f.hud.vibration());
    f.draw(screen, 1.0, &Tool::Hand);
    for hit in [
        Hit::Power,
        Hit::Motion,
        Hit::ColourBlind,
        Hit::LargeText,
        Hit::Language,
    ] {
        f.find(&hit);
    }
    f.tap(centre(f.find(&Hit::SettingsClose)));
    assert!(f.hud.title_open() && !f.hud.settings_open());

    // From the game menu, closing goes back to the menu.
    f.hud.close_title();
    f.hud.open_menu(None);
    f.draw(screen, 1.0, &Tool::Hand);
    f.tap(centre(f.find(&Hit::Settings)));
    assert!(f.hud.settings_open() && !f.hud.menu_open());
    f.draw(screen, 1.0, &Tool::Hand);
    // A menu of its own: none of the game's HUD round it, and a tap
    // anywhere stays in it.
    assert!(f
        .hud
        .hits
        .iter()
        .all(|(_, h)| !matches!(h, Hit::Tool(_) | Hit::Menu | Hit::Research | Hit::Sound)));
    assert!(f.hud.covers([5.0, 5.0]));
    let (_, actions) = f.tap(centre(f.find(&Hit::SettingsClose)));
    assert_eq!(actions, vec![Action::MenuOpened]);
    assert!(f.hud.menu_open() && !f.hud.settings_open());

    // A volume set from a saved file lands on the nearest step.
    f.hud.set_volume(60);
    assert_eq!(f.hud.volume(), 50);
}

#[test]
fn the_settings_sheet_and_new_game_rules_fit_a_small_phone_either_way_in_every_language() {
    for lang in i18n::Lang::ALL {
        i18n::set_lang(lang);
        for (size, scale) in [((640, 1136), 2.0), ((1136, 640), 2.0), ((2048, 2732), 2.0)] {
            for form in [false, true] {
                let mut f = Fixture::new();
                if form {
                    f.hud.open_menu(Some((0, 42)));
                } else {
                    f.hud.open_settings();
                }
                f.draw(size, scale, &Tool::Hand);
                let want = if form {
                    vec![Hit::RuleOre, Hit::RuleOrders, Hit::StartNew]
                } else {
                    vec![
                        Hit::Volume,
                        Hit::Vibration,
                        Hit::Language,
                        Hit::SettingsClose,
                    ]
                };
                for hit in want {
                    let r = f.find(&hit);
                    assert!(
                        r[0] >= 0.0
                            && r[1] >= 0.0
                            && r[0] + r[2] <= size.0 as f32
                            && r[1] + r[3] <= size.1 as f32,
                        "{lang:?} {size:?}: {hit:?} at {r:?} off screen"
                    );
                }
            }
        }
    }
    i18n::set_lang(i18n::Lang::En);
}

#[test]
fn a_picker_sheet_sets_what_it_takes_and_when_it_runs() {
    use mofact_core::command::{Compare, Condition};
    let mut f = Fixture::new();
    let mut w = f.snapshot.entities[2].clone();
    w.id = EntityId(7);
    w.building = "warehouse".into();
    w.pos = TilePos::new(8, 0);
    w.held = vec![("copper-plate".into(), 2), ("gear".into(), 300)];
    let mut p = w.clone();
    p.id = EntityId(8);
    p.building = "picker".into();
    p.pos = TilePos::new(10, 1);
    p.dir = Dir::East;
    p.held = Vec::new();
    f.snapshot.entities.push(w);
    f.snapshot.entities.push(p);
    f.hud.select(Some(EntityId(8)));
    for (size, scale) in [((1280, 720), 1.0), ((750, 1334), 2.0)] {
        let n = f.snapshot.entities.len() - 1;
        f.snapshot.entities[n].when = None;
        f.draw(size, scale, &Tool::Hand);
        // Anything, or one of what the warehouse behind it holds.
        f.find(&Hit::SetFilter(EntityId(8), None));
        f.find(&Hit::SetFilter(EntityId(8), Some("gear".into())));
        f.find(&Hit::SetCondition(EntityId(8), None));
        // "While at least" starts counting the store's first item.
        let at_least = Some(Condition {
            item: "gear".into(),
            compare: Compare::AtLeast,
            amount: 10,
        });
        assert_eq!(
            f.tap(centre(
                f.find(&Hit::SetCondition(EntityId(8), at_least.clone()))
            ))
            .1,
            vec![Action::Send(Command::SetCondition {
                entity: EntityId(8),
                when: at_least,
            })]
        );
        // With a condition: copper below 3, the item and amount steps show.
        let below = Condition {
            item: "copper-plate".into(),
            compare: Compare::Below,
            amount: 3,
        };
        f.snapshot.entities[n].when = Some(below.clone());
        f.hud.select(Some(EntityId(8)));
        f.draw(size, scale, &Tool::Hand);
        f.find(&Hit::SetCondition(
            EntityId(8),
            Some(Condition {
                item: "gear".into(),
                ..below.clone()
            }),
        ));
        let up = Hit::SetCondition(
            EntityId(8),
            Some(Condition {
                amount: 4,
                ..below.clone()
            }),
        );
        f.find(&up);
        // Stepping down stops at nothing, never below.
        f.find(&Hit::SetCondition(
            EntityId(8),
            Some(Condition {
                amount: 0,
                ..below.clone()
            }),
        ));
        f.hud.select(Some(EntityId(8)));
    }
}

#[test]
fn a_tools_tip_folds_to_its_name_and_a_tap_opens_it() {
    let mut f = Fixture::new();
    let tool = Tool::Place("furnace".into());
    f.draw((1080, 2340), 3.0, &tool);
    let open = f.find(&Hit::Tip);
    let (_, at) = f.hud.tip_seen.clone().expect("the tip names the tool");
    assert!(at.elapsed() < HINT_FOR, "a new tool's tip is open");
    // Later, it folds to the tool's name; a tap opens it again.
    f.hud.tip_seen = Some((tool.clone(), Instant::now().checked_sub(HINT_FOR).unwrap()));
    f.draw((1080, 2340), 3.0, &tool);
    let folded = f.find(&Hit::Tip);
    assert!(
        folded[2] < open[2] && folded[3] <= open[3],
        "{folded:?} vs {open:?}"
    );
    f.tap(centre(folded));
    assert!(f.hud.tip_open);
    f.draw((1080, 2340), 3.0, &tool);
    assert_eq!(f.find(&Hit::Tip), open);
    // Another tool starts fresh, folded once it has been read.
    f.draw((1080, 2340), 3.0, &Tool::Belt);
    assert!(!f.hud.tip_open);
}

#[test]
fn the_inspect_sheet_says_what_a_machine_makes_a_minute() {
    let mut f = Fixture::new();
    f.snapshot.entities[1].recipe = Some("gear".into());
    f.snapshot.entities[1].state = MachineState::Working;
    f.hud.select(Some(EntityId(2)));
    let words = |f: &mut Fixture| {
        f.overlay.keep_words = true;
        f.draw((1179, 2556), 3.0, &Tool::Hand);
        f.overlay.words.clone()
    };
    // A gear a second at the assembler's speed.
    assert!(words(&mut f).iter().any(|w| w == "Gear · 60/min"));
    // Four levels of a 5% crafting boost make it a fifth faster.
    f.snapshot.research.levels = vec![("lean-assembly".into(), 4)];
    assert!(words(&mut f).iter().any(|w| w == "Gear · 72/min"));
}

#[test]
fn settings_copied_from_one_machine_go_onto_others_like_it() {
    let mut f = Fixture::new();
    f.snapshot.entities[1].recipe = Some("gear".into());
    f.hud.select(Some(EntityId(2)));
    f.draw((1179, 2556), 3.0, &Tool::Hand);
    let brush = Brush::of(&f.content, &f.snapshot.entities[1]).expect("a recipe to copy");
    let copy = f.find(&Hit::CopySettings(Box::new(brush.clone())));
    assert_eq!(f.tap(centre(copy)).1, vec![Action::Tool(Tool::Hand)]);
    assert_eq!(f.hud.brush(), Some(&brush));
    assert_eq!(f.hud.selected(), None);

    // Another assembler on wire takes gear; one on gear already needs nothing.
    let mut other = f.snapshot.entities[1].clone();
    other.id = EntityId(7);
    other.recipe = Some("wire".into());
    assert_eq!(
        brush.commands(&f.content, &other),
        vec![Command::SetRecipe {
            entity: EntityId(7),
            recipe: Some("gear".into())
        }]
    );
    other.recipe = Some("gear".into());
    assert!(brush.commands(&f.content, &other).is_empty());
    assert!(brush.matches(&f.content, &other));
    // A belt, or a furnace that can't make gears, doesn't take it.
    let belt = f.snapshot.entities[2].clone();
    assert!(!brush.matches(&f.content, &belt));
    other.building = "furnace".into();
    assert!(!brush.matches(&f.content, &other));

    // The tip says what to tap, and Done puts the settings away.
    f.draw((1179, 2556), 3.0, &Tool::Hand);
    let done = f.find(&Hit::BrushDone);
    f.tap(centre(done));
    assert_eq!(f.hud.brush(), None);
    // Picking up any other tool does too.
    f.hud.brush = Some(brush);
    f.draw((1179, 2556), 3.0, &Tool::Belt);
    assert_eq!(f.hud.brush(), None);
}

#[test]
fn a_button_brings_the_camera_home_once_the_hub_is_out_of_sight() {
    let mut f = Fixture::new();
    f.draw((1179, 2556), 3.0, &Tool::Hand);
    assert!(!f.hud.hits.iter().any(|(_, h)| matches!(h, Hit::Home(_))));
    f.home = Some([1.5, 1.5]);
    f.draw((1179, 2556), 3.0, &Tool::Hand);
    let home = f.find(&Hit::Home([1.5, 1.5]));
    assert_eq!(f.tap(centre(home)).1, vec![Action::Focus([1.5, 1.5])]);
    // With a tool in hand that corner is the rotate button's.
    f.draw((1179, 2556), 3.0, &Tool::Belt);
    assert!(!f.hud.hits.iter().any(|(_, h)| matches!(h, Hit::Home(_))));
}
