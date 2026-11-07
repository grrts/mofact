//! Rails and trains (T46, T47): trains share signalled track without
//! meeting, and an ore train delivers what its drills mine.

use super::*;
use mofact_core::command::TrainStop;
use mofact_core::ids::{BuildingId, ResearchId};
use std::path::Path;

fn content() -> Content {
    mofact_content::load_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data"))
        .expect("data loads")
}

fn item(s: &str) -> ItemId {
    ItemId::from(s)
}

/// A world with railways researched and plenty to build with.
fn railway() -> Sim {
    let mut sim = Sim::with_content(&content());
    sim.complete_research(&ResearchId::from("railways"));
    for (i, n) in [
        ("iron-plate", 5000),
        ("steel", 5000),
        ("circuit", 5000),
        ("motor", 500),
        ("brick", 500),
        ("copper-plate", 500),
    ] {
        sim.add_stock(&item(i), n);
    }
    sim
}

fn try_place(sim: &mut Sim, building: &str, x: i32, y: i32, dir: Dir) -> CommandResult {
    sim.apply(&Command::Place {
        building: BuildingId::from(building),
        pos: TilePos::new(x, y),
        dir,
        recipe: None,
    })
}

fn place(sim: &mut Sim, building: &str, x: i32, y: i32, dir: Dir) -> EntityId {
    match try_place(sim, building, x, y, dir) {
        CommandResult::Applied {
            created: Some(id), ..
        } => id,
        other => panic!("placing {building} at {x},{y}: {other:?}"),
    }
}

fn schedule(sim: &mut Sim, train: EntityId, stops: &[(i32, i32, bool)]) {
    let stops = stops
        .iter()
        .map(|&(x, y, load)| TrainStop {
            at: TilePos::new(x, y),
            load,
        })
        .collect();
    let r = sim.apply(&Command::SetSchedule {
        entity: train,
        stops,
    });
    assert!(matches!(r, CommandResult::Applied { .. }), "{r:?}");
}

fn run(sim: &mut Sim, seconds: u32) {
    for _ in 0..seconds * mofact_core::TICKS_PER_SECOND {
        sim.step();
    }
}

/// Each train's tiles: where it stands and where it is moving to.
fn train_tiles(sim: &Sim) -> Vec<Vec<TilePos>> {
    sim.rails
        .trains
        .iter()
        .filter_map(|id| match sim.states[id.0 as usize].as_ref()? {
            State::Train(t) => Some(std::iter::once(t.at).chain(t.to).collect()),
            _ => None,
        })
        .collect()
}

/// The tiles round the edge of the rectangle from (x0, y0) to (x1, y1), clockwise from the top left.
fn ring(x0: i32, y0: i32, x1: i32, y1: i32) -> Vec<TilePos> {
    let mut v = Vec::new();
    v.extend((x0..x1).map(|x| TilePos::new(x, y0)));
    v.extend((y0..y1).map(|y| TilePos::new(x1, y)));
    v.extend((x0 + 1..=x1).rev().map(|x| TilePos::new(x, y1)));
    v.extend((y0 + 1..=y1).rev().map(|y| TilePos::new(x0, y)));
    v
}

/// A loop of track 20 by 10 with a signal on each side, splitting it into
/// four blocks, and two trains running round it clockwise between two stops.
fn signalled_loop() -> (Sim, [EntityId; 2]) {
    let mut sim = railway();
    let signals = [
        TilePos::new(10, 0),
        TilePos::new(19, 5),
        TilePos::new(9, 9),
        TilePos::new(0, 4),
    ];
    let stops = [TilePos::new(4, 0), TilePos::new(15, 9)];
    for t in ring(0, 0, 19, 9) {
        let what = if signals.contains(&t) {
            "rail-signal"
        } else if stops.contains(&t) {
            "train-stop"
        } else {
            "rail"
        };
        place(&mut sim, what, t.x, t.y, Dir::North);
    }
    let a = place(&mut sim, "locomotive", 2, 0, Dir::East);
    let b = place(&mut sim, "locomotive", 17, 9, Dir::West);
    // Stopping to unload with nothing aboard moves straight on.
    for t in [a, b] {
        schedule(&mut sim, t, &[(4, 0, false), (15, 9, false)]);
    }
    (sim, [a, b])
}

#[test]
fn two_trains_share_signalled_track_safely() {
    let (mut sim, trains) = signalled_loop();
    assert_eq!(sim.rails.blocks.values().collect::<HashSet<_>>().len(), 4);
    let mut laps = [0usize; 2];
    let mut last = [0usize; 2];
    for tick in 0..120 * mofact_core::TICKS_PER_SECOND {
        sim.step();
        let tiles = train_tiles(&sim);
        assert_eq!(tiles.len(), 2);
        // Never on the same tile, and never in the same block.
        assert!(
            tiles[0].iter().all(|t| !tiles[1].contains(t)),
            "tick {tick}: trains share a tile: {tiles:?}"
        );
        let blocks = |ts: &Vec<TilePos>| -> HashSet<u32> {
            ts.iter()
                .filter_map(|t| sim.rails.blocks.get(t).copied())
                .collect()
        };
        assert!(
            blocks(&tiles[0]).is_disjoint(&blocks(&tiles[1])),
            "tick {tick}: trains share a block: {tiles:?}"
        );
        for (k, &id) in trains.iter().enumerate() {
            let Some(State::Train(t)) = &sim.states[id.0 as usize] else {
                panic!("train gone");
            };
            if t.next != last[k] {
                laps[k] += 1;
                last[k] = t.next;
            }
        }
    }
    // A lap is 56 tiles; at eight tiles a second, two minutes is about
    // seventeen laps, two stops each, less waiting at signals.
    for (k, n) in laps.iter().enumerate() {
        assert!(*n >= 20, "train {k} reached only {n} stops");
    }
}

#[test]
fn without_signals_trains_still_never_share_a_tile() {
    let mut sim = railway();
    for t in ring(0, 0, 9, 5) {
        place(&mut sim, "rail", t.x, t.y, Dir::North);
    }
    let a = place(&mut sim, "locomotive", 1, 0, Dir::East);
    let b = place(&mut sim, "locomotive", 8, 5, Dir::West);
    // Both chase the same far corner, round and round.
    for t in [a, b] {
        schedule(&mut sim, t, &[(9, 5, false), (0, 0, false)]);
    }
    for tick in 0..60 * mofact_core::TICKS_PER_SECOND {
        sim.step();
        let tiles = train_tiles(&sim);
        assert!(
            tiles[0].iter().all(|t| !tiles[1].contains(t)),
            "tick {tick}: {tiles:?}"
        );
    }
}

/// Four drills on iron feed a long belt into a stop at the west end of a line
/// fifty tiles long; a train carries the ore to a stop at the east end,
/// which unloads into the hub.
fn ore_line() -> (Sim, EntityId) {
    let mut sim = railway();
    place(&mut sim, "hub", 60, 0, Dir::East);
    for x in 10..=61 {
        let what = if x == 10 || x == 61 {
            "train-stop"
        } else {
            "rail"
        };
        // The east stop faces north, into the hub.
        place(&mut sim, what, x, 3, Dir::North);
    }
    place(&mut sim, "belt", 10, 4, Dir::North);
    // Forty tiles of belt hold the 50-odd ore mined while the train is away.
    for x in 11..=50 {
        place(&mut sim, "belt", x, 4, Dir::West);
    }
    for x in 47..=50 {
        sim.set_ore(TilePos::new(x, 5), &item("iron-ore"));
        place(&mut sim, "drill", x, 5, Dir::North);
    }
    let train = place(&mut sim, "locomotive", 12, 3, Dir::West);
    schedule(&mut sim, train, &[(10, 3, true), (61, 3, false)]);
    (sim, train)
}

#[test]
fn an_ore_train_delivers_what_its_drills_mine() {
    let (mut sim, _) = ore_line();
    // Four drills at 1.2 seconds an ore make 200 a minute. The train takes
    // 200 a trip, and the belt holds what's mined while it is away.
    run(&mut sim, 120);
    let before = sim.delivered(&item("iron-ore"));
    let minutes = 20;
    run(&mut sim, 60 * minutes);
    let rate = (sim.delivered(&item("iron-ore")) - before) as f64 / f64::from(minutes);
    assert!(
        (190.0..=201.0).contains(&rate),
        "{rate} ore a minute, not 200"
    );
}

#[test]
fn a_saved_railway_plays_on_exactly_like_the_original() {
    let (mut sim, _) = ore_line();
    run(&mut sim, 75);
    let (mut loaded, warnings) = Sim::load(&content(), &sim.save()).expect("loads");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(loaded.snapshot().trains, sim.snapshot().trains);
    for _ in 0..90 * mofact_core::TICKS_PER_SECOND {
        sim.step();
        loaded.step();
    }
    assert_eq!(loaded.snapshot().trains, sim.snapshot().trains);
    assert_eq!(
        loaded.delivered(&item("iron-ore")),
        sim.delivered(&item("iron-ore"))
    );
    assert!(sim.delivered(&item("iron-ore")) > 0);
}

#[test]
fn track_under_a_train_stays_and_a_removed_train_comes_back_with_its_schedule() {
    let (mut sim, train) = ore_line();
    run(&mut sim, 20);
    let under = sim.snapshot().trains[0].pos;
    let rail = sim.entity_at(under).expect("track");
    assert_eq!(
        sim.apply(&Command::Remove { entity: rail }),
        CommandResult::Refused(CommandError::Blocked)
    );
    // Trains go only on track no train has.
    assert!(matches!(
        try_place(&mut sim, "locomotive", 20, 0, Dir::East),
        CommandResult::Refused(CommandError::Blocked)
    ));
    assert!(matches!(
        try_place(&mut sim, "locomotive", under.x, under.y, Dir::East),
        CommandResult::Refused(CommandError::Blocked)
    ));
    let cargo: u32 = sim.snapshot().trains[0].cargo.iter().map(|c| c.1).sum();
    assert!(cargo > 0, "it has been loading for a while");
    let ore = sim.stock(&item("iron-ore"));
    let mut history = History::new();
    history.apply(&mut sim, &Command::Remove { entity: train });
    assert!(sim.snapshot().trains.is_empty());
    assert_eq!(
        sim.stock(&item("iron-ore")),
        ore + cargo,
        "cargo comes back"
    );
    assert!(history.undo(&mut sim));
    let back = &sim.snapshot().trains[0];
    assert_eq!(back.schedule.len(), 2);
    assert_eq!(back.pos, under);
    run(&mut sim, 1);
    // The belt is still in front of the stop it waits at.
    assert!(sim.rails.trains.len() == 1);
}

#[test]
fn a_train_with_no_way_to_its_stop_says_so() {
    let mut sim = railway();
    for x in 0..5 {
        place(&mut sim, "rail", x, 0, Dir::North);
    }
    place(&mut sim, "train-stop", 9, 0, Dir::North);
    let t = place(&mut sim, "locomotive", 1, 0, Dir::East);
    schedule(&mut sim, t, &[(9, 0, false)]);
    run(&mut sim, 2);
    assert_eq!(
        sim.snapshot().trains[0].state,
        mofact_core::snapshot::TrainState::NoPath
    );
    // Lay the missing track and it sets off.
    for x in 5..9 {
        place(&mut sim, "rail", x, 0, Dir::North);
    }
    run(&mut sim, 3);
    assert_eq!(sim.snapshot().trains[0].pos, TilePos::new(9, 0));
}

use std::collections::HashSet;

#[test]
fn a_signal_put_on_a_rail_takes_its_place_and_undo_puts_the_rail_back() {
    let mut sim = railway();
    for x in 0..5 {
        place(&mut sim, "rail", x, 0, Dir::North);
    }
    let steel = sim.stock(&item("steel"));
    let mut history = History::new();
    let r = history.apply(
        &mut sim,
        &Command::Place {
            building: BuildingId::from("rail-signal"),
            pos: TilePos::new(2, 0),
            dir: Dir::North,
            recipe: None,
        },
    );
    assert!(
        matches!(
            r,
            CommandResult::Applied {
                created: Some(_),
                ..
            }
        ),
        "{r:?}"
    );
    let at = |sim: &Sim| {
        let id = sim.entity_at(TilePos::new(2, 0)).unwrap();
        sim.snapshot()
            .entities
            .iter()
            .find(|e| e.id == id)
            .unwrap()
            .building
            .clone()
    };
    assert_eq!(at(&sim).as_str(), "rail-signal");
    // The rail's steel came back and the signal's went: one each.
    assert_eq!(sim.stock(&item("steel")), steel);
    assert_eq!(sim.rails.blocks.values().collect::<HashSet<_>>().len(), 2);
    assert!(history.undo(&mut sim));
    assert_eq!(at(&sim).as_str(), "rail");
    assert_eq!(sim.rails.blocks.values().collect::<HashSet<_>>().len(), 1);
    assert!(history.redo(&mut sim));
    assert_eq!(at(&sim).as_str(), "rail-signal");
}
