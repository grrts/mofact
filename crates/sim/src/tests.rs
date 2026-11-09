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
        CommandResult::Refused(CommandError::NoOre),
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
fn one_undo_reverses_a_whole_drag_even_after_a_redo() {
    let mut sim = prototype_start();
    sim.add_stock(&item("iron-plate"), 1000);
    let mut history = History::new();
    let belts: Vec<_> = (0..5)
        .map(|x| Command::Place {
            building: BuildingId::from("belt"),
            pos: TilePos::new(x, 40),
            dir: Dir::East,
            recipe: None,
        })
        .collect();
    let before = sim.snapshot().entities.len();
    history.apply_all(&mut sim, &belts);
    assert_eq!(sim.snapshot().entities.len(), before + 5);
    // Remove the whole line in one sweep, then turn one of them back.
    let ids: Vec<EntityId> = (0..5)
        .map(|x| sim.entity_at(TilePos::new(x, 40)).unwrap())
        .collect();
    let sweep: Vec<_> = ids
        .iter()
        .map(|&entity| Command::Remove { entity })
        .collect();
    history.apply_all(&mut sim, &sweep);
    assert_eq!(sim.snapshot().entities.len(), before);
    assert!(history.undo(&mut sim), "the sweep comes back in one step");
    assert_eq!(sim.snapshot().entities.len(), before + 5);
    assert!(history.redo(&mut sim));
    assert!(history.undo(&mut sim));
    // The revived belts have new ids; undoing the drag must still find them.
    assert!(history.undo(&mut sim), "undo the drag");
    assert_eq!(sim.snapshot().entities.len(), before);
    assert!(!history.can_undo());
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

/// Fluid handling researched and plenty to build with.
fn plumbing(content: &Content) -> Sim {
    let mut sim = Sim::with_content(content);
    for r in ["steel-working", "motors", "fluid-handling", "steam-power"] {
        sim.complete_research(&r.into());
    }
    for i in [
        "iron-plate",
        "copper-plate",
        "gear",
        "wire",
        "motor",
        "brick",
        "steel",
        "circuit",
    ] {
        sim.add_stock(&item(i), 100_000);
    }
    sim
}

/// A powered pole at `x`,`y` with `turbines` wind turbines beside it.
fn power_at(sim: &mut Sim, x: i32, y: i32, turbines: i32) {
    place(sim, "pole", x, y, Dir::East, None);
    for t in 0..turbines {
        place(sim, "wind-turbine", x - 1 + 2 * t, y, Dir::East, None);
    }
}

/// Water in the tank at the end of a well, `len` tiles of pipe and a pump.
fn water_through(len: i32, seconds: u32) -> u32 {
    let mut sim = plumbing(&content());
    place(&mut sim, "water-well", 0, 0, Dir::East, None);
    power_at(&mut sim, 0, 2, 2);
    for x in 1..=len {
        place(&mut sim, "pipe", x, 0, Dir::East, None);
    }
    place(&mut sim, "pump", len + 1, 0, Dir::East, None);
    power_at(&mut sim, len + 1, 2, 1);
    let tank = place(&mut sim, "tank", len + 2, 0, Dir::East, None);
    run(&mut sim, 1); // let the line fill (the tank holds 2500)
    let before = sim.inventory(tank).first().map_or(0, |s| s.1);
    run(&mut sim, seconds);
    sim.inventory(tank).first().map_or(0, |s| s.1) - before
}

#[test]
fn a_long_pipe_delivers_less() {
    let short = water_through(2, 2);
    let long = water_through(200, 2);
    // The well gives 600 a second; two tiles of pipe pass all of it.
    assert!((1150..=1250).contains(&short), "short line: {short}");
    // 200 tiles let out about 110 a second.
    assert!((180..=260).contains(&long), "long line: {long}");
}

#[test]
fn a_pump_restores_flow_on_a_long_line() {
    let mut sim = plumbing(&content());
    place(&mut sim, "water-well", 0, 0, Dir::East, None);
    power_at(&mut sim, 0, 2, 2);
    for x in 1..=200 {
        // A pump every 40 tiles splits the line into short segments.
        let pump = x % 40 == 0 && x < 200;
        let b = if pump { "pump" } else { "pipe" };
        place(&mut sim, b, x, 0, Dir::East, None);
        if pump {
            power_at(&mut sim, x, 2, 1);
        }
    }
    place(&mut sim, "pump", 201, 0, Dir::East, None);
    power_at(&mut sim, 201, 2, 1);
    let tank = place(&mut sim, "tank", 202, 0, Dir::East, None);
    run(&mut sim, 2);
    let before = sim.inventory(tank).first().map_or(0, |s| s.1);
    run(&mut sim, 2);
    let got = sim.inventory(tank).first().map_or(0, |s| s.1) - before;
    assert!(got > 600, "pumped line: {got}");
}

#[test]
fn pipes_never_mix_two_fluids() {
    let mut sim = plumbing(&content());
    sim.complete_research(&"oil-processing".into());
    power_at(&mut sim, 0, -2, 3);
    place(&mut sim, "oil-well", 0, 0, Dir::East, None);
    place(&mut sim, "pipe", 1, 0, Dir::East, None);
    place(&mut sim, "water-well", 4, 0, Dir::East, None);
    power_at(&mut sim, 4, 2, 2);
    place(&mut sim, "pipe", 3, 0, Dir::East, None);
    run(&mut sim, 1);
    let plates = sim.stock(&item("iron-plate"));
    let joined = sim.apply(&Command::Place {
        building: "pipe".into(),
        pos: TilePos::new(2, 0),
        dir: Dir::East,
        recipe: None,
    });
    assert_eq!(joined, CommandResult::Refused(CommandError::FluidsMix));
    assert_eq!(sim.stock(&item("iron-plate")), plates, "nothing paid");
    assert_eq!(sim.entity_at(TilePos::new(2, 0)), None);
    // A pump between them is fine: it can't push oil into water.
    place(&mut sim, "pump", 2, 0, Dir::East, None);
    run(&mut sim, 1);
    let water = sim.entity_at(TilePos::new(3, 0)).unwrap();
    assert_eq!(sim.inventory(water)[0].0, item("water"));
}

#[test]
fn underground_pipes_join_their_ends() {
    let mut sim = plumbing(&content());
    place(&mut sim, "water-well", 0, 0, Dir::East, None);
    power_at(&mut sim, 0, 2, 2);
    place(&mut sim, "underground-pipe", 1, 0, Dir::East, None);
    place(&mut sim, "furnace", 3, 0, Dir::East, None); // in the way above ground
    place(&mut sim, "underground-pipe", 8, 0, Dir::East, None);
    let end = place(&mut sim, "pipe", 9, 0, Dir::East, None);
    run(&mut sim, 1);
    assert_eq!(sim.inventory(end)[0].0, item("water"));
    let snap = sim.snapshot();
    let view = snap.entities.iter().find(|e| e.id == end).unwrap();
    assert!(view.fluid.as_ref().is_some_and(|f| f.fill > 0.0));
    assert_eq!(
        view.links,
        fluids::bit(Dir::West),
        "joined to the exit only"
    );
}

#[test]
fn fluid_survives_saves_and_rebuilds() {
    let mut sim = plumbing(&content());
    place(&mut sim, "water-well", 0, 0, Dir::East, None);
    power_at(&mut sim, 0, 2, 2);
    for x in 1..=4 {
        place(&mut sim, "pipe", x, 0, Dir::East, None);
    }
    let tank = place(&mut sim, "tank", 5, 0, Dir::East, None);
    run(&mut sim, 1);
    let well = sim.entity_at(TilePos::new(0, 0)).unwrap();
    sim.apply(&Command::Remove { entity: well });
    let held = sim.inventory(tank)[0].1;
    assert!(held > 400, "{held}");
    let (loaded, warnings) = Sim::load(&content(), &sim.save()).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let after = loaded.inventory(tank)[0].1;
    assert!(held.abs_diff(after) <= 1, "{held} then {after}");
    // Cutting the line keeps what each side held, less the cut pipe's share
    // (100 of the line's 2900).
    let cut = sim.entity_at(TilePos::new(2, 0)).unwrap();
    sim.apply(&Command::Remove { entity: cut });
    let left = sim.inventory(sim.entity_at(TilePos::new(1, 0)).unwrap())[0].1;
    let right = sim.inventory(tank)[0].1;
    assert!(
        left + right + 2 >= held - held / 25,
        "{left} + {right} of {held}"
    );
}

/// Kilowatts the first grid gives, averaged over `seconds` while a coal
/// goes into `boiler` every second.
fn steam_kw(sim: &mut Sim, boiler: EntityId, seconds: u32) -> f32 {
    let mut total = 0.0;
    for _ in 0..seconds {
        sim.feed(boiler, &item("coal"));
        run(sim, 1);
        total += sim.snapshot().grids[0].supply_kw;
    }
    total / seconds as f32
}

#[test]
fn a_steam_engine_gives_less_when_the_water_runs_short() {
    let mut sim = plumbing(&content());
    // Water into a boiler facing east, steam out to an engine.
    let well = place(&mut sim, "water-well", 0, 0, Dir::East, None);
    let feed = place(&mut sim, "pipe", 1, 0, Dir::East, None);
    let boiler = place(&mut sim, "boiler", 2, 0, Dir::East, None);
    place(&mut sim, "pipe", 3, 0, Dir::East, None);
    let engine = place(&mut sim, "steam-engine", 4, 0, Dir::East, None);
    // One turbine to start the well, and more wells than it can run, so
    // the grid wants all the engine can give.
    place(&mut sim, "pole", 2, 2, Dir::East, None);
    place(&mut sim, "wind-turbine", 3, 2, Dir::East, None);
    for x in 0..4 {
        place(&mut sim, "water-well", x, 4, Dir::East, None);
    }
    let warming = steam_kw(&mut sim, boiler, 10);
    let full = steam_kw(&mut sim, boiler, 10);
    assert!(
        full > 300.0,
        "wind and steam together: {full} kW (warming up: {warming})"
    );
    let view = |sim: &Sim| {
        sim.snapshot()
            .entities
            .into_iter()
            .find(|e| e.id == engine)
            .unwrap()
    };
    assert_eq!(view(&sim).state, MachineState::Working);

    // Cut the water: the engine runs dry and the grid is back to the wind.
    sim.apply(&Command::Remove { entity: feed });
    sim.apply(&Command::Remove { entity: well });
    // What's already in the boiler and the steam pipe lasts a while.
    run(&mut sim, 20);
    let dry = steam_kw(&mut sim, boiler, 5);
    assert!(dry < 61.0, "only the wind left: {dry} kW");
    assert_eq!(view(&sim).state, MachineState::Starved);

    // An engine saves what it gave on its last tick.
    let (loaded, warnings) = Sim::load(&content(), &sim.save()).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(loaded.save(), sim.save());
}

#[test]
fn engines_burn_only_for_what_the_wind_does_not_give() {
    let mut sim = plumbing(&content());
    place(&mut sim, "water-well", 0, 0, Dir::East, None);
    place(&mut sim, "pipe", 1, 0, Dir::East, None);
    let boiler = place(&mut sim, "boiler", 2, 0, Dir::East, None);
    place(&mut sim, "pipe", 3, 0, Dir::East, None);
    place(&mut sim, "steam-engine", 4, 0, Dir::East, None);
    place(&mut sim, "pole", 2, 2, Dir::East, None);
    place(&mut sim, "wind-turbine", 3, 2, Dir::East, None);
    // The well asks 90 kW; the turbine gives 60, so steam makes up 30.
    let warming = steam_kw(&mut sim, boiler, 20);
    let snap = sim.snapshot();
    let g = &snap.grids[0];
    assert!(
        (85.0..=100.0).contains(&g.supply_kw),
        "{warming} then {g:?}"
    );
}

#[test]
fn goals_come_one_at_a_time_and_pay_out_when_met() {
    let mut sim = Sim::with_content(&content());
    sim.add_stock(&item("iron-plate"), 1000);
    let hub = place(&mut sim, "hub", 0, 0, Dir::East, None);
    let goal = |sim: &Sim| sim.snapshot().goal.unwrap();
    assert_eq!(
        goal(&sim),
        GoalView {
            index: 0,
            have: 0,
            need: 50
        }
    );
    for _ in 0..49 {
        sim.feed(hub, &item("iron-plate"));
    }
    run(&mut sim, 1);
    assert_eq!(goal(&sim).have, 49);
    sim.feed(hub, &item("iron-plate"));
    run(&mut sim, 1);
    // On to three drills; the first goal pays nothing.
    assert_eq!(
        goal(&sim),
        GoalView {
            index: 1,
            have: 0,
            need: 3
        }
    );
    // Drills built before their goal still count.
    let plates = sim.stock(&item("iron-plate"));
    for x in 0..3 {
        sim.set_ore(TilePos::new(10 + x * 2, 0), &item("iron-ore"));
        place(&mut sim, "drill", 10 + x * 2, 0, Dir::East, None);
    }
    let cost = plates - sim.stock(&item("iron-plate"));
    run(&mut sim, 1);
    assert_eq!(goal(&sim).index, 2);
    assert_eq!(
        sim.stock(&item("iron-plate")),
        plates - cost + 30,
        "the drills goal gives 30 plates"
    );
    // The goal survives a save.
    let (loaded, warnings) = Sim::load(&content(), &sim.save()).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(loaded.goal(), 2);
    assert_eq!(loaded.save(), sim.save());
}

/// What a tank or pipe holds, in whole units.
fn held(sim: &Sim, id: EntityId) -> u32 {
    sim.inventory(id).first().map_or(0, |s| s.1)
}

fn state_of(sim: &Sim, id: EntityId) -> MachineState {
    sim.snapshot()
        .entities
        .into_iter()
        .find(|e| e.id == id)
        .unwrap()
        .state
}

#[test]
fn a_refinery_stops_when_one_output_backs_up() {
    let mut sim = plumbing(&content());
    sim.complete_research(&"oil-processing".into());
    // Crude in from the west; the refinery faces east, so light oil goes
    // out its front into a tank, and tar out its right into one pipe.
    place(&mut sim, "oil-well", 0, 0, Dir::East, None);
    place(&mut sim, "pipe", 1, 0, Dir::East, None);
    let refinery = place(&mut sim, "refinery", 2, 0, Dir::East, None);
    place(&mut sim, "pipe", 4, 0, Dir::East, None);
    let light = place(&mut sim, "tank", 5, 0, Dir::East, None);
    let tar = place(&mut sim, "pipe", 2, 2, Dir::East, None);
    place(&mut sim, "pole", 0, 2, Dir::East, None);
    for (x, y) in [(-2, 3), (-1, 3), (0, 3), (1, 3), (-2, 4), (-1, 4), (0, 4)] {
        place(&mut sim, "wind-turbine", x, y, Dir::East, None);
    }

    // It sets itself to refining, and makes both at once.
    run(&mut sim, 12);
    assert_eq!(state_of(&sim, refinery), MachineState::Working);
    assert!(held(&sim, light) > 0, "no light oil");
    assert!(held(&sim, tar) > 0, "no tar");

    // One pipe of tar fills after a few crafts, and with it the refinery
    // stops, though the light oil tank has plenty of room.
    run(&mut sim, 40);
    assert_eq!(state_of(&sim, refinery), MachineState::Blocked);
    let before = held(&sim, light);
    assert!(before < 1000, "light oil tank: {before}");
    run(&mut sim, 10);
    assert_eq!(held(&sim, light), before, "light oil kept coming");
    let names: Vec<_> = sim.inventory(refinery).into_iter().map(|s| s.0).collect();
    assert!(names.contains(&item("tar")), "{names:?}");

    // Both outputs survive a save.
    let (loaded, warnings) = Sim::load(&content(), &sim.save()).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(loaded.inventory(refinery), sim.inventory(refinery));
    assert_eq!(state_of(&loaded, refinery), MachineState::Blocked);

    // Give the tar somewhere to go and the light oil flows again.
    place(&mut sim, "tank", 2, 3, Dir::East, None);
    run(&mut sim, 12);
    assert_ne!(state_of(&sim, refinery), MachineState::Blocked);
    assert!(held(&sim, light) > before, "still stopped");
}

#[test]
fn tar_from_a_refinery_cracks_into_more_light_oil() {
    let mut sim = plumbing(&content());
    sim.complete_research(&"oil-processing".into());
    place(&mut sim, "oil-well", 0, 0, Dir::East, None);
    place(&mut sim, "pipe", 1, 0, Dir::East, None);
    place(&mut sim, "refinery", 2, 0, Dir::East, None);
    let light = place(&mut sim, "tank", 4, 0, Dir::East, None);
    // Tar goes out the refinery's right into a chemical plant facing
    // south, which takes water from the west and sends light oil south.
    place(&mut sim, "pipe", 2, 2, Dir::East, None);
    let plant = place(
        &mut sim,
        "chemical-plant",
        2,
        3,
        Dir::South,
        Some("cracking"),
    );
    place(&mut sim, "water-well", 0, 3, Dir::East, None);
    place(&mut sim, "pipe", 1, 3, Dir::East, None);
    let cracked = place(&mut sim, "tank", 2, 4, Dir::East, None);
    place(&mut sim, "pole", 0, 1, Dir::East, None);
    for x in -2..=-1 {
        for y in -1..=3 {
            place(&mut sim, "wind-turbine", x, y, Dir::East, None);
        }
    }
    run(&mut sim, 60);
    assert!(held(&sim, light) > 0, "no light oil from the refinery");
    assert!(held(&sim, cracked) > 0, "no light oil from the tar");
    assert_ne!(state_of(&sim, plant), MachineState::Blocked);
}

#[test]
fn drills_go_on_every_kind_of_generated_ore() {
    // The map the player sees is generated; drills must work on all of it,
    // not only the starter patches a scenario places by hand.
    let content = content();
    let mut sim = Sim::with_content(&content);
    sim.add_stock(&item("iron-plate"), 10_000);
    sim.set_world(42);
    let gen = mofact_world::WorldGen::new(42, sim.tables.ores.len() as u8);
    let mut found = std::collections::HashMap::new();
    'search: for y in -400..400 {
        for x in -400..400 {
            let t = TilePos::new(x, y);
            if let Some(o) = mofact_world::ore_at(&gen, t) {
                found.entry(o.resource).or_insert(t);
                if found.len() == sim.tables.ores.len() {
                    break 'search;
                }
            }
        }
    }
    assert_eq!(found.len(), 4, "iron, copper, stone and coal all generate");
    for (resource, t) in found {
        let ore = sim.tables.items[sim.tables.ores[resource as usize] as usize].clone();
        assert_eq!(sim.ore_at(t), Some(ore.clone()));
        let drill = place(&mut sim, "drill", t.x, t.y, Dir::East, None);
        run(&mut sim, 2);
        assert_eq!(
            sim.inventory(drill),
            vec![(ore.clone(), 1)],
            "a drill on {ore:?} at {t:?} mines it"
        );
    }
}

#[test]
fn water_and_cliffs_block_and_trees_block_until_cleared() {
    let content = content();
    let mut sim = Sim::with_content(&content);
    sim.add_stock(&item("iron-plate"), 10_000);
    sim.set_world(42);
    let gen = mofact_world::WorldGen::new(42, sim.tables.ores.len() as u8);
    let find = |want: &dyn Fn(mofact_world::Tile) -> bool| {
        (-300..300)
            .flat_map(|y| (-300..300).map(move |x| TilePos::new(x, y)))
            .find(|&t| want(mofact_world::tile_at(&gen, t)))
            .expect("the map has one")
    };
    let belt = |sim: &mut Sim, t: TilePos| {
        sim.apply(&Command::Place {
            building: BuildingId::from("belt"),
            pos: t,
            dir: Dir::East,
            recipe: None,
        })
    };
    let water = find(&|t| t.terrain.is_water());
    let cliff = find(&|t| t.terrain == mofact_world::Terrain::Cliff);
    for t in [water, cliff] {
        assert_eq!(
            belt(&mut sim, t),
            CommandResult::Refused(CommandError::Terrain),
            "{t:?}"
        );
    }
    let tree = find(&|t| t.decor == Some(mofact_world::Decor::Tree));
    assert_eq!(
        belt(&mut sim, tree),
        CommandResult::Refused(CommandError::Overgrown)
    );
    // Clear it, and the undo puts it back.
    let CommandResult::Applied { undo, .. } = sim.apply(&Command::Clear { tile: tree }) else {
        panic!("clearing a tree")
    };
    assert_eq!(undo, vec![Command::Regrow { tile: tree }]);
    assert_eq!(*sim.snapshot().cleared, vec![tree]);
    assert!(matches!(
        sim.apply(&Command::Clear { tile: tree }),
        CommandResult::Refused(_)
    ));
    // Cleared ground saves, loads, and takes a belt.
    let (mut loaded, _) = Sim::load(&content, &sim.save()).unwrap();
    loaded.set_world(42);
    assert_eq!(*loaded.snapshot().cleared, vec![tree]);
    assert!(matches!(
        belt(&mut loaded, tree),
        CommandResult::Applied { .. }
    ));
    assert!(matches!(
        sim.apply(&Command::Regrow { tile: tree }),
        CommandResult::Applied { .. }
    ));
    assert!(sim.snapshot().cleared.is_empty());
    // Open grass takes it straight away.
    let grass = find(&|t| t.terrain == mofact_world::Terrain::Grass && t.decor.is_none());
    assert!(matches!(
        belt(&mut sim, grass),
        CommandResult::Applied { .. }
    ));
}

#[test]
fn drills_run_patches_dry_and_saves_keep_what_is_left() {
    let content = content();
    let mut sim = Sim::with_content(&content);
    sim.add_stock(&item("iron-plate"), 10_000);
    let t = TilePos::new(0, 0);
    sim.set_ore_amount(t, &item("iron-ore"), 3);
    let drill = place(&mut sim, "drill", t.x, t.y, Dir::East, None);
    for x in 1..=4 {
        place(&mut sim, "belt", x, 0, Dir::East, None);
    }
    run(&mut sim, 3);
    let left = sim.ore_left(t).expect("ore still under the drill");
    assert!(left < 3, "the drill mined some");
    // A save part way through keeps the amount left.
    let (loaded, _) = Sim::load(&content, &sim.save()).unwrap();
    assert_eq!(loaded.ore_left(t), Some(left));
    run(&mut sim, 40);
    assert_eq!(sim.ore_left(t), None, "the tile ran dry");
    assert_eq!(*sim.snapshot().depleted, vec![t]);
    assert!(sim.inventory(drill).is_empty(), "nothing left to hand on");
    assert_eq!(state_of(&sim, drill), MachineState::Starved);
    assert_eq!(sim.ore_at(t), None);
    let (loaded, _) = Sim::load(&content, &sim.save()).unwrap();
    assert_eq!(*loaded.snapshot().depleted, vec![t]);
}

#[test]
fn version_9_saves_from_before_ore_ran_out_still_load() {
    // Written by the build before ore amounts were saved.
    let content = content();
    let (sim, warnings) = Sim::load(&content, include_bytes!("../tests/v9.mofact")).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(sim.snapshot().entities.len(), 1, "the drill is still there");
    assert_eq!(sim.ore_left(TilePos::new(0, 0)), Some(PLACED_ORE));
    assert!(sim.snapshot().depleted.is_empty());
}

#[test]
fn pins_go_on_the_map_undo_and_save() {
    let content = content();
    let mut sim = Sim::with_content(&content);
    let a = TilePos::new(400, -30);
    let CommandResult::Applied { undo, .. } = sim.apply(&Command::Pin { tile: a }) else {
        panic!("pinning a tile")
    };
    assert_eq!(undo, vec![Command::Unpin { tile: a }]);
    assert_eq!(*sim.snapshot().pins, vec![a]);
    assert!(matches!(
        sim.apply(&Command::Pin { tile: a }),
        CommandResult::Refused(_)
    ));
    let (loaded, _) = Sim::load(&content, &sim.save()).unwrap();
    assert_eq!(*loaded.snapshot().pins, vec![a]);
    assert!(matches!(
        sim.apply(&Command::Unpin { tile: a }),
        CommandResult::Applied { .. }
    ));
    assert!(sim.snapshot().pins.is_empty());
    for i in 0..MAX_PINS as i32 + 3 {
        sim.apply(&Command::Pin {
            tile: TilePos::new(i, 0),
        });
    }
    assert_eq!(sim.snapshot().pins.len(), MAX_PINS, "the map holds so many");
}

#[test]
fn version_11_saves_from_before_pins_still_load() {
    let content = content();
    let (sim, warnings) = Sim::load(&content, include_bytes!("../tests/v11.mofact")).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(sim.snapshot().entities.len(), 1);
    let left = sim.ore_left(TilePos::new(0, 0)).expect("ore still there");
    assert!(
        left < 50,
        "the amount mined before the save is kept: {left}"
    );
    assert!(sim.snapshot().pins.is_empty());
}

#[test]
fn only_drills_need_ore() {
    // Every other building goes on open ground with no ore under it.
    let content = content();
    let mut sim = Sim::with_content(&content);
    for i in &content.items {
        sim.add_stock(&i.id, 10_000);
    }
    for r in &content.research {
        sim.complete_research(&r.id);
    }
    sim.set_world(42);
    let gen = mofact_world::WorldGen::new(42, sim.tables.ores.len() as u8);
    let mut x = 0;
    for b in &content.buildings {
        // Open ground near the start, big enough for it, with no ore on it.
        let (w, h) = (i32::from(b.size.0), i32::from(b.size.1));
        let pos = loop {
            x += 1;
            let at = TilePos::new(mofact_world::START.x - 12, mofact_world::START.y + 6 + x);
            let tiles: Vec<TilePos> = (0..h)
                .flat_map(|dy| (0..w).map(move |dx| TilePos::new(at.x + dx, at.y + dy)))
                .collect();
            if tiles.iter().all(|t| {
                mofact_world::ore_at(&gen, *t).is_none()
                    && mofact_world::obstacle(&gen, *t).is_none()
                    && sim.entity_at(*t).is_none()
            }) {
                break at;
            }
        };
        x += h;
        let got = sim.apply(&Command::Place {
            building: b.id.clone(),
            pos,
            dir: Dir::East,
            recipe: None,
        });
        if matches!(b.role, mofact_core::content::BuildingRole::Miner { .. }) {
            assert_eq!(got, CommandResult::Refused(CommandError::NoOre), "{}", b.id);
        } else {
            assert_ne!(
                got,
                CommandResult::Refused(CommandError::NoOre),
                "{} needs no ore",
                b.id
            );
        }
    }
}

/// A titanium furnace facing east: titanium ore from the west, coal from
/// the north, titanium out of the front to the hub. Its iron ore by-product
/// leaves by its right side, where nothing is yet.
fn titanium_line() -> (Sim, EntityId) {
    let mut sim = Sim::with_content(&content());
    sim.add_stock(&item("iron-plate"), 200);
    sim.complete_research(&"titanium-smelting".into());
    place(&mut sim, "hub", 9, -1, Dir::East, None);
    sim.set_ore(TilePos::new(0, 0), &item("titanium-ore"));
    place(&mut sim, "drill", 0, 0, Dir::East, None);
    belts(&mut sim, 1..=4, 0, Dir::East);
    sim.set_ore(TilePos::new(5, -2), &item("coal"));
    place(&mut sim, "drill", 5, -2, Dir::South, None);
    place(&mut sim, "belt", 5, -1, Dir::South, None);
    let furnace = place(&mut sim, "furnace", 5, 0, Dir::East, Some("titanium-plate"));
    belts(&mut sim, 6..=8, 0, Dir::East);
    (sim, furnace)
}

#[test]
fn a_blocked_by_product_stops_the_machine() {
    let (mut sim, furnace) = titanium_line();
    // With nowhere for the iron ore to go, a few crafts fill the furnace's
    // by-product slot and it stops making titanium too.
    run(&mut sim, 30);
    assert!(sim.delivered(&item("titanium-plate")) > 0, "no titanium");
    run(&mut sim, 120);
    assert_eq!(state_of(&sim, furnace), MachineState::Blocked);
    let before = sim.delivered(&item("titanium-plate"));
    run(&mut sim, 30);
    assert_eq!(sim.delivered(&item("titanium-plate")), before, "kept going");
    assert_eq!(sim.delivered(&item("iron-ore")), 0, "ore left by the front");

    // A belt from its right side to the hub takes the iron ore away, and it
    // runs again: about 12 titanium a minute with the drill's 50 ore.
    place(&mut sim, "belt", 5, 1, Dir::East, None);
    belts(&mut sim, 6..=8, 1, Dir::East);
    run(&mut sim, 20);
    assert_ne!(state_of(&sim, furnace), MachineState::Blocked);
    let titanium = rate(&mut sim, "titanium-plate", 2);
    assert!(
        (titanium - 12.0).abs() <= 1.0,
        "{titanium} titanium a minute"
    );
    let ore = rate(&mut sim, "iron-ore", 2);
    assert!((ore - 12.0).abs() <= 1.0, "{ore} iron ore a minute");
}

#[test]
fn a_two_tile_machine_puts_out_along_its_whole_front() {
    let mut sim = Sim::with_content(&content());
    sim.add_stock(&item("steel"), 100);
    sim.add_stock(&item("processor"), 10);
    sim.add_stock(&item("robot-frame"), 10);
    sim.add_stock(&item("iron-plate"), 200);
    sim.add_stock(&item("silicon"), 100);
    sim.add_stock(&item("advanced-circuit"), 100);
    sim.add_stock(&item("wire"), 10);
    for r in [
        "precision-engineering",
        "precision-assembly",
        "solar-energy",
    ] {
        sim.complete_research(&r.into());
    }
    place(&mut sim, "hub", 10, -1, Dir::East, None);
    sim.set_ore(TilePos::new(-3, 0), &item("iron-ore"));
    place(&mut sim, "drill", -3, 0, Dir::East, None);
    place(&mut sim, "furnace", -2, 0, Dir::East, None);
    place(&mut sim, "belt", -1, 0, Dir::East, None);
    let asm = place(
        &mut sim,
        "precision-assembler",
        0,
        0,
        Dir::East,
        Some("gear"),
    );
    // Only the lower of its two front tiles has a belt.
    belts(&mut sim, 2..=9, 1, Dir::East);
    place(&mut sim, "solar-panel", 0, 4, Dir::East, None);
    place(&mut sim, "pole", 2, 2, Dir::East, None);
    let gears = rate(&mut sim, "gear", 2);
    assert!((gears - 25.0).abs() <= 1.0, "{gears} gears a minute");
    assert_ne!(state_of(&sim, asm), MachineState::Blocked);
}

#[test]
fn the_map_seed_and_preset_save_and_older_saves_keep_seed_42() {
    let content = content();
    let mut sim = Sim::with_content(&content);
    sim.set_world_gen(7_777, mofact_world::Preset::Lakes);
    let (loaded, _) = Sim::load(&content, &sim.save()).unwrap();
    assert_eq!(
        loaded.world_seed(),
        Some((7_777, mofact_world::Preset::Lakes))
    );
    // Version 12 had no seed: the game goes on using the default map.
    let (old, warnings) = Sim::load(&content, include_bytes!("../tests/v12-pins.mofact")).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(old.world_seed(), None);
    assert_eq!(*old.snapshot().pins, vec![TilePos::new(5, 6)]);
}

/// The save compatibility sweep (T67, R54): a save from every version the
/// game has shipped is kept in `tests/`, and each must load without a
/// warning, keep its factory, play on, and save again as the current
/// version. A version bump must add a save from the version before it.
#[test]
fn a_save_from_every_earlier_version_loads_and_plays_on() {
    let content = content();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    // Version 2 never reached the main branch, so no player has one.
    let unshipped = [2u16];
    let mut missing = Vec::new();
    for v in 1..crate::save::VERSION {
        if unshipped.contains(&v) {
            continue;
        }
        let path = dir.join(format!("v{v}.mofact"));
        let Ok(bytes) = std::fs::read(&path) else {
            missing.push(v);
            continue;
        };
        let (mut sim, warnings) =
            Sim::load(&content, &bytes).unwrap_or_else(|e| panic!("v{v} does not load: {e}"));
        assert!(warnings.is_empty(), "v{v}: {warnings:?}");
        let before = sim.snapshot();
        assert!(!before.entities.is_empty(), "v{v} lost its factory");
        assert!(before.tick > 0, "v{v} lost its clock");
        for _ in 0..120 {
            sim.step();
        }
        assert_eq!(sim.tick(), before.tick + 120, "v{v} does not play on");
        // Saved again it is the current version, and loads back the same.
        let again = sim.save();
        let (reloaded, warnings) = Sim::load(&content, &again).unwrap();
        assert!(warnings.is_empty(), "v{v} saved again: {warnings:?}");
        assert_eq!(reloaded.save(), again, "v{v} does not round-trip");
        assert_eq!(reloaded.snapshot().entities, sim.snapshot().entities);
    }
    assert!(
        missing.is_empty(),
        "no save kept for versions {missing:?}: write one with the game at that version into crates/sim/tests/"
    );
}

/// A damaged save (cut short by a crash, or with bytes flipped on a bad
/// card) must be refused or loaded, never crash the game (T67).
#[test]
fn a_damaged_save_is_refused_never_a_crash() {
    let content = content();
    let good = include_bytes!("../tests/v12.mofact").to_vec();
    let current = {
        let (sim, _) = Sim::load(&content, &good).unwrap();
        sim.save()
    };
    for bytes in [good, current] {
        let mut damaged: Vec<Vec<u8>> = (0..bytes.len())
            .step_by(3)
            .map(|n| bytes[..n].to_vec())
            .collect();
        let mut x = 0x2545_f491_u32;
        for _ in 0..400 {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            let mut b = bytes.clone();
            let at = x as usize % b.len();
            b[at] ^= 1 << (x >> 24 & 7);
            damaged.push(b);
        }
        for (n, b) in damaged.iter().enumerate() {
            let outcome = std::panic::catch_unwind(|| {
                Sim::load(&content, b).map(|(mut s, _)| {
                    // Whatever loads must also play and save.
                    s.step();
                    s.save().len()
                })
            });
            assert!(outcome.is_ok(), "damaged save {n} crashed the game");
        }
    }
}

#[test]
fn the_skyhook_takes_its_stages_in_order_and_keeps_them_through_a_save() {
    let mut sim = Sim::with_content(&content());
    sim.complete_research(&"skyhook".into());
    for (i, n) in [("steel", 200), ("titanium-plate", 100), ("ai-core", 10)] {
        sim.add_stock(&item(i), n);
    }
    let sky = place(&mut sim, "skyhook", 0, 0, Dir::East, None);
    fn sim_mega(sim: &Sim) -> mofact_core::snapshot::MegaView {
        sim.snapshot().mega.expect("the skyhook shows its stage")
    }
    let view = sim_mega(&sim);
    assert_eq!(
        (view.stage, view.stages, view.name.as_str()),
        (0, 5, "Anchor")
    );
    // A later stage's item is refused while the anchor is going in.
    assert!(!sim.feed(sky, &item("climber")));
    for _ in 0..2000 {
        assert!(sim.feed(sky, &item("steel")));
    }
    assert!(!sim.feed(sky, &item("steel")), "the anchor has its steel");
    for _ in 0..299 {
        assert!(sim.feed(sky, &item("tether-segment")));
    }
    assert_eq!(sim_mega(&sim).stage, 0);
    assert!(sim.feed(sky, &item("tether-segment")));
    let view = sim_mega(&sim);
    assert_eq!((view.stage, view.name.as_str()), (1, "Tether"));
    for _ in 0..7 {
        assert!(sim.feed(sky, &item("superconductor")));
    }

    let (loaded, warnings) = Sim::load(&content(), &sim.save()).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let view = sim_mega(&loaded);
    assert_eq!(view.stage, 1);
    assert!(view
        .needs
        .iter()
        .any(|(i, have, need)| i.as_str() == "superconductor" && *have == 7 && *need == 1000));

    let mut sim = loaded;
    for _ in 0..1000 {
        sim.feed(sky, &item("tether-segment"));
    }
    for _ in 0..993 {
        sim.feed(sky, &item("superconductor"));
    }
    for _ in 0..60 {
        sim.feed(sky, &item("climber"));
    }
    for _ in 0..15 {
        assert!(sim.feed(sky, &item("orbital-beacon")));
    }
    assert!(!sim_mega(&sim).done());
    for _ in 0..30 {
        assert!(sim.feed(sky, &item("counterweight")));
    }
    let view = sim_mega(&sim);
    assert!(view.done(), "{view:?}");
    assert!(
        !sim.feed(sky, &item("counterweight")),
        "a built skyhook takes nothing"
    );
}

#[test]
fn a_row_of_labs_pools_more_pack_types_than_a_lab_has_sides() {
    let mut sim = Sim::with_content(&content());
    sim.complete_research(&"satellites".into());
    for r in [
        "superconductors",
        "drones",
        "fusion",
        "electric-engines",
        "titanium-smelting",
        "aluminium-processing",
        "rare-earth-processing",
        "precision-engineering",
        "silicon-processing",
        "chemistry",
        "sulfur-processing",
        "advanced-electronics",
        "oil-processing",
        "steam-power",
        "fluid-handling",
        "motors",
        "steel-working",
    ] {
        sim.complete_research(&r.into());
    }
    for (i, n) in [
        ("brick", 30),
        ("gear", 50),
        ("circuit", 15),
        ("iron-plate", 40),
        ("wire", 20),
    ] {
        sim.add_stock(&item(i), n);
    }
    let labs: Vec<_> = (0..3)
        .map(|x| place(&mut sim, "lab", x, 0, Dir::East, None))
        .collect();
    place(&mut sim, "pole", 1, 1, Dir::East, None);
    for x in 0..3 {
        place(&mut sim, "wind-turbine", x, 2, Dir::East, None);
    }
    let pick = Command::SetResearch {
        research: Some("nanotech".into()),
    };
    assert!(matches!(sim.apply(&pick), CommandResult::Applied { .. }));
    // Five pack types reach the first lab and two the last: no lab is
    // brought all seven.
    let packs = [
        "survey-pack",
        "engineering-pack",
        "chemistry-pack",
        "precision-pack",
        "aerospace-pack",
        "orbital-pack",
        "stellar-pack",
    ];
    for _ in 0..120 {
        for (k, p) in packs.iter().enumerate() {
            sim.feed(labs[if k < 5 { 0 } else { 2 }], &item(p));
        }
        run(&mut sim, 20);
    }
    let r = sim.snapshot().research;
    assert!(r.progress.0 >= 60, "{r:?}");
}

#[test]
fn the_demo_stops_research_at_its_tier_until_the_full_game_is_bought() {
    let mut sim = Sim::with_content(&content());
    for r in [
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
    ] {
        sim.complete_research(&r.into());
    }
    let pick = |r: &str| Command::SetResearch {
        research: Some(r.into()),
    };
    sim.apply(&pick("titanium-smelting"));
    assert!(sim.snapshot().research.current.is_some());
    sim.set_demo(Some(4));
    assert_eq!(
        sim.snapshot().research.current,
        None,
        "research past the gate stops"
    );
    assert_eq!(
        sim.apply(&pick("titanium-smelting")),
        CommandResult::Refused(CommandError::FullGame)
    );
    assert!(
        matches!(
            sim.apply(&pick("solar-energy")),
            CommandResult::Applied { .. }
        ),
        "the demo's own tiers stay open"
    );
    sim.set_demo(None);
    assert!(matches!(
        sim.apply(&pick("titanium-smelting")),
        CommandResult::Applied { .. }
    ));
}

#[test]
fn repeatable_research_never_runs_out_and_each_level_speeds_drills() {
    let content = content();
    let mut sim = Sim::with_content(&content);
    // Everything up to it.
    for r in &content.research {
        if r.repeat.is_none() {
            sim.complete_research(&r.id);
        }
    }
    let deep: mofact_core::ids::ResearchId = "deep-survey".into();
    let pick = Command::SetResearch {
        research: Some(deep.clone()),
    };
    assert!(matches!(sim.apply(&pick), CommandResult::Applied { .. }));
    let r = sim.tables.research(&deep).unwrap() as usize;
    // One unit short of the first level, then labs finish it.
    sim.research.units[r] = 49;
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
    let lab = place(&mut sim, "lab", 0, 1, Dir::East, None);
    for st in &content.research[r].cost {
        assert!(sim.feed(lab, &st.item), "{}", st.item);
    }
    let before = sim.snapshot().research.progress;
    assert_eq!(before, (49, 50));
    run(&mut sim, 120);
    let snap = sim.snapshot().research;
    assert!(snap.done.contains(&deep));
    assert_eq!(snap.levels, vec![(deep.clone(), 1)]);
    // Labs carry on with the next level, which takes twice the units.
    assert_eq!(snap.current, Some(deep.clone()), "it never runs out");
    assert_eq!(snap.progress, (0, 100));
    let again = Command::SetResearch {
        research: Some(deep.clone()),
    };
    assert!(matches!(sim.apply(&again), CommandResult::Applied { .. }));

    // Each level digs ten percent faster.
    let ticks = |sim: &Sim| {
        let Role::Miner { ticks } =
            sim.tables.buildings[sim.tables.building(&"drill".into()).unwrap() as usize].role
        else {
            unreachable!()
        };
        sim.mine_ticks(ticks)
    };
    let one = ticks(&sim);
    sim.complete_research(&deep);
    sim.complete_research(&deep);
    let three = ticks(&sim);
    assert_eq!(sim.snapshot().research.levels, vec![(deep.clone(), 3)]);
    assert!(three < one, "{three} < {one}");

    // A save keeps the level, the boost and the labs on it.
    let (loaded, warnings) = Sim::load(&content, &sim.save()).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(loaded.snapshot().research, sim.snapshot().research);
    assert_eq!(ticks(&loaded), three);
}

/// Content whose radar reaches four cells, so a test can watch it finish.
fn short_radar() -> Content {
    let mut c = content();
    let radar = c
        .buildings
        .iter_mut()
        .find(|b| b.id == BuildingId::from("radar"))
        .unwrap();
    radar.role = mofact_core::content::BuildingRole::Radar {
        reach: 4 * mofact_core::FOG_CELL as u32,
        seconds_per_patch: 1.0,
    };
    c
}

fn cell(x: i32, y: i32) -> mofact_core::geom::FogCell {
    mofact_core::geom::FogCell::new(x, y)
}

/// Cells a disc of `r` cells round (0, 0) holds.
fn disc(r: i32) -> usize {
    (-r..=r)
        .flat_map(|y| (-r..=r).map(move |x| x * x + y * y))
        .filter(|d| *d <= r * r + r)
        .count()
}

#[test]
fn a_new_game_sees_the_land_round_its_hub() {
    let mut sim = Sim::with_content(&content());
    assert!(sim.snapshot().explored.is_empty());
    place(&mut sim, "hub", 0, 0, Dir::East, None);
    let snap = sim.snapshot();
    assert_eq!(snap.explored.len(), disc(REVEAL_HUB));
    assert!(sim.explored(cell(5, 0)) && sim.explored(cell(-4, 3)));
    assert!(!sim.explored(cell(6, 0)) && !sim.explored(cell(4, 4)));
    assert!(snap.explored.windows(2).all(|w| w[0] < w[1]), "sorted");
}

#[test]
fn a_powered_radar_charts_the_nearest_land_first_and_stops_at_its_reach() {
    let c = short_radar();
    let mut sim = Sim::with_content(&c);
    for (i, n) in [
        ("iron-plate", 100),
        ("gear", 50),
        ("wire", 50),
        ("circuit", 20),
    ] {
        sim.add_stock(&item(i), n);
    }
    place(&mut sim, "pole", 0, 0, Dir::East, None);
    place(&mut sim, "wind-turbine", 1, 0, Dir::East, None);
    let radar = place(&mut sim, "radar", 0, 1, Dir::East, None);
    // It sees round itself as soon as it stands.
    assert_eq!(sim.snapshot().explored.len(), disc(REVEAL_RADAR));
    run(&mut sim, 2);
    // The nearest hidden cells are 2 and 3 cells off on the two axes.
    let found: Vec<_> = sim
        .snapshot()
        .explored
        .iter()
        .copied()
        .filter(|c| c.x * c.x + c.y * c.y > REVEAL_RADAR * REVEAL_RADAR + REVEAL_RADAR)
        .collect();
    assert!(!found.is_empty(), "nothing charted in two seconds");
    assert!(
        found.iter().all(|c| c.x * c.x + c.y * c.y == 13),
        "{found:?}"
    );
    run(&mut sim, 60);
    let snap = sim.snapshot();
    assert_eq!(snap.explored.len(), disc(4));
    assert!(!sim.explored(cell(5, 0)));
    let view = snap.entities.iter().find(|e| e.id == radar).unwrap();
    assert_eq!(view.state, MachineState::Idle, "done, so it rests");
    // The fog it lifted stays lifted over a save, and it doesn't start again.
    let (loaded, _) = Sim::load(&c, &sim.save()).unwrap();
    assert_eq!(loaded.snapshot().explored, snap.explored);
    let view = loaded.snapshot();
    let view = view.entities.iter().find(|e| e.id == radar).unwrap();
    assert_eq!(view.state, MachineState::Idle);
}

#[test]
fn an_unpowered_radar_charts_nothing() {
    let mut sim = Sim::with_content(&short_radar());
    for (i, n) in [("iron-plate", 100), ("gear", 50), ("circuit", 20)] {
        sim.add_stock(&item(i), n);
    }
    place(&mut sim, "radar", 0, 1, Dir::East, None);
    run(&mut sim, 10);
    assert_eq!(sim.snapshot().explored.len(), disc(REVEAL_RADAR));
}

/// A sim with drones researched and plenty to build ports with.
fn drone_sim() -> Sim {
    let mut sim = Sim::with_content(&content());
    sim.complete_research(&"drones".into());
    for (i, n) in [
        ("iron-plate", 5000),
        ("copper-plate", 5000),
        ("gear", 500),
        ("wire", 500),
        ("circuit", 500),
        ("steel", 500),
        ("advanced-circuit", 200),
        ("drone", 40),
    ] {
        sim.add_stock(&item(i), n);
    }
    sim
}

/// A powered drone port with its corner at `x`, 0.
fn port(sim: &mut Sim, x: i32) -> EntityId {
    let p = place(sim, "drone-port", x, 0, Dir::East, None);
    place(sim, "pole", x, 2, Dir::East, None);
    place(sim, "wind-turbine", x + 1, 2, Dir::East, None);
    p
}

#[test]
fn a_port_brings_its_drones_and_they_go_with_it() {
    let mut sim = drone_sim();
    let p = port(&mut sim, 0);
    assert_eq!(sim.port_drones(p), 4);
    assert!(
        sim.snapshot().drones.is_empty(),
        "resting drones aren't drawn"
    );
    assert_eq!(sim.stock(&item("drone")), 36);
    sim.apply(&Command::Remove { entity: p });
    assert_eq!(sim.port_drones(p), 0);
    assert_eq!(
        sim.stock(&item("drone")),
        40,
        "removing gives the drones back"
    );
}

/// Logistic drones (T49): five ports in a row make one network 200 tiles
/// long, and its drones fill a requester chest at the far end from the hub.
#[test]
fn a_requester_two_hundred_tiles_out_is_kept_stocked_by_a_chain_of_ports() {
    let mut sim = drone_sim();
    place(&mut sim, "hub", -5, 0, Dir::East, None);
    for x in [0, 50, 100, 150, 200] {
        port(&mut sim, x);
    }
    let chest = place(&mut sim, "requester-chest", 205, 0, Dir::East, None);
    let ask = Command::SetRequest {
        entity: chest,
        request: Some((item("gear"), 40)),
    };
    assert!(matches!(sim.apply(&ask), CommandResult::Applied { .. }));
    let gears = sim.stock(&item("gear"));
    run(&mut sim, 10);
    let flying = sim.snapshot().drones;
    assert!(!flying.is_empty() && flying.iter().all(|d| d.cargo == Some(item("gear"))));
    // Their batteries fly 120 tiles, so they charge at ports on the way.
    run(&mut sim, 110);
    assert_eq!(sim.inventory(chest), vec![(item("gear"), 40)]);
    assert_eq!(sim.stock(&item("gear")), gears - 40);
    // It asks for no more than it wants.
    run(&mut sim, 30);
    assert_eq!(sim.inventory(chest), vec![(item("gear"), 40)]);
    // Out of reach of every port, a chest gets nothing.
    let far = place(&mut sim, "requester-chest", 400, 0, Dir::East, None);
    sim.apply(&Command::SetRequest {
        entity: far,
        request: Some((item("gear"), 10)),
    });
    run(&mut sim, 60);
    assert!(sim.inventory(far).is_empty());
}

/// Construction drones (T50): a planned block of 50 assemblers is built
/// with no more taps, from the hub's stock, and pays the same as by hand.
#[test]
fn construction_drones_build_a_fifty_machine_plan_with_no_taps() {
    let mut sim = drone_sim();
    place(&mut sim, "hub", -5, 0, Dir::East, None);
    port(&mut sim, 0);
    let plates = sim.stock(&item("iron-plate"));
    let mut planned = 0;
    for k in 0..50 {
        let (x, y) = (-20 + (k % 10) * 4, 6 + (k / 10) * 4);
        let plan = Command::Plan {
            building: "assembler".into(),
            pos: TilePos::new(x, y),
            dir: Dir::East,
            recipe: Some("gear".into()),
        };
        assert!(
            matches!(sim.apply(&plan), CommandResult::Applied { .. }),
            "{x},{y}"
        );
        planned += 1;
    }
    assert_eq!(sim.snapshot().ghosts.len(), planned);
    // Nothing is spent until a drone sets off with it.
    assert_eq!(sim.stock(&item("iron-plate")), plates);
    run(&mut sim, 240);
    let snap = sim.snapshot();
    assert!(snap.ghosts.is_empty(), "{} ghosts left", snap.ghosts.len());
    let built: Vec<_> = snap
        .entities
        .iter()
        .filter(|e| e.building == BuildingId::from("assembler"))
        .collect();
    assert_eq!(built.len(), 50);
    assert!(built
        .iter()
        .all(|e| e.recipe == Some(RecipeId::from("gear"))));
    let cost = content()
        .buildings
        .iter()
        .find(|b| b.id == BuildingId::from("assembler"))
        .unwrap()
        .cost
        .iter()
        .find(|c| c.item == item("iron-plate"))
        .unwrap()
        .count;
    assert_eq!(sim.stock(&item("iron-plate")), plates - 50 * cost);
}

#[test]
fn marked_buildings_are_taken_down_and_a_ghost_can_be_called_off() {
    let mut sim = drone_sim();
    place(&mut sim, "hub", -5, 0, Dir::East, None);
    port(&mut sim, 0);
    let belt = place(&mut sim, "belt", 10, 10, Dir::East, None);
    let plates = sim.stock(&item("iron-plate"));
    assert!(matches!(
        sim.apply(&Command::Mark {
            entity: belt,
            on: true
        }),
        CommandResult::Applied { .. }
    ));
    assert_eq!(sim.snapshot().marked, vec![belt]);
    let plan = Command::Plan {
        building: "belt".into(),
        pos: TilePos::new(12, 12),
        dir: Dir::East,
        recipe: None,
    };
    sim.apply(&plan);
    sim.apply(&Command::Unplan {
        pos: TilePos::new(12, 12),
    });
    run(&mut sim, 20);
    let snap = sim.snapshot();
    assert!(snap.entities.iter().all(|e| e.id != belt), "taken down");
    assert!(snap.marked.is_empty() && snap.ghosts.is_empty());
    assert!(snap.entities.iter().all(|e| e.pos != TilePos::new(12, 12)));
    assert_eq!(
        sim.stock(&item("iron-plate")),
        plates + 1,
        "the belt comes back"
    );
}

/// Drones in flight, their loads, ghosts and requests all survive a save,
/// and the loaded game plays on exactly like the original.
#[test]
fn drones_mid_flight_save_and_play_on_the_same() {
    let mut sim = drone_sim();
    place(&mut sim, "hub", -5, 0, Dir::East, None);
    port(&mut sim, 0);
    port(&mut sim, 50);
    let chest = place(&mut sim, "requester-chest", 60, 0, Dir::East, None);
    sim.apply(&Command::SetRequest {
        entity: chest,
        request: Some((item("circuit"), 30)),
    });
    for k in 0..8 {
        sim.apply(&Command::Plan {
            building: "assembler".into(),
            pos: TilePos::new(10 + k * 3, 10),
            dir: Dir::South,
            recipe: None,
        });
    }
    run(&mut sim, 3);
    assert!(
        sim.snapshot().drones.iter().any(|d| d.at != d.to),
        "some in flight"
    );
    let bytes = sim.save();
    let (mut back, warnings) = Sim::load(&content(), &bytes).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(back.save(), bytes);
    run(&mut sim, 60);
    run(&mut back, 60);
    assert_eq!(back.save(), sim.save());
    assert_eq!(sim.inventory(chest), vec![(item("circuit"), 30)]);
}

#[test]
fn drills_dig_round_themselves_once_their_tile_runs_dry() {
    let content = content();
    let mut sim = Sim::with_content(&content);
    sim.add_stock(&item("iron-plate"), 10_000);
    let t = TilePos::new(0, 0);
    let north = TilePos::new(0, -1);
    sim.set_ore_amount(t, &item("iron-ore"), 3);
    sim.set_ore_amount(north, &item("iron-ore"), 2);
    let drill = place(&mut sim, "drill", t.x, t.y, Dir::East, None);
    for x in 1..=4 {
        place(&mut sim, "belt", x, 0, Dir::East, None);
    }
    let view = |sim: &Sim| {
        let snap = sim.snapshot();
        let e = snap
            .entities
            .iter()
            .find(|e| e.id == drill)
            .unwrap()
            .clone();
        (e.ore_left, e.ore, e.state)
    };
    assert_eq!(view(&sim).0, Some(5), "its own tile and the one beside it");
    run(&mut sim, 5);
    assert_eq!(sim.ore_left(t), None, "its own tile went first");
    assert!(sim.ore_left(north).is_some(), "then the tile beside it");
    assert_eq!(view(&sim).1, Some(item("iron-ore")));
    assert_eq!(view(&sim).2, MachineState::Working);
    run(&mut sim, 40);
    assert_eq!(view(&sim), (Some(0), None, MachineState::Starved));
    assert_eq!(*sim.snapshot().depleted, vec![north, t]);
}

#[test]
fn a_drill_claims_the_same_ore_round_it_on_the_map() {
    let content = content();
    let mut sim = Sim::with_content(&content);
    sim.add_stock(&item("iron-plate"), 10_000);
    sim.set_world(42);
    // The middle of the starter iron patch: ore on all nine tiles.
    let t = TilePos::new(-10, 1);
    let iron = sim.ore_at(t).expect("starter iron");
    let drill = place(&mut sim, "drill", t.x, t.y, Dir::East, None);
    let left = |sim: &Sim| {
        sim.snapshot()
            .entities
            .iter()
            .find(|e| e.id == drill)
            .and_then(|e| e.ore_left)
    };
    assert_eq!(left(&sim), Some(9 * mofact_world::SPAWN_ORE));
    assert_eq!(sim.ore_at(TilePos::new(-11, 0)), Some(iron));
    // A save keeps the claim, and loading claims no more.
    let (loaded, _) = Sim::load(&content, &sim.save()).unwrap();
    assert_eq!(left(&loaded), Some(9 * mofact_world::SPAWN_ORE));
}
