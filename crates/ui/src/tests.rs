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
    }
}

struct Fixture {
    content: Content,
    look: Look,
    snapshot: Snapshot,
    overlay: Overlay,
    hud: Hud,
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
    let last = f.find(&Hit::Tool(5));
    assert!(first[1] > 2000.0, "portrait: along the bottom");
    assert_eq!(first[1], last[1]);
    assert!(last[0] > first[0]);

    f.draw((2340, 1080), 3.0, &Tool::Hand);
    let first = f.find(&Hit::Tool(0));
    let last = f.find(&Hit::Tool(5));
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
            for selected in [None, Some(EntityId(1)), Some(EntityId(2))] {
                let mut f = Fixture::new();
                f.hud.select(selected);
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
