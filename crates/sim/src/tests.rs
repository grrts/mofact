//! Scenario tests: the prototype's chains, run tick by tick.

use super::*;
use mofact_core::ids::{BuildingId, RecipeId};
use std::path::Path;

fn content() -> Content {
    mofact_content::load_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data"))
        .expect("data loads")
}

fn item(s: &str) -> ItemId {
    ItemId::from(s)
}

fn place(
    sim: &mut Sim,
    building: &str,
    x: i32,
    y: i32,
    dir: Dir,
    recipe: Option<&str>,
) -> EntityId {
    let cmd = Command::Place {
        building: BuildingId::from(building),
        pos: TilePos::new(x, y),
        dir,
        recipe: recipe.map(RecipeId::from),
    };
    match sim.apply(&cmd) {
        CommandResult::Applied {
            created: Some(id), ..
        } => id,
        other => panic!("placing {building} at {x},{y}: {other:?}"),
    }
}

fn belts(sim: &mut Sim, xs: std::ops::RangeInclusive<i32>, y: i32, dir: Dir) {
    for x in xs {
        place(sim, "belt", x, y, dir, None);
    }
}

fn run(sim: &mut Sim, seconds: u32) {
    for _ in 0..seconds * mofact_core::TICKS_PER_SECOND {
        sim.step();
    }
}

/// The prototype's new game: hub, and an iron line drill -> belt -> furnace -> belt -> hub.
fn prototype_start() -> Sim {
    let mut sim = Sim::with_content(&content());
    sim.add_stock(&item("iron-plate"), 60);
    sim.add_stock(&item("copper-plate"), 24);
    place(&mut sim, "hub", 26, 26, Dir::East, None);
    sim.set_ore(TilePos::new(19, 27), &item("iron-ore"));
    place(&mut sim, "drill", 19, 27, Dir::East, None);
    belts(&mut sim, 20..=21, 27, Dir::East);
    place(&mut sim, "furnace", 22, 27, Dir::East, None);
    belts(&mut sim, 23..=25, 27, Dir::East);
    sim
}

/// Items per minute delivered to the hub, measured over `minutes` after a warm-up minute.
fn rate(sim: &mut Sim, what: &str, minutes: u32) -> f64 {
    run(sim, 60);
    let before = sim.delivered(&item(what));
    run(sim, 60 * minutes);
    (sim.delivered(&item(what)) - before) as f64 / minutes as f64
}

#[test]
fn ticks_advance() {
    let mut sim = Sim::new();
    run(&mut sim, 1);
    assert_eq!(sim.snapshot().tick, 60);
}

#[test]
fn starter_line_smelts_fifty_plates_a_minute() {
    // One drill at 1.2 s per ore feeds one furnace at 1.2 s per plate: 50 a minute.
    let mut sim = prototype_start();
    let r = rate(&mut sim, "iron-plate", 3);
    assert!((49.0..=50.5).contains(&r), "{r} iron plates per minute");
}

#[test]
fn gear_assembler_runs_at_the_input_rate() {
    // 50 plates a minute into a gear assembler (2 plates, 1 s) gives 25 gears a minute.
    let mut sim = prototype_start();
    let furnace_out = sim.entity_at(TilePos::new(23, 27)).unwrap();
    let CommandResult::Applied { .. } = sim.apply(&Command::Remove {
        entity: furnace_out,
    }) else {
        panic!()
    };
    place(&mut sim, "assembler", 23, 27, Dir::East, Some("gear"));
    let r = rate(&mut sim, "gear", 3);
    assert!((24.0..=25.5).contains(&r), "{r} gears per minute");
    assert_eq!(
        sim.delivered(&item("iron-plate")),
        0,
        "every plate went into gears"
    );
}

#[test]
fn full_circuit_chain_runs_at_the_gear_rate() {
    // The prototype's last goal: gears and wire belted into one circuit assembler.
    // 25 gears a minute limit it to 25 circuits a minute; spare wire backs up.
    let mut sim = Sim::with_content(&content());
    sim.add_stock(&item("iron-plate"), 500);
    sim.add_stock(&item("copper-plate"), 100);
    place(&mut sim, "hub", 30, 9, Dir::East, None);
    for (y, ore, recipe) in [
        (10, "iron-ore", "gear"),
        (12, "copper-ore", "wire"),
        (13, "copper-ore", "wire"),
    ] {
        sim.set_ore(TilePos::new(0, y), &item(ore));
        place(&mut sim, "drill", 0, y, Dir::East, None);
        belts(&mut sim, 1..=2, y, Dir::East);
        place(&mut sim, "furnace", 3, y, Dir::East, None);
        place(&mut sim, "belt", 4, y, Dir::East, None);
        place(&mut sim, "assembler", 5, y, Dir::East, Some(recipe));
    }
    // Gears: east along y=10, then south into the circuit assembler's top side.
    belts(&mut sim, 6..=9, 10, Dir::East);
    place(&mut sim, "belt", 10, 10, Dir::South, None);
    // Wire: both lines merge going north at x=9, then east into its left side.
    belts(&mut sim, 6..=8, 12, Dir::East);
    belts(&mut sim, 6..=8, 13, Dir::East);
    place(&mut sim, "belt", 9, 13, Dir::North, None);
    place(&mut sim, "belt", 9, 12, Dir::North, None);
    place(&mut sim, "belt", 9, 11, Dir::East, None);
    place(&mut sim, "assembler", 10, 11, Dir::East, Some("circuit"));
    belts(&mut sim, 11..=29, 11, Dir::East);

    let r = rate(&mut sim, "circuit", 3);
    assert!((24.0..=25.5).contains(&r), "{r} circuits per minute");
    let wire_line = sim.snapshot();
    let starving = wire_line
        .entities
        .iter()
        .filter(|e| e.state == MachineState::Starved)
        .count();
    assert_eq!(starving, 0, "the gear-limited chain has nothing starving");
}

#[test]
fn removing_refunds_and_undo_restores() {
    let mut sim = prototype_start();
    let iron = sim.stock(&item("iron-plate"));
    let furnace = sim.entity_at(TilePos::new(22, 27)).unwrap();
    let CommandResult::Applied { undo, .. } = sim.apply(&Command::Remove { entity: furnace })
    else {
        panic!()
    };
    assert_eq!(
        sim.stock(&item("iron-plate")),
        iron + 5,
        "furnace cost refunded"
    );
    assert_eq!(sim.entity_at(TilePos::new(22, 27)), None);
    for c in &undo {
        assert!(matches!(sim.apply(c), CommandResult::Applied { .. }));
    }
    assert!(sim.entity_at(TilePos::new(22, 27)).is_some());
    assert_eq!(sim.stock(&item("iron-plate")), iron);
    assert!(
        rate(&mut sim, "iron-plate", 1) > 48.0,
        "the line works again"
    );
}

#[test]
fn refusals_change_nothing() {
    let mut sim = prototype_start();
    let before = sim.stock(&item("iron-plate"));
    let place_at = |x, b: &str| Command::Place {
        building: BuildingId::from(b),
        pos: TilePos::new(x, 27),
        dir: Dir::East,
        recipe: None,
    };
    assert_eq!(
        sim.apply(&place_at(22, "belt")),
        CommandResult::Refused(CommandError::Blocked)
    );
    assert_eq!(
        sim.apply(&place_at(40, "drill")),
        CommandResult::Refused(CommandError::Blocked),
        "no ore there"
    );
    assert_eq!(
        sim.apply(&place_at(40, "rocket")),
        CommandResult::Refused(CommandError::UnknownContent)
    );
    let hub = sim.entity_at(TilePos::new(27, 27)).unwrap();
    assert_eq!(
        sim.apply(&Command::Remove { entity: hub }),
        CommandResult::Refused(CommandError::Locked)
    );
    assert_eq!(sim.stock(&item("iron-plate")), before);
}

#[test]
fn ten_thousand_belt_tiles_tick_fast() {
    // 100 lines of 100 belts, each fed by a drill and backed up at the end.
    let mut sim = Sim::with_content(&content());
    sim.add_stock(&item("iron-plate"), 20_000);
    for y in 0..100 {
        sim.set_ore(TilePos::new(0, y), &item("iron-ore"));
        place(&mut sim, "drill", 0, y, Dir::East, None);
        belts(&mut sim, 1..=100, y, Dir::East);
    }
    run(&mut sim, 30);
    let ticks = 600;
    let start = std::time::Instant::now();
    for _ in 0..ticks {
        sim.step();
    }
    let per_tick = start.elapsed() / ticks;
    println!("10,100 entities: {per_tick:?} per tick");
    // The plan's target is 1 ms on desktop in release builds; debug test builds get 20x headroom.
    let limit_us = if cfg!(debug_assertions) {
        20_000
    } else {
        1_000
    };
    assert!(per_tick.as_micros() < limit_us, "{per_tick:?} per tick");
}
