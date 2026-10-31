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

/// A world where every belt tier, splitters and underground belts are researched.
fn with_fast_belts() -> Sim {
    let mut sim = Sim::with_content(&content());
    for r in ["fast-belts", "logistics", "express-belts"] {
        sim.complete_research(&r.into());
    }
    sim
}

/// Ticks until the first ore crosses a line of `belt`s, `len` long, into the hub.
fn first_delivery(belt: &str, len: i32) -> u32 {
    let mut sim = with_fast_belts();
    sim.add_stock(&item("iron-plate"), 100);
    sim.add_stock(&item("gear"), 100);
    sim.add_stock(&item("steel"), 100);
    sim.set_ore(TilePos::new(0, 1), &item("iron-ore"));
    place(&mut sim, "drill", 0, 1, Dir::East, None);
    for x in 1..=len {
        place(&mut sim, belt, x, 1, Dir::East, None);
    }
    place(&mut sim, "hub", len + 1, 0, Dir::East, None);
    for t in 1..=20 * mofact_core::TICKS_PER_SECOND {
        sim.step();
        if sim.delivered(&item("iron-ore")) > 0 {
            return t;
        }
    }
    panic!("nothing arrived on {belt}");
}

#[test]
fn belt_tiers_move_items_at_their_speed() {
    let mine = ticks(1.2);
    let slow = first_delivery("belt", 12) - mine;
    let fast = first_delivery("fast-belt", 12) - mine;
    let express = first_delivery("express-belt", 12) - mine;
    // 12 tiles at 2, 4 and 6 tiles a second, give or take a tick per hand-over.
    assert!(slow.abs_diff(360) <= 14, "slow: {slow} ticks");
    assert!(fast.abs_diff(180) <= 14, "fast: {fast} ticks");
    assert!(express.abs_diff(120) <= 14, "express: {express} ticks");
}

#[test]
fn mixed_tiers_lose_nothing() {
    // drill -> fast -> slow -> fast -> hub: every ore mined arrives.
    let mut sim = with_fast_belts();
    sim.add_stock(&item("iron-plate"), 100);
    sim.add_stock(&item("gear"), 100);
    sim.set_ore(TilePos::new(0, 1), &item("iron-ore"));
    place(&mut sim, "drill", 0, 1, Dir::East, None);
    for x in 1..=12 {
        let belt = if (5..=8).contains(&x) {
            "belt"
        } else {
            "fast-belt"
        };
        place(&mut sim, belt, x, 1, Dir::East, None);
    }
    place(&mut sim, "hub", 13, 0, Dir::East, None);
    run(&mut sim, 60);
    let on_belts: u32 = (1..=12)
        .map(|x| {
            let id = sim.entity_at(TilePos::new(x, 1)).unwrap();
            sim.inventory(id).iter().map(|(_, n)| n).sum::<u32>()
        })
        .sum();
    let mined = 60 * mofact_core::TICKS_PER_SECOND / ticks(1.2);
    let arrived = sim.delivered(&item("iron-ore")) as u32;
    assert!(
        (arrived + on_belts).abs_diff(mined) <= 1,
        "{arrived} arrived and {on_belts} on belts of {mined} mined"
    );
}

#[test]
fn locked_buildings_and_recipes_are_refused_until_researched() {
    let mut sim = Sim::with_content(&content());
    sim.add_stock(&item("iron-plate"), 100);
    sim.add_stock(&item("copper-plate"), 100);
    sim.add_stock(&item("gear"), 100);
    let fast = Command::Place {
        building: BuildingId::from("fast-belt"),
        pos: TilePos::new(0, 0),
        dir: Dir::East,
        recipe: None,
    };
    assert_eq!(
        sim.apply(&fast),
        CommandResult::Refused(CommandError::Locked)
    );
    let asm = place(&mut sim, "assembler", 2, 0, Dir::East, None);
    let motor = Command::SetRecipe {
        entity: asm,
        recipe: Some(RecipeId::from("motor")),
    };
    assert_eq!(
        sim.apply(&motor),
        CommandResult::Refused(CommandError::Locked)
    );
    // Motors need steel working first.
    let pick = |r: &str| Command::SetResearch {
        research: Some(r.into()),
    };
    assert_eq!(
        sim.apply(&pick("motors")),
        CommandResult::Refused(CommandError::Locked)
    );
    sim.complete_research(&"steel-working".into());
    sim.complete_research(&"motors".into());
    assert!(matches!(sim.apply(&motor), CommandResult::Applied { .. }));
    assert_eq!(
        sim.apply(&pick("motors")),
        CommandResult::Refused(CommandError::Locked),
        "done research can't be picked again"
    );
    sim.complete_research(&"fast-belts".into());
    assert!(matches!(sim.apply(&fast), CommandResult::Applied { .. }));
}

/// A lab fed survey packs by a belt from an assembler, powered by a wind
/// turbine through a pole.
fn lab_line() -> Sim {
    let mut sim = Sim::with_content(&content());
    sim.add_stock(&item("iron-plate"), 200);
    sim.add_stock(&item("copper-plate"), 200);
    sim.add_stock(&item("brick"), 50);
    sim.add_stock(&item("gear"), 50);
    sim.add_stock(&item("circuit"), 50);
    // Packs are put straight into the assembler's input by its feeder belts:
    // copper plates and gears from the stock-fed hub would need a whole
    // factory, so this places full belts instead.
    let asm = place(&mut sim, "assembler", 0, 0, Dir::East, Some("survey-pack"));
    let _ = asm;
    belts(&mut sim, 1..=3, 0, Dir::East);
    place(&mut sim, "lab", 4, 0, Dir::East, None);
    sim.add_stock(&item("wire"), 20);
    place(&mut sim, "pole", 4, 1, Dir::East, None);
    place(&mut sim, "wind-turbine", 5, 1, Dir::East, None);
    sim
}

#[test]
fn labs_turn_packs_into_research() {
    let mut sim = lab_line();
    let pick = Command::SetResearch {
        research: Some("steel-working".into()),
    };
    assert!(matches!(sim.apply(&pick), CommandResult::Applied { .. }));
    // Feed the assembler by hand: 15 units of one pack each.
    let asm = sim.entity_at(TilePos::new(0, 0)).unwrap();
    for _ in 0..15 {
        sim.feed(asm, &item("copper-plate"));
        sim.feed(asm, &item("gear"));
        run(&mut sim, 6);
    }
    // Each unit takes the lab 10 s, so it is the slow end of the line.
    let r = sim.snapshot().research;
    assert!(
        r.current.is_some() && (1..15).contains(&r.progress.0),
        "{r:?}"
    );
    assert_eq!(r.progress.1, 15);
    run(&mut sim, 100);
    assert!(sim.research_done(&"steel-working".into()));
    let snap = sim.snapshot();
    assert_eq!(snap.research.done, vec!["steel-working".into()]);
    assert_eq!(snap.research.current, None, "labs stop when it's done");
    // Steel is open now.
    let asm2 = place(&mut sim, "furnace", 0, 3, Dir::East, Some("steel"));
    let _ = asm2;
}

#[test]
fn research_survives_a_save_and_old_saves_still_load() {
    let mut sim = lab_line();
    sim.complete_research(&"steel-working".into());
    let pick = Command::SetResearch {
        research: Some("motors".into()),
    };
    assert!(matches!(sim.apply(&pick), CommandResult::Applied { .. }));
    let bytes = sim.save();
    let (loaded, warnings) = Sim::load(&content(), &bytes).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(loaded.snapshot().research, sim.snapshot().research);
    assert_eq!(loaded.save(), bytes);

    let v1 = include_bytes!("../tests/v1.mofact");
    let (old, warnings) = Sim::load(&content(), v1).expect("a v1 save loads");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(!old.snapshot().entities.is_empty());
    assert!(old.snapshot().research.done.is_empty());
}

/// Ticks until `labs` labs, each fed two survey packs, finish their first
/// units of steel working on one wind turbine's grid.
fn first_units(labs: i32) -> (u64, u32) {
    let mut sim = Sim::with_content(&content());
    for (i, n) in [
        ("iron-plate", 100),
        ("gear", 100),
        ("wire", 100),
        ("brick", 100),
        ("circuit", 100),
    ] {
        sim.add_stock(&item(i), n);
    }
    place(&mut sim, "pole", 0, 0, Dir::East, None);
    place(&mut sim, "wind-turbine", 1, 0, Dir::East, None);
    let ids: Vec<_> = (0..labs)
        .map(|x| place(&mut sim, "lab", x, 1, Dir::East, None))
        .collect();
    for &lab in &ids {
        assert!(sim.feed(lab, &item("survey-pack")));
    }
    let pick = Command::SetResearch {
        research: Some("steel-working".into()),
    };
    assert!(matches!(sim.apply(&pick), CommandResult::Applied { .. }));
    let start = sim.snapshot().tick;
    while sim.snapshot().research.progress.0 == 0 {
        assert!(sim.snapshot().tick - start < 10_000, "labs never finish");
        sim.step();
    }
    (
        sim.snapshot().tick - start,
        sim.snapshot().research.progress.0,
    )
}

#[test]
fn a_short_grid_slows_machines() {
    // One turbine gives 60 kW: enough for one lab, half enough for two.
    let (one, units) = first_units(1);
    assert_eq!(units, 1);
    assert!(one.abs_diff(600) <= 2, "one lab at full power: {one} ticks");
    let (two, units) = first_units(2);
    assert_eq!(units, 2, "both labs finish together");
    assert!(
        two.abs_diff(1200) <= 4,
        "two labs at half power: {two} ticks"
    );
    let snap = {
        let mut sim = Sim::with_content(&content());
        sim.add_stock(&item("iron-plate"), 100);
        sim.add_stock(&item("gear"), 100);
        sim.add_stock(&item("wire"), 100);
        place(&mut sim, "pole", 0, 0, Dir::East, None);
        place(&mut sim, "wind-turbine", 1, 0, Dir::East, None);
        sim.step();
        sim.snapshot()
    };
    assert_eq!(snap.grids.len(), 1);
    assert_eq!(snap.grids[0].supply_kw, 60.0);
    assert_eq!(
        snap.grids[0].satisfaction, 1.0,
        "nothing asks, so all is met"
    );
}

#[test]
fn machines_off_the_grid_have_no_power() {
    let mut sim = Sim::with_content(&content());
    for (i, n) in [
        ("iron-plate", 100),
        ("gear", 100),
        ("wire", 100),
        ("brick", 100),
        ("circuit", 100),
    ] {
        sim.add_stock(&item(i), n);
    }
    let lab = place(&mut sim, "lab", 0, 0, Dir::East, None);
    sim.feed(lab, &item("survey-pack"));
    sim.apply(&Command::SetResearch {
        research: Some("steel-working".into()),
    });
    run(&mut sim, 20);
    let view = |sim: &Sim| {
        sim.snapshot()
            .entities
            .into_iter()
            .find(|e| e.id == lab)
            .unwrap()
    };
    assert_eq!(view(&sim).state, MachineState::NoPower);
    assert_eq!(view(&sim).grid, None);
    assert_eq!(view(&sim).progress, 0.0);

    // A pole with no generator links it, but gives nothing.
    place(&mut sim, "pole", 2, 0, Dir::East, None);
    run(&mut sim, 1);
    assert_eq!(view(&sim).grid, Some(0));
    assert_eq!(view(&sim).state, MachineState::NoPower);

    // A turbine far away on its own pole is another grid until a pole links them.
    place(&mut sim, "wind-turbine", 12, 0, Dir::East, None);
    place(&mut sim, "pole", 12, 1, Dir::East, None);
    run(&mut sim, 1);
    assert_eq!(sim.snapshot().grids.len(), 2);
    assert_eq!(view(&sim).state, MachineState::NoPower);
    place(&mut sim, "pole", 8, 0, Dir::East, None);
    run(&mut sim, 5);
    assert_eq!(sim.snapshot().grids.len(), 1);
    assert_eq!(view(&sim).state, MachineState::Working);
    assert!(view(&sim).progress > 0.0);

    // Power survives a save: the grids come back from where things stand.
    let bytes = sim.save();
    let (loaded, warnings) = Sim::load(&content(), &bytes).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(loaded.snapshot(), sim.snapshot());

    // Taking the turbine away stops the lab where it is.
    let turbine = sim.entity_at(TilePos::new(12, 0)).unwrap();
    sim.apply(&Command::Remove { entity: turbine });
    run(&mut sim, 1);
    let p = view(&sim).progress;
    run(&mut sim, 5);
    assert_eq!(view(&sim).progress, p);
    assert_eq!(view(&sim).state, MachineState::NoPower);
}

/// A world with splitters and underground belts researched and plenty in stock.
fn logistics() -> Sim {
    let mut sim = with_fast_belts();
    for (i, n) in [("iron-plate", 500), ("gear", 200), ("circuit", 100)] {
        sim.add_stock(&item(i), n);
    }
    sim
}

/// Items on belt tiles matching `on`.
fn items_on(sim: &Sim, on: impl Fn(TilePos) -> bool) -> usize {
    sim.snapshot()
        .belt_items
        .iter()
        .filter(|i| on(i.tile))
        .count()
}

#[test]
fn splitters_are_locked_until_logistics() {
    let mut sim = Sim::with_content(&content());
    sim.add_stock(&item("iron-plate"), 100);
    sim.add_stock(&item("circuit"), 100);
    let splitter = Command::Place {
        building: "splitter".into(),
        pos: TilePos::new(0, 0),
        dir: Dir::East,
        recipe: None,
    };
    assert_eq!(
        sim.apply(&splitter),
        CommandResult::Refused(CommandError::Locked)
    );
    sim.complete_research(&"logistics".into());
    assert!(matches!(
        sim.apply(&splitter),
        CommandResult::Applied { .. }
    ));
}

#[test]
fn two_rows_of_splitters_balance_four_lines() {
    let mut sim = logistics();
    // One line east into a splitter, which sends north and south into two
    // more; those send east and west: four lines of 13 belts.
    belts(&mut sim, 0..=2, 0, Dir::East);
    place(&mut sim, "splitter", 3, 0, Dir::East, None);
    belts(&mut sim, -2..=-1, 0, Dir::East);
    for y in [-1, -2] {
        place(&mut sim, "belt", 3, y, Dir::North, None);
    }
    for y in [1, 2] {
        place(&mut sim, "belt", 3, y, Dir::South, None);
    }
    place(&mut sim, "splitter", 3, -3, Dir::North, None);
    place(&mut sim, "splitter", 3, 3, Dir::South, None);
    for y in [-3, 3] {
        belts(&mut sim, -10..=2, y, Dir::West);
        belts(&mut sim, 4..=16, y, Dir::East);
    }
    let first = sim.entity_at(TilePos::new(-2, 0)).unwrap();
    for _ in 0..40 {
        assert!(sim.feed(first, &item("iron-plate")));
        run(&mut sim, 1);
    }
    run(&mut sim, 20);
    let lines = [
        items_on(&sim, |t| t.y == -3 && t.x < 3),
        items_on(&sim, |t| t.y == -3 && t.x > 3),
        items_on(&sim, |t| t.y == 3 && t.x < 3),
        items_on(&sim, |t| t.y == 3 && t.x > 3),
    ];
    assert_eq!(lines, [10; 4], "40 plates split four ways");
}

#[test]
fn underground_belts_pass_under_buildings() {
    let mut sim = logistics();
    belts(&mut sim, 0..=1, 0, Dir::East);
    let entrance = place(&mut sim, "underground-belt", 2, 0, Dir::East, None);
    // Something in the way above.
    place(&mut sim, "furnace", 4, 0, Dir::East, None);
    let exit = place(&mut sim, "underground-belt", 6, 0, Dir::East, None);
    belts(&mut sim, 7..=12, 0, Dir::East);
    let view = |sim: &Sim, id| {
        sim.snapshot()
            .entities
            .into_iter()
            .find(|e| e.id == id)
            .unwrap()
    };
    assert!(!view(&sim, entrance).exit);
    assert!(view(&sim, exit).exit);

    let first = sim.entity_at(TilePos::new(0, 0)).unwrap();
    for _ in 0..5 {
        sim.feed(first, &item("iron-plate"));
        run(&mut sim, 1);
    }
    run(&mut sim, 10);
    assert_eq!(items_on(&sim, |t| t.x >= 7), 5, "all five came out");
    assert_eq!(
        sim.inventory(sim.entity_at(TilePos::new(4, 0)).unwrap()),
        vec![]
    );

    // Taking the exit away leaves the entrance holding what arrives; putting
    // it back pairs them again, and the pair survives a save.
    sim.apply(&Command::Remove { entity: exit });
    for _ in 0..2 {
        sim.feed(first, &item("iron-plate"));
        run(&mut sim, 1);
    }
    run(&mut sim, 5);
    assert_eq!(items_on(&sim, |t| t.x >= 7), 5);
    let exit = place(&mut sim, "underground-belt", 6, 0, Dir::East, None);
    let bytes = sim.save();
    let (mut loaded, warnings) = Sim::load(&content(), &bytes).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(loaded.save(), bytes);
    assert!(view(&loaded, exit).exit);
    run(&mut loaded, 10);
    assert_eq!(items_on(&loaded, |t| t.x >= 7), 7);

    // Too far apart, two ends don't pair.
    let far = place(&mut sim, "underground-belt", 0, 5, Dir::East, None);
    let other = place(&mut sim, "underground-belt", 7, 5, Dir::East, None);
    assert!(!view(&sim, far).exit && !view(&sim, other).exit);
}
