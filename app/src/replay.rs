//! T20's check: replaying recorded input builds the prototype's starter chain.

use crate::built::Built;
use mofact_core::command::CommandResult;
use mofact_core::geom::TilePos;
use mofact_core::ids::{BuildingId, ItemId};
use mofact_input::Pointer::{self, *};
use mofact_input::{CameraControl, Controls, Tool, MOUSE};
use mofact_sim::Sim;
use std::path::Path;

/// Screen pixel at the centre of `tile`, with the camera centred on (0, 0) at
/// 32 px a tile on an 800 × 600 screen.
fn px(x: i32, y: i32) -> (f32, f32) {
    (
        400.0 + (x as f32 + 0.5) * 32.0,
        300.0 + (y as f32 + 0.5) * 32.0,
    )
}

fn tap(id: u64, x: i32, y: i32) -> Vec<Pointer> {
    let (x, y) = px(x, y);
    vec![Down { id, x, y }, Up { id }]
}

fn drag(id: u64, from: (i32, i32), to: (i32, i32)) -> Vec<Pointer> {
    let (a, b) = (px(from.0, from.1), px(to.0, to.1));
    vec![
        Down { id, x: a.0, y: a.1 },
        Move {
            id,
            x: (a.0 + b.0) / 2.0,
            y: (a.1 + b.1) / 2.0,
        },
        Move { id, x: b.0, y: b.1 },
        Up { id },
    ]
}

/// Play `events` with `tool`, applying every command to `sim` straight away.
fn play(
    sim: &mut Sim,
    controls: &mut Controls,
    built: &mut Built,
    tool: Tool,
    events: Vec<Pointer>,
) {
    controls.builder.set_tool(tool);
    for e in events {
        built.update(&sim.snapshot());
        for c in controls.handle(e, built) {
            let result = sim.apply(&c);
            assert!(
                matches!(result, CommandResult::Applied { .. }),
                "{c:?}: {result:?}"
            );
        }
        sim.step(); // a fresh tick so `built` sees the change
    }
}

#[test]
fn input_replay_builds_the_starter_chain() {
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data");
    let content = mofact_content::load_dir(&data).unwrap();
    let mut sim = Sim::with_content(&content);
    let item = |s: &str| ItemId::from(s);
    sim.add_stock(&item("iron-plate"), 60);
    sim.set_ore(TilePos::new(-3, 0), &item("iron-ore"));
    sim.apply(&mofact_core::command::Command::Place {
        building: BuildingId::from("hub"),
        pos: TilePos::new(4, -1),
        dir: mofact_core::geom::Dir::East,
        recipe: None,
    });

    let mut controls = Controls::new(CameraControl::new([0.0, 0.0], 32.0, [800.0, 600.0]));
    let mut built = Built::new(&content);
    // On a phone: finger 1 taps a drill on the ore, drags belts, taps a furnace,
    // drags belts into the hub. Then the mouse removes a stray belt and draws it again.
    play(
        &mut sim,
        &mut controls,
        &mut built,
        Tool::Place("drill".into()),
        tap(1, -3, 0),
    );
    play(
        &mut sim,
        &mut controls,
        &mut built,
        Tool::Belt,
        drag(1, (-2, 0), (-1, 0)),
    );
    play(
        &mut sim,
        &mut controls,
        &mut built,
        Tool::Place("furnace".into()),
        tap(1, 0, 0),
    );
    play(
        &mut sim,
        &mut controls,
        &mut built,
        Tool::Belt,
        drag(1, (1, 0), (3, 0)),
    );
    play(
        &mut sim,
        &mut controls,
        &mut built,
        Tool::Remove,
        tap(MOUSE, 2, 0),
    );
    assert!(sim.entity_at(TilePos::new(2, 0)).is_none());
    play(
        &mut sim,
        &mut controls,
        &mut built,
        Tool::Belt,
        drag(MOUSE, (2, 0), (3, 0)),
    );

    for _ in 0..3 * 60 * mofact_core::TICKS_PER_SECOND {
        sim.step();
    }
    let plates = sim.delivered(&item("iron-plate"));
    // 50 plates a minute once running; allow the first few seconds to fill the line.
    assert!(
        (140..=150).contains(&plates),
        "{plates} plates in 3 minutes"
    );
}

/// T51's check: a 20-machine block, selected with one drag, pasted three
/// times with a tap each, as fast as the fingers go: well under a minute.
#[test]
fn a_twenty_machine_block_pastes_three_times() {
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data");
    let content = mofact_content::load_dir(&data).unwrap();
    let mut sim = Sim::with_content(&content);
    for (item, n) in [("iron-plate", 2000), ("copper-plate", 2000)] {
        sim.add_stock(&ItemId::from(item), n);
    }
    // Two rows of ten assemblers making gears, with a belt between them.
    for x in -5..5 {
        for (y, dir) in [
            (-4, mofact_core::geom::Dir::South),
            (-2, mofact_core::geom::Dir::North),
        ] {
            let placed = sim.apply(&mofact_core::command::Command::Place {
                building: BuildingId::from("assembler"),
                pos: TilePos::new(x, y),
                dir,
                recipe: Some("gear".into()),
            });
            assert!(
                matches!(placed, CommandResult::Applied { .. }),
                "{placed:?}"
            );
        }
    }
    sim.step();
    let mut controls = Controls::new(CameraControl::new([0.0, 0.0], 32.0, [800.0, 600.0]));
    let mut built = Built::new(&content);
    play(
        &mut sim,
        &mut controls,
        &mut built,
        Tool::Select,
        drag(1, (-5, -4), (4, -2)),
    );
    assert_eq!(controls.builder.selection.len(), 20);

    let snap = sim.snapshot();
    let bp = mofact_input::Blueprint::capture(
        "Blueprint 1",
        snap.entities
            .iter()
            .filter(|e| controls.builder.selection.contains(&e.id))
            .map(|e| (e.building.clone(), e.pos, 1, e.dir, e.recipe.clone())),
    );
    assert_eq!(bp.size, (10, 3));
    controls.builder.hold(bp, TilePos::new(-5, -4), Vec::new());
    for row in [0, 4, 8] {
        // A tap puts the block's middle under the finger, Place builds it.
        play(
            &mut sim,
            &mut controls,
            &mut built,
            Tool::Paste,
            tap(1, 0, row + 1),
        );
        built.update(&sim.snapshot());
        assert!(controls.builder.blocked(&built).is_empty());
        for c in controls.builder.build_paste() {
            assert!(
                matches!(sim.apply(&c), CommandResult::Applied { .. }),
                "{c:?}"
            );
        }
        sim.step();
    }
    let assemblers = sim
        .snapshot()
        .entities
        .iter()
        .filter(|e| {
            e.building.as_str() == "assembler"
                && e.recipe.as_ref().is_some_and(|r| r.as_str() == "gear")
        })
        .count();
    assert_eq!(assemblers, 80);
}
