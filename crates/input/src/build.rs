//! Build tools (T20): place, drag belts, rotate, remove, ported from the
//! prototype's pointer logic. The tools turn tile positions into simulation
//! [`Command`]s; they ask a [`Ground`] what is already built.

use mofact_core::command::Command;
use mofact_core::geom::{Dir, TilePos};
use mofact_core::ids::{BuildingId, EntityId, RecipeId};

use crate::blueprint::Blueprint;

/// The most tiles a side a selection box spans.
pub const SELECT_MAX: i32 = 96;

/// What stands on a tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Occupant {
    pub id: EntityId,
    pub belt: bool,
    pub dir: Dir,
    /// The hub can't be removed.
    pub removable: bool,
    /// Rails, signals and stops: trains go on them, and signals and stops
    /// can be put on a rail in its place.
    pub track: bool,
}

/// The built world, as the tools see it.
pub trait Ground {
    fn at(&self, tile: TilePos) -> Option<Occupant>;
    /// Whether a tree or boulder stands on an empty tile, for the remove
    /// tool to clear.
    fn tree(&self, _tile: TilePos) -> bool {
        false
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tool {
    /// Pan and inspect.
    Hand,
    /// Draw belts along the drag path.
    Belt,
    /// Tap to place one building; a drag pans instead (see [`Controls`](crate::Controls)).
    Place(BuildingId),
    /// Mark whatever the drag passes over, and remove it all when the finger lifts.
    Remove,
    /// Drag a box round buildings to select them (T51), for copying,
    /// moving, deleting or keeping as a blueprint.
    Select,
    /// A blueprint in hand ([`Builder::paste`]): a drag slides it, a tap
    /// puts it under the finger, and the HUD's Place button builds it.
    Paste,
}

/// A blueprint in hand, where it would go, and what it moves.
#[derive(Clone, Debug, PartialEq)]
pub struct Paste {
    pub blueprint: Blueprint,
    /// Its top-left corner.
    pub at: TilePos,
    /// The buildings it was cut from: building it removes them first, so
    /// the group moves. Empty for a copy.
    pub moving: Vec<EntityId>,
}

/// A building the tools would place, for drawing as a ghost.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ghost {
    pub building: BuildingId,
    pub tile: TilePos,
    pub dir: Dir,
    pub recipe: Option<RecipeId>,
}

#[derive(Debug)]
enum Gesture {
    Belt {
        path: Vec<(TilePos, Dir)>,
        /// The drag started on this belt: a tap without moving turns it.
        start: Option<Occupant>,
    },
    Place(TilePos),
    /// A building drawn along the drag, as track is.
    Line(Vec<(TilePos, Dir)>),
    /// What the finger has passed over so far, in the order it got there,
    /// and the trees it passed over.
    Remove(Vec<EntityId>, Vec<TilePos>),
    /// A selection box from one corner tile to the other.
    Select(TilePos, TilePos),
    /// A blueprint being slid: the tile the finger was last on, and
    /// whether it has left the first.
    Slide(TilePos, bool),
}

#[derive(Debug)]
pub struct Builder {
    pub tool: Tool,
    /// The way new buildings face.
    pub dir: Dir,
    pub recipe: Option<RecipeId>,
    /// The belt tier the belt tool lays.
    pub belt: BuildingId,
    /// Buildings drawn along a drag rather than placed by a tap: track.
    pub lines: Vec<BuildingId>,
    /// Buildings placed on track: signals, stops and trains.
    pub on_track: Vec<BuildingId>,
    /// What the select tool has selected.
    pub selection: Vec<EntityId>,
    /// The blueprint in hand, with the paste tool.
    pub paste: Option<Paste>,
    gesture: Option<Gesture>,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            tool: Tool::Hand,
            dir: Dir::East,
            recipe: None,
            belt: "belt".into(),
            lines: Vec::new(),
            on_track: Vec::new(),
            selection: Vec::new(),
            paste: None,
            gesture: None,
        }
    }
}

impl Builder {
    pub fn set_tool(&mut self, tool: Tool) {
        if tool != Tool::Select {
            self.selection.clear();
        }
        if tool != Tool::Paste {
            self.paste = None;
        }
        self.tool = tool;
        self.gesture = None;
    }

    /// Take `blueprint` in hand with its corner at `at`; `moving` names
    /// the buildings it was cut from.
    pub fn hold(&mut self, blueprint: Blueprint, at: TilePos, moving: Vec<EntityId>) {
        self.set_tool(Tool::Paste);
        self.paste = Some(Paste {
            blueprint,
            at,
            moving,
        });
    }

    /// The commands that build the blueprint in hand where it is: for a
    /// move, removing what it was cut from first. A move is done once
    /// built, and puts the blueprint away; a copy stays in hand to paste again.
    pub fn build_paste(&mut self) -> Vec<Command> {
        let Some(p) = &self.paste else {
            return Vec::new();
        };
        let mut out: Vec<Command> = p
            .moving
            .iter()
            .map(|&entity| Command::Remove { entity })
            .collect();
        out.extend(p.blueprint.commands(p.at));
        if !p.moving.is_empty() {
            self.set_tool(Tool::Select);
        }
        out
    }

    /// The tiles the blueprint in hand would go on that are taken: by a
    /// building it isn't moving, or by a tree.
    pub fn blocked(&self, ground: &dyn Ground) -> Vec<TilePos> {
        let Some(p) = &self.paste else {
            return Vec::new();
        };
        p.blueprint
            .tiles(p.at)
            .filter(|&t| match ground.at(t) {
                Some(o) => !p.moving.contains(&o.id),
                None => ground.tree(t),
            })
            .collect()
    }

    /// The selection box being drawn: its top-left tile and size.
    pub fn selecting(&self) -> Option<(TilePos, (i32, i32))> {
        match &self.gesture {
            Some(Gesture::Select(a, b)) => {
                let (a, b) = clamp_box(*a, *b);
                Some((
                    TilePos::new(a.x.min(b.x), a.y.min(b.y)),
                    ((a.x - b.x).abs() + 1, (a.y - b.y).abs() + 1),
                ))
            }
            _ => None,
        }
    }

    /// Whether the building in hand is drawn along a drag.
    pub fn draws_line(&self) -> bool {
        matches!(&self.tool, Tool::Place(b) if self.lines.contains(b))
    }

    /// Whether the building in hand goes on what stands at a tile: only
    /// on nothing, or on track if it is a signal, stop or train.
    pub fn places_on(&self, here: Option<Occupant>) -> bool {
        match (&self.tool, here) {
            (_, None) => true,
            (Tool::Place(b), Some(o)) => o.track && self.on_track.contains(b),
            _ => false,
        }
    }

    /// Turn the way new buildings face, or the blueprint in hand about its middle.
    pub fn rotate(&mut self) {
        match &mut self.paste {
            Some(p) => {
                let (w, h) = p.blueprint.size;
                p.blueprint.rotate();
                p.at = TilePos::new(p.at.x + (w - h) / 2, p.at.y + (h - w) / 2);
            }
            None => self.dir = self.dir.rotate_cw(),
        }
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
            Tool::Place(_) if self.draws_line() => Some(Gesture::Line(vec![(tile, self.dir)])),
            Tool::Place(_) => Some(Gesture::Place(tile)),
            Tool::Remove => {
                let (mut marks, mut trees) = (Vec::new(), Vec::new());
                mark(&mut marks, &mut trees, tile, ground);
                Some(Gesture::Remove(marks, trees))
            }
            Tool::Select => Some(Gesture::Select(tile, tile)),
            Tool::Paste => Some(Gesture::Slide(tile, false)),
        };
        Vec::new()
    }

    /// The pointer moved onto `tile`.
    pub fn drag(&mut self, tile: TilePos, ground: &dyn Ground) -> Vec<Command> {
        match &mut self.gesture {
            Some(Gesture::Belt { path, .. }) => extend(path, tile),
            Some(Gesture::Line(path)) => *path = corner(path[0].0, tile),
            Some(Gesture::Place(t)) => *t = tile,
            Some(Gesture::Remove(marks, trees)) => mark(marks, trees, tile, ground),
            Some(Gesture::Select(_, b)) => *b = tile,
            Some(Gesture::Slide(last, moved)) => {
                if let Some(p) = &mut self.paste {
                    p.at = TilePos::new(p.at.x + tile.x - last.x, p.at.y + tile.y - last.y);
                }
                *moved |= tile != *last;
                *last = tile;
            }
            None => {}
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
                            building: self.belt.clone(),
                            pos: tile,
                            dir,
                            recipe: None,
                        }),
                    }
                }
                // `self.dir` stays as it was: drawing a belt must not change
                // which way the next machine faces behind the player's back.
            }
            // Track goes on every free tile along the way; it joins up by itself.
            Some(Gesture::Line(path)) => {
                if let Tool::Place(building) = &self.tool {
                    for (tile, _) in path {
                        if ground.at(tile).is_none() {
                            out.push(Command::Place {
                                building: building.clone(),
                                pos: tile,
                                dir: self.dir,
                                recipe: None,
                            });
                        }
                    }
                }
            }
            // A tap on something already built inspects it instead (the game does that).
            Some(Gesture::Place(tile)) if !self.places_on(ground.at(tile)) => {}
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
            Some(Gesture::Remove(marks, trees)) => {
                out.extend(marks.into_iter().map(|entity| Command::Remove { entity }));
                out.extend(trees.into_iter().map(|tile| Command::Clear { tile }));
            }
            // Everything removable the box touches; a tap picks one building.
            Some(Gesture::Select(a, b)) => {
                let (a, b) = clamp_box(a, b);
                self.selection.clear();
                for y in a.y.min(b.y)..=a.y.max(b.y) {
                    for x in a.x.min(b.x)..=a.x.max(b.x) {
                        if let Some(o) = ground.at(TilePos::new(x, y)) {
                            if o.removable && !self.selection.contains(&o.id) {
                                self.selection.push(o.id);
                            }
                        }
                    }
                }
            }
            // A tap puts the blueprint's middle under the finger.
            Some(Gesture::Slide(tile, false)) => {
                if let Some(p) = &mut self.paste {
                    let (w, h) = p.blueprint.size;
                    p.at = TilePos::new(tile.x - (w - 1) / 2, tile.y - (h - 1) / 2);
                }
            }
            Some(Gesture::Slide(_, true)) | None => {}
        }
        out
    }

    /// What a remove drag has marked so far, to draw before it goes.
    pub fn marked(&self) -> &[EntityId] {
        match &self.gesture {
            Some(Gesture::Remove(marks, _)) => marks,
            _ => &[],
        }
    }

    /// The trees a remove drag has marked so far.
    pub fn marked_trees(&self) -> &[TilePos] {
        match &self.gesture {
            Some(Gesture::Remove(_, trees)) => trees,
            _ => &[],
        }
    }

    /// A second finger came down, or the gesture was cancelled: drop it.
    pub fn cancel(&mut self) {
        self.gesture = None;
    }

    /// What the current gesture would build, to draw as ghosts.
    pub fn ghosts(&self) -> Vec<Ghost> {
        match (&self.gesture, &self.tool) {
            (_, Tool::Paste) => self
                .paste
                .iter()
                .flat_map(|p| {
                    p.blueprint
                        .placed(p.at)
                        .map(|(b, tile, _, dir, recipe)| Ghost {
                            building: b.clone(),
                            tile,
                            dir,
                            recipe: recipe.cloned(),
                        })
                })
                .collect(),
            (Some(Gesture::Belt { path, .. }), _) => path
                .iter()
                .map(|&(tile, dir)| Ghost {
                    building: self.belt.clone(),
                    tile,
                    dir,
                    recipe: None,
                })
                .collect(),
            (Some(Gesture::Line(path)), Tool::Place(b)) => path
                .iter()
                .map(|&(tile, _)| Ghost {
                    building: b.clone(),
                    tile,
                    dir: self.dir,
                    recipe: None,
                })
                .collect(),
            (Some(Gesture::Place(tile)), Tool::Place(b)) => vec![Ghost {
                building: b.clone(),
                tile: *tile,
                dir: self.dir,
                recipe: self.recipe.clone(),
            }],
            _ => Vec::new(),
        }
    }
}

/// A selection box from `a` towards `b`, at most [`SELECT_MAX`] a side.
fn clamp_box(a: TilePos, b: TilePos) -> (TilePos, TilePos) {
    let c = |from: i32, to: i32| from + (to - from).clamp(1 - SELECT_MAX, SELECT_MAX - 1);
    (a, TilePos::new(c(a.x, b.x), c(a.y, b.y)))
}

/// Mark what stands on `tile` for removal, once; the hub can't be marked.
fn mark(marks: &mut Vec<EntityId>, trees: &mut Vec<TilePos>, tile: TilePos, ground: &dyn Ground) {
    match ground.at(tile) {
        Some(o) if o.removable && !marks.contains(&o.id) => marks.push(o.id),
        Some(_) => {}
        None if ground.tree(tile) && !trees.contains(&tile) => trees.push(tile),
        None => {}
    }
}

/// The tiles from `a` to `b` along the longer axis first, then the
/// shorter: a straight run with at most one bend, as track is laid.
fn corner(a: TilePos, b: TilePos) -> Vec<(TilePos, Dir)> {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let bend = if dx.abs() >= dy.abs() {
        TilePos::new(b.x, a.y)
    } else {
        TilePos::new(a.x, b.y)
    };
    let mut path = vec![(a, Dir::East)];
    for to in [bend, b] {
        let mut at = path[path.len() - 1].0;
        while at != to {
            let next = TilePos::new(at.x + (to.x - at.x).signum(), at.y + (to.y - at.y).signum());
            let n = path.len();
            path[n - 1].1 = dir_between(at, next);
            path.push((next, dir_between(at, next)));
            at = next;
        }
    }
    path
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
            track: false,
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
        assert_eq!(
            b.dir, East,
            "drawing a belt leaves the machine facing alone"
        );
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
    fn tapping_something_built_places_nothing_and_a_cancelled_sweep_removes_nothing() {
        let (mut b, mut map) = (Builder::default(), Map::default());
        map.0.insert(t(4, 4), belt(9, Dir::East));
        b.set_tool(Tool::Place("drill".into()));
        b.start(t(4, 4), &map);
        assert!(b.end(&map).is_empty(), "the game inspects it instead");
        b.set_tool(Tool::Remove);
        b.start(t(4, 4), &map);
        assert_eq!(b.marked(), [EntityId(9)]);
        b.cancel(); // a second finger, or the radial menu opening
        assert!(b.marked().is_empty());
        assert!(b.end(&map).is_empty());
    }

    #[test]
    fn track_is_drawn_along_the_drag_on_free_tiles() {
        let mut map = Map::default();
        map.0.insert(t(2, 0), belt(9, Dir::East));
        let mut b = Builder {
            lines: vec!["rail".into()],
            ..Builder::default()
        };
        b.set_tool(Tool::Place("rail".into()));
        assert!(b.draws_line());
        b.start(t(0, 0), &map);
        b.drag(t(3, 2), &map);
        assert_eq!(b.ghosts().len(), 6);
        let placed: Vec<_> = b
            .end(&map)
            .into_iter()
            .map(|c| match c {
                Command::Place { pos, .. } => (pos.x, pos.y),
                other => panic!("{other:?}"),
            })
            .collect();
        // The long way first, then the turn; the built tile is skipped.
        assert_eq!(placed, [(0, 0), (1, 0), (3, 0), (3, 1), (3, 2)]);
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
        cmds.extend(b.drag(t(0, 0), &map)); // back over the first: marked once
        cmds.extend(b.drag(t(2, 0), &map)); // the hub: never marked
        assert!(cmds.is_empty(), "nothing goes until the finger lifts");
        assert_eq!(b.marked(), [EntityId(1), EntityId(2)]);
        assert_eq!(
            b.end(&map),
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

    #[test]
    fn a_box_selects_what_it_touches_and_a_copy_slides_and_builds() {
        let mut map = Map::default();
        map.0.insert(t(0, 0), belt(1, Dir::East));
        map.0.insert(t(1, 0), belt(2, Dir::East));
        map.0.insert(t(5, 5), belt(3, Dir::East));
        map.0.insert(
            t(1, 1),
            Occupant {
                removable: false,
                ..belt(4, Dir::East)
            },
        );
        let mut b = Builder::default();
        b.set_tool(Tool::Select);
        b.start(t(1, 1), &map);
        b.drag(t(0, 0), &map);
        assert_eq!(b.selecting(), Some((t(0, 0), (2, 2))));
        assert!(b.end(&map).is_empty(), "selecting builds nothing");
        assert_eq!(b.selection, [EntityId(1), EntityId(2)], "never the hub");
        // A tap picks one.
        b.start(t(5, 5), &map);
        b.end(&map);
        assert_eq!(b.selection, [EntityId(3)]);

        let bp = Blueprint::capture(
            "Blueprint 1",
            [
                ("belt".into(), t(0, 0), 1, Dir::East, None),
                ("belt".into(), t(1, 0), 1, Dir::East, None),
            ],
        );
        b.hold(bp, t(0, 0), Vec::new());
        assert!(b.selection.is_empty());
        assert_eq!(b.blocked(&map), [t(0, 0), t(1, 0)]);
        // A drag slides it by as many tiles as the finger goes.
        b.start(t(3, 3), &map);
        b.drag(t(4, 5), &map);
        b.end(&map);
        assert_eq!(b.paste.as_ref().unwrap().at, t(1, 2));
        // A tap puts its middle under the finger.
        b.start(t(10, 10), &map);
        b.end(&map);
        assert_eq!(b.paste.as_ref().unwrap().at, t(10, 10));
        assert_eq!(b.ghosts().len(), 2);
        assert!(b.blocked(&map).is_empty());
        let built = b.build_paste();
        assert_eq!(built.len(), 2);
        assert!(
            matches!(&built[1], Command::Place { pos, .. } if *pos == t(11, 10)),
            "{built:?}"
        );
        assert_eq!(b.tool, Tool::Paste, "a copy stays in hand");
        b.rotate();
        let p = b.paste.as_ref().unwrap();
        assert_eq!(p.blueprint.size, (1, 2));
        assert_eq!(p.blueprint.pieces[0].dir, Dir::South);
    }

    #[test]
    fn a_move_removes_the_originals_first_and_ignores_them_when_blocked() {
        let mut map = Map::default();
        map.0.insert(t(0, 0), belt(1, Dir::East));
        map.0.insert(t(1, 0), belt(2, Dir::East));
        let bp = Blueprint::capture(
            "",
            [
                ("belt".into(), t(0, 0), 1, Dir::East, None),
                ("belt".into(), t(1, 0), 1, Dir::East, None),
            ],
        );
        let mut b = Builder::default();
        b.hold(bp, t(0, 0), vec![EntityId(1), EntityId(2)]);
        b.start(t(0, 0), &map);
        b.drag(t(1, 0), &map);
        b.end(&map);
        assert!(b.blocked(&map).is_empty(), "it overlaps only itself");
        let built = b.build_paste();
        assert_eq!(
            built[..2],
            [
                Command::Remove {
                    entity: EntityId(1)
                },
                Command::Remove {
                    entity: EntityId(2)
                }
            ]
        );
        assert!(matches!(&built[2], Command::Place { pos, .. } if *pos == t(1, 0)));
        assert_eq!(b.tool, Tool::Select, "a move is done once built");
        assert!(b.paste.is_none());
    }

    #[test]
    fn a_selection_box_stops_at_its_largest() {
        let map = Map::default();
        let mut b = Builder::default();
        b.set_tool(Tool::Select);
        b.start(t(0, 0), &map);
        b.drag(t(1000, -1000), &map);
        assert_eq!(
            b.selecting(),
            Some((t(0, 1 - SELECT_MAX), (SELECT_MAX, SELECT_MAX)))
        );
    }
}
