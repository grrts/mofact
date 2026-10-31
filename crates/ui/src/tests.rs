use super::*;
use mofact_core::geom::TilePos;
use std::path::Path;

fn content() -> Content {
    mofact_content::load_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data"))
        .expect("the game data loads")
}

fn snapshot() -> Snapshot {
    let entity = |id, building: &str, x| EntityView {
        id: EntityId(id),
        building: building.into(),
        pos: TilePos::new(x, 0),
        dir: Dir::East,
        recipe: None,
        ore: None,
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
    }
}

struct Fixture {
    content: Content,
    look: Look,
    snapshot: Snapshot,
    overlay: Overlay,
    hud: Hud,
    belt: BuildingId,
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
        id: EntityId(9),
        building: "pole".into(),
        pos: TilePos::new(9, 0),
        dir: Dir::East,
        recipe: None,
        ore: None,
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
        .filter(|b| !matches!(b.role, BuildingRole::Hub | BuildingRole::Belt { .. }))
        .collect();
    assert_eq!(keys.len(), others.len());
    for (i, (k, b)) in keys.iter().enumerate() {
        assert!(!RESERVED_KEYS.contains(k), "{b} took {k}");
        assert!(keys[i + 1..].iter().all(|(o, _)| o != k), "{k} twice");
    }
    assert!(keys.contains(&('d', "drill".into())));
    assert!(keys.contains(&('l', "lab".into())));
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
