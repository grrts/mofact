//! Blueprints (T51, R37): a group of buildings copied off the map, to
//! paste again elsewhere or keep in the library for later.
//!
//! A blueprint keeps each building's place relative to the group's
//! top-left corner, the way it faces and its recipe. It turns a quarter at
//! a time, and turns into the [`Command`]s that build it at a spot.

use mofact_core::command::Command;
use mofact_core::geom::{Dir, TilePos};
use mofact_core::ids::{BuildingId, RecipeId};
use serde::{Deserialize, Serialize};

/// How many blueprints the library keeps.
pub const LIBRARY_SIZE: usize = 24;

/// One building in a blueprint.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Piece {
    pub building: BuildingId,
    /// Its top-left tile, from the blueprint's top-left corner.
    pub at: (i32, i32),
    /// Its footprint, tiles a side: every building is square.
    pub size: u8,
    pub dir: Dir,
    #[serde(default)]
    pub recipe: Option<RecipeId>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Blueprint {
    pub name: String,
    /// In the order they were first built, so pairs that join up by
    /// order (an underground belt's entrance, then its exit) still do.
    pub pieces: Vec<Piece>,
    /// Width and height in tiles.
    pub size: (i32, i32),
}

impl Blueprint {
    /// A blueprint of `pieces`: building, top-left tile, size, facing
    /// and recipe, in the order they were built.
    pub fn capture(
        name: impl Into<String>,
        pieces: impl IntoIterator<Item = (BuildingId, TilePos, u8, Dir, Option<RecipeId>)>,
    ) -> Self {
        let list: Vec<_> = pieces.into_iter().collect();
        let (mut lo, mut hi) = ((i32::MAX, i32::MAX), (i32::MIN, i32::MIN));
        for (_, p, s, ..) in &list {
            let s = i32::from((*s).max(1));
            lo = (lo.0.min(p.x), lo.1.min(p.y));
            hi = (hi.0.max(p.x + s), hi.1.max(p.y + s));
        }
        let size = if list.is_empty() {
            (0, 0)
        } else {
            (hi.0 - lo.0, hi.1 - lo.1)
        };
        Self {
            name: name.into(),
            pieces: list
                .into_iter()
                .map(|(building, p, size, dir, recipe)| Piece {
                    building,
                    at: (p.x - lo.0, p.y - lo.1),
                    size: size.max(1),
                    dir,
                    recipe,
                })
                .collect(),
            size,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.pieces.is_empty()
    }

    /// Turn it a quarter clockwise, about its own corner.
    pub fn rotate(&mut self) {
        let h = self.size.1;
        for p in &mut self.pieces {
            let s = i32::from(p.size);
            p.at = (h - p.at.1 - s, p.at.0);
            p.dir = p.dir.rotate_cw();
        }
        self.size = (self.size.1, self.size.0);
    }

    /// Each piece's building, top-left tile, facing and recipe, with the
    /// blueprint's corner at `at`.
    pub fn placed(
        &self,
        at: TilePos,
    ) -> impl Iterator<Item = (&BuildingId, TilePos, u8, Dir, Option<&RecipeId>)> {
        self.pieces.iter().map(move |p| {
            (
                &p.building,
                TilePos::new(at.x + p.at.0, at.y + p.at.1),
                p.size,
                p.dir,
                p.recipe.as_ref(),
            )
        })
    }

    /// Every tile it covers with its corner at `at`.
    pub fn tiles(&self, at: TilePos) -> impl Iterator<Item = TilePos> + '_ {
        self.placed(at).flat_map(|(_, p, s, ..)| {
            let s = i32::from(s);
            (0..s).flat_map(move |dy| (0..s).map(move |dx| TilePos::new(p.x + dx, p.y + dy)))
        })
    }

    /// The commands that build it with its corner at `at`.
    pub fn commands(&self, at: TilePos) -> Vec<Command> {
        self.placed(at)
            .map(|(building, pos, _, dir, recipe)| Command::Place {
                building: building.clone(),
                pos,
                dir,
                recipe: recipe.cloned(),
            })
            .collect()
    }

    /// How many of each building it takes, most first.
    pub fn counts(&self) -> Vec<(&BuildingId, u32)> {
        let mut out: Vec<(&BuildingId, u32)> = Vec::new();
        for p in &self.pieces {
            match out.iter_mut().find(|(b, _)| **b == p.building) {
                Some((_, n)) => *n += 1,
                None => out.push((&p.building, 1)),
            }
        }
        out.sort_by_key(|p| std::cmp::Reverse(p.1));
        out
    }
}

/// The blueprint library, as text to keep on the device. Unreadable text
/// is an empty library, never a failure.
pub fn library_from_text(text: &str) -> Vec<Blueprint> {
    let mut list: Vec<Blueprint> = ron::from_str(text).unwrap_or_default();
    list.truncate(LIBRARY_SIZE);
    list
}

pub fn library_to_text(library: &[Blueprint]) -> String {
    ron::to_string(library).unwrap_or_default()
}

/// The name for the next blueprint saved: "Blueprint N", N one more
/// than any already in the library.
pub fn next_name(library: &[Blueprint], word: &str) -> String {
    let n = library
        .iter()
        .filter_map(|b| b.name.rsplit(' ').next()?.parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    format!("{word} {}", n + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(x: i32, y: i32) -> TilePos {
        TilePos::new(x, y)
    }

    fn sample() -> Blueprint {
        Blueprint::capture(
            "Blueprint 1",
            [
                (
                    "assembler".into(),
                    t(10, 5),
                    2,
                    Dir::East,
                    Some("gear".into()),
                ),
                ("belt".into(), t(12, 5), 1, Dir::South, None),
                ("belt".into(), t(12, 6), 1, Dir::South, None),
            ],
        )
    }

    #[test]
    fn capture_keeps_places_from_the_corner() {
        let b = sample();
        assert_eq!(b.size, (3, 2));
        assert_eq!(b.pieces[0].at, (0, 0));
        assert_eq!(b.pieces[2].at, (2, 1));
        let cmds = b.commands(t(-4, 0));
        assert_eq!(
            cmds[0],
            Command::Place {
                building: "assembler".into(),
                pos: t(-4, 0),
                dir: Dir::East,
                recipe: Some("gear".into())
            }
        );
        assert_eq!(b.tiles(t(0, 0)).count(), 6);
        assert_eq!(b.counts()[0], (&"belt".into(), 2));
    }

    #[test]
    fn four_turns_come_back_and_one_turns_the_footprint() {
        let mut b = sample();
        b.rotate();
        assert_eq!(b.size, (2, 3));
        // The 2x2 assembler at the top-left ends at the top-right.
        assert_eq!(b.pieces[0].at, (0, 0));
        assert_eq!(b.pieces[0].dir, Dir::South);
        // The belt at (2, 1) goes to (0, 2), facing west.
        assert_eq!(b.pieces[2].at, (0, 2));
        assert_eq!(b.pieces[2].dir, Dir::West);
        let mut tiles: Vec<_> = b.tiles(t(0, 0)).collect();
        tiles.sort();
        tiles.dedup();
        assert_eq!(tiles.len(), 6, "nothing overlaps after a turn");
        assert!(tiles.iter().all(|p| p.x < 2 && p.y < 3));
        for _ in 0..3 {
            b.rotate();
        }
        assert_eq!(b, sample());
    }

    #[test]
    fn the_library_round_trips_and_names_count_on() {
        let lib = vec![sample()];
        let back = library_from_text(&library_to_text(&lib));
        assert_eq!(back, lib);
        assert_eq!(next_name(&back, "Blueprint"), "Blueprint 2");
        assert!(library_from_text("not a library").is_empty());
    }
}
