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
    let mut sim = circuit_chain();
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

fn circuit_chain() -> Sim {
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
    sim
}

#[test]
fn a_loaded_save_plays_on_exactly_like_the_original() {
    let mut sim = circuit_chain();
    let removed = sim.entity_at(TilePos::new(7, 13)).unwrap();
    assert!(matches!(
        sim.apply(&Command::Remove { entity: removed }),
        CommandResult::Applied { .. }
    ));
    run(&mut sim, 40);
    let bytes = sim.save();
    let (mut loaded, warnings) = Sim::load(&content(), &bytes).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        loaded.save(),
        bytes,
        "saving a loaded world gives the same bytes"
    );
    assert_eq!(
        loaded.entity_at(TilePos::new(8, 13)),
        sim.entity_at(TilePos::new(8, 13)),
        "ids kept"
    );
    run(&mut sim, 30);
    run(&mut loaded, 30);
    assert_eq!(loaded.snapshot(), sim.snapshot());
    assert_eq!(
        loaded.delivered(&item("circuit")),
        sim.delivered(&item("circuit"))
    );
    assert!(sim.delivered(&item("circuit")) > 0);
}

#[test]
fn damaged_and_future_saves_are_refused() {
    use crate::save::{LoadError, MAGIC};
    let c = content();
    let bytes = circuit_chain().save();
    assert_eq!(Sim::load(&c, b"PNG...").err(), Some(LoadError::NotASave));
    assert!(matches!(
        Sim::load(&c, &bytes[..bytes.len() / 2]),
        Err(LoadError::Corrupt(_))
    ));
    let mut future = MAGIC.to_vec();
    future.extend_from_slice(&99u16.to_le_bytes());
    assert_eq!(Sim::load(&c, &future).err(), Some(LoadError::TooNew(99)));
}

#[test]
fn saves_survive_content_changes() {
    let bytes = circuit_chain().save();
    let mut c = content();
    c.buildings.retain(|b| b.id != BuildingId::from("drill"));
    c.items.retain(|i| i.id != item("copper-ore"));
    let (sim, warnings) = Sim::load(&c, &bytes).unwrap();
    assert!(
        warnings.iter().any(|w| w.contains("`drill`")),
        "{warnings:?}"
    );
    assert!(
        warnings.iter().any(|w| w.contains("`copper-ore`")),
        "{warnings:?}"
    );
    assert!(
        sim.entity_at(TilePos::new(0, 10)).is_none(),
        "drills dropped"
    );
    assert!(
        sim.entity_at(TilePos::new(30, 9)).is_some(),
        "the hub stays"
    );
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

/// What undo must restore: every building, where it faces, an assembler's recipe, and the stock.
fn fingerprint(sim: &Sim) -> (Vec<String>, Vec<(ItemId, u32)>) {
    let snap = sim.snapshot();
    let mut ents: Vec<String> = snap
        .entities
        .iter()
        .map(|e| {
            let recipe = if e.building.as_str() == "assembler" {
                e.recipe.clone()
            } else {
                None
            };
            format!("{} {:?} {:?} {:?}", e.building, e.pos, e.dir, recipe)
        })
        .collect();
    ents.sort();
    (ents, snap.stock)
}

#[test]
fn do_then_undo_restores_state() {
    // Property test over many random action sequences of up to UNDO_STEPS actions.
    let buildings = ["belt", "drill", "furnace", "assembler"];
    let recipes = ["gear", "wire", "circuit"];
    for seed in 0..300u64 {
        let mut rng = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let mut next = move |n: u64| {
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (rng >> 33) % n
        };
        let mut sim = prototype_start();
        sim.add_stock(&item("copper-plate"), 1000);
        sim.add_stock(&item("iron-plate"), 1000);
        for x in 10..16 {
            sim.set_ore(TilePos::new(x, 20), &item("iron-ore"));
        }
        let before = fingerprint(&sim);
        let mut history = History::new();
        let mut applied = 0;
        while applied < UNDO_STEPS {
            let ids: Vec<EntityId> = sim
                .snapshot()
                .entities
                .iter()
                .filter(|e| e.building.as_str() != "hub")
                .map(|e| e.id)
                .collect();
            let pick = |n: u64| ids[n as usize % ids.len().max(1)];
            let cmd = match next(4) {
                0 | 1 => Command::Place {
                    building: BuildingId::from(buildings[next(4) as usize]),
                    pos: TilePos::new(10 + next(6) as i32, 18 + next(4) as i32),
                    dir: Dir::ALL[next(4) as usize],
                    recipe: None,
                },
                2 if !ids.is_empty() => Command::Remove {
                    entity: pick(next(1000)),
                },
                3 if !ids.is_empty() => Command::Rotate {
                    entity: pick(next(1000)),
                    dir: Dir::ALL[next(4) as usize],
                },
                _ if !ids.is_empty() => Command::SetRecipe {
                    entity: pick(next(1000)),
                    recipe: Some(RecipeId::from(recipes[next(3) as usize])),
                },
                _ => continue,
            };
            if matches!(history.apply(&mut sim, &cmd), CommandResult::Applied { .. }) {
                applied += 1;
            }
        }
        let after = fingerprint(&sim);
        for i in 0..applied {
            assert!(history.undo(&mut sim), "seed {seed}: undo {i} failed");
        }
        assert!(!history.can_undo());
        assert_eq!(fingerprint(&sim), before, "seed {seed}: undo all");
        for i in 0..applied {
            assert!(history.redo(&mut sim), "seed {seed}: redo {i} failed");
        }
        assert_eq!(fingerprint(&sim), after, "seed {seed}: redo all");
    }
}

#[test]
fn history_keeps_twenty_steps() {
    let mut sim = prototype_start();
    sim.add_stock(&item("iron-plate"), 1000);
    let mut history = History::new();
    for x in 0..25 {
        let cmd = Command::Place {
            building: BuildingId::from("belt"),
            pos: TilePos::new(x, 0),
            dir: Dir::East,
            recipe: None,
        };
        history.apply(&mut sim, &cmd);
    }
    let mut n = 0;
    while history.undo(&mut sim) {
        n += 1;
    }
    assert_eq!(n, UNDO_STEPS);
    assert!(
        sim.entity_at(TilePos::new(4, 0)).is_some(),
        "the oldest five stay"
    );
    assert!(sim.entity_at(TilePos::new(5, 0)).is_none());
}

#[test]
fn a_drill_reports_the_ore_under_it() {
    let sim = circuit_chain();
    let snap = sim.snapshot();
    let at = |x, y| {
        snap.entities
            .iter()
            .find(|e| e.pos == TilePos::new(x, y))
            .unwrap()
    };
    assert_eq!(at(0, 10).ore, Some(item("iron-ore")));
    assert_eq!(at(0, 12).ore, Some(item("copper-ore")));
    assert_eq!(at(3, 10).ore, None, "only miners have ore");
}
