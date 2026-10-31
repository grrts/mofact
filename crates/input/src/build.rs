//! Build tools (T20): place, drag belts, rotate, remove, ported from the
//! prototype's pointer logic. The tools turn tile positions into simulation
//! [`Command`]s; they ask a [`Ground`] what is already built.

use mofact_core::command::Command;
use mofact_core::geom::{Dir, TilePos};
use mofact_core::ids::{BuildingId, EntityId, RecipeId};

/// What stands on a tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Occupant {
    pub id: EntityId,
    pub belt: bool,
    pub dir: Dir,
    /// The hub can't be removed.
    pub removable: bool,
}

/// The built world, as the tools see it.
pub trait Ground {
    fn at(&self, tile: TilePos) -> Option<Occupant>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tool {
    /// Pan and inspect.
    Hand,
    /// Draw belts along the drag path.
    Belt,
    /// Place one building where the finger lifts.
    Place(BuildingId),
    /// Remove whatever the drag passes over.
    Remove,
}

/// A building the tools would place, for drawing as a ghost.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ghost {
    pub building: BuildingId,
    pub tile: TilePos,
    pub dir: Dir,
}

#[derive(Debug)]
enum Gesture {
    Belt {
        path: Vec<(TilePos, Dir)>,
        /// The drag started on this belt: a tap without moving turns it.
        start: Option<Occupant>,
    },
    Place(TilePos),
    Remove(TilePos),
}

#[derive(Debug)]
pub struct Builder {
    pub tool: Tool,
    /// The way new buildings face.
    pub dir: Dir,
    pub recipe: Option<RecipeId>,
    gesture: Option<Gesture>,
}

const BELT: &str = "belt";

impl Default for Builder {
    fn default() -> Self {
        Self {
            tool: Tool::Hand,
            dir: Dir::East,
            recipe: None,
            gesture: None,
        }
    }
}

impl Builder {
    pub fn set_tool(&mut self, tool: Tool) {
        self.tool = tool;
        self.gesture = None;
    }

    pub fn rotate(&mut self) {
        self.dir = self.dir.rotate_cw();
    }

    /// A finger or the left button went down on `tile`. Returns commands to run now.
    pub fn start(&mut self, tile: TilePos, ground: &dyn Ground) -> Vec<Command> {
        self.gesture = match &self.tool {
            Tool::Hand => None,
            Tool::Belt => {
                let start = ground.at(tile).filter(|o| o.belt);
                let dir = start.map_or(self.dir, |o| o.dir);
                Some(Gesture::Belt {
                    path: vec![(tile, dir)],
                    start,
                })
            }
            Tool::Place(_) => Some(Gesture::Place(tile)),
            Tool::Remove => {
                self.gesture = Some(Gesture::Remove(tile));
                return remove(tile, ground);
            }
        };
        Vec::new()
    }

    /// The pointer moved onto `tile`.
    pub fn drag(&mut self, tile: TilePos, ground: &dyn Ground) -> Vec<Command> {
        match &mut self.gesture {
            Some(Gesture::Belt { path, .. }) => extend(path, tile),
            Some(Gesture::Place(t)) => *t = tile,
            Some(Gesture::Remove(t)) if *t != tile => {
                *t = tile;
                return remove(tile, ground);
            }
            _ => {}
        }
        Vec::new()
    }

    /// The pointer lifted: build what the gesture drew.
    pub fn end(&mut self, ground: &dyn Ground) -> Vec<Command> {
        let mut out = Vec::new();
        match self.gesture.take() {
            Some(Gesture::Belt { path, start }) => {
                if let (1, Some(s)) = (path.len(), start) {
                    // A tap on a belt turns it.
                    out.push(Command::Rotate {
                        entity: s.id,
                        dir: s.dir.rotate_cw(),
                    });
                    return out;
                }
                let last = path.len() - 1;
                for (i, &(tile, dir)) in path.iter().enumerate() {
                    match ground.at(tile) {
                        // Belts the path crosses turn to follow it; a drag that ends on
                        // a belt joins it rather than turning it.
                        Some(o) if o.belt && o.dir != dir && (i < last || last == 0) => {
                            out.push(Command::Rotate { entity: o.id, dir })
                        }
                        Some(_) => {}
                        None => out.push(Command::Place {
                            building: BELT.into(),
                            pos: tile,
                            dir,
                            recipe: None,
                        }),
                    }
                }
                if path.len() > 1 {
                    self.dir = path[last].1;
                }
            }
            Some(Gesture::Place(tile)) => {
                if let Tool::Place(building) = &self.tool {
                    out.push(Command::Place {
                        building: building.clone(),
                        pos: tile,
                        dir: self.dir,
                        recipe: self.recipe.clone(),
                    });
                }
            }
            Some(Gesture::Remove(_)) | None => {}
        }
        out
    }

    /// A second finger came down, or the gesture was cancelled: drop it.
    pub fn cancel(&mut self) {
        self.gesture = None;
    }

    /// What the current gesture would build, to draw as ghosts.
    pub fn ghosts(&self) -> Vec<Ghost> {
        match (&self.gesture, &self.tool) {
            (Some(Gesture::Belt { path, .. }), _) => path
                .iter()
                .map(|&(tile, dir)| Ghost {
                    building: BELT.into(),
                    tile,
                    dir,
                })
                .collect(),
            (Some(Gesture::Place(tile)), Tool::Place(b)) => vec![Ghost {
                building: b.clone(),
                tile: *tile,
                dir: self.dir,
            }],
            _ => Vec::new(),
        }
    }
}

fn remove(tile: TilePos, ground: &dyn Ground) -> Vec<Command> {
    match ground.at(tile) {
        Some(o) if o.removable => vec![Command::Remove { entity: o.id }],
        _ => Vec::new(),
    }
}

fn dir_between(a: TilePos, b: TilePos) -> Dir {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    if dx > 0 {
        Dir::East
    } else if dy > 0 {
        Dir::South
    } else if dx < 0 {
        Dir::West
    } else {
        Dir::North
    }
}

/// Grow the belt path one tile at a time towards `to`, the long axis first,
/// each tile pointing at the next. Dragging back onto the previous tile undoes
/// the last step.
fn extend(path: &mut Vec<(TilePos, Dir)>, to: TilePos) {
    let last = path[path.len() - 1].0;
    if last == to {
        return;
    }
    if path.len() > 1 && path[path.len() - 2].0 == to {
        path.pop();
        return;
    }
    let mut at = last;
    for _ in 0..200 {
        if at == to {
            break;
        }
        let (dx, dy) = (to.x - at.x, to.y - at.y);
        let next = if dx.abs() >= dy.abs() {
            TilePos::new(at.x + dx.signum(), at.y)
        } else {
            TilePos::new(at.x, at.y + dy.signum())
        };
        let dir = dir_between(at, next);
        let n = path.len();
        path[n - 1].1 = dir;
        path.push((next, dir));
        at = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Map(HashMap<TilePos, Occupant>);

    impl Ground for Map {
        fn at(&self, t: TilePos) -> Option<Occupant> {
            self.0.get(&t).copied()
        }
    }

    fn t(x: i32, y: i32) -> TilePos {
        TilePos::new(x, y)
    }

    fn belt(id: u32, dir: Dir) -> Occupant {
        Occupant {
            id: EntityId(id),
            belt: true,
            dir,
            removable: true,
        }
    }

    #[test]
    fn dragging_draws_an_l_shaped_belt_that_turns_the_corner() {
        let (mut b, map) = (Builder::default(), Map::default());
        b.set_tool(Tool::Belt);
        b.start(t(0, 0), &map);
        b.drag(t(3, 2), &map);
        let placed: Vec<_> = b
            .end(&map)
            .into_iter()
            .map(|c| match c {
                Command::Place { pos, dir, .. } => (pos.x, pos.y, dir),
                other => panic!("{other:?}"),
            })
            .collect();
        use Dir::*;
        assert_eq!(
            placed,
            vec![
                (0, 0, East),
                (1, 0, East),
                (2, 0, South),
                (2, 1, East),
                (3, 1, South),
                (3, 2, South)
            ]
        );
        assert_eq!(b.dir, South, "the next belt carries on the same way");
    }

    #[test]
    fn dragging_back_undoes_a_step_and_crossed_belts_turn() {
        let (mut b, mut map) = (Builder::default(), Map::default());
        map.0.insert(t(1, 0), belt(7, Dir::North));
        map.0.insert(t(3, 0), belt(8, Dir::North));
        b.set_tool(Tool::Belt);
        b.start(t(0, 0), &map);
        b.drag(t(4, 0), &map);
        b.drag(t(3, 0), &map);
        assert_eq!(b.ghosts().len(), 4);
        let cmds = b.end(&map);
        assert!(cmds.contains(&Command::Rotate {
            entity: EntityId(7),
            dir: Dir::East
        }));
        assert!(
            !cmds.iter().any(|c| matches!(
                c,
                Command::Rotate {
                    entity: EntityId(8),
                    ..
                }
            )),
            "the drag ends on belt 8, so it joins it: {cmds:?}"
        );
    }

    #[test]
    fn tapping_a_belt_turns_it() {
        let (mut b, mut map) = (Builder::default(), Map::default());
        map.0.insert(t(5, 5), belt(3, Dir::East));
        b.set_tool(Tool::Belt);
        b.start(t(5, 5), &map);
        assert_eq!(
            b.end(&map),
            vec![Command::Rotate {
                entity: EntityId(3),
                dir: Dir::South
            }]
        );
    }

    #[test]
    fn place_follows_the_finger_and_remove_sweeps() {
        let (mut b, mut map) = (Builder::default(), Map::default());
        b.set_tool(Tool::Place("furnace".into()));
        b.rotate();
        b.start(t(1, 1), &map);
        b.drag(t(2, 1), &map);
        assert_eq!(b.ghosts()[0].tile, t(2, 1));
        let placed = b.end(&map);
        assert!(
            matches!(&placed[..], [Command::Place { pos, dir: Dir::South, .. }] if *pos == t(2, 1))
        );

        map.0.insert(t(0, 0), belt(1, Dir::East));
        map.0.insert(t(1, 0), belt(2, Dir::East));
        map.0.insert(
            t(2, 0),
            Occupant {
                removable: false,
                ..belt(3, Dir::East)
            },
        );
        b.set_tool(Tool::Remove);
        let mut cmds = b.start(t(0, 0), &map);
        cmds.extend(b.drag(t(1, 0), &map));
        cmds.extend(b.drag(t(1, 0), &map)); // same tile again: nothing
        cmds.extend(b.drag(t(2, 0), &map)); // the hub: nothing
        assert_eq!(
            cmds,
            vec![
                Command::Remove {
                    entity: EntityId(1)
                },
                Command::Remove {
                    entity: EntityId(2)
                }
            ]
        );
    }
}
