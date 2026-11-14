//! The first-hour coach: on the early goals it shows a new player where
//! things go, right on the map.
//!
//! On the first goal it names each part of the starter line. On the "more
//! drills" goal it lays out a whole second line (a drill on free iron ore, a
//! furnace in front of it, a belt to the hub) as faint ghosts, marks the
//! next piece to build and which tool builds it, and turns that tool the
//! right way when the player picks it.

use crate::ore::OreMap;
use mofact_core::content::{BuildingRole as Role, Content, Want};
use mofact_core::geom::{Dir, TilePos};
use mofact_core::ids::BuildingId;
use mofact_core::snapshot::Snapshot;
use mofact_input::Tool;
use std::collections::HashSet;

/// How far from the hub a guided line may reach for ore.
const REACH: i32 = 12;

/// One piece of a guided line: a building on some tiles, all facing `dir`.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub building: BuildingId,
    pub tiles: Vec<TilePos>,
    pub dir: Dir,
}

impl Step {
    /// The tool that builds it.
    pub fn tool(&self) -> Tool {
        if self.building.as_str() == "belt" {
            Tool::Belt
        } else {
            Tool::Place(self.building.clone())
        }
    }

    /// What to tell the player while it is the next piece.
    pub fn say(&self) -> &'static str {
        match self.building.as_str() {
            "drill" => mofact_ui::tr("guide.step_drill"),
            "furnace" => mofact_ui::tr("guide.step_furnace"),
            _ => mofact_ui::tr("guide.step_belt"),
        }
    }
}

/// What the coach shows this frame.
pub enum Coach {
    /// Labels over the starter line: tile, size in tiles, and text.
    Labels(Vec<(TilePos, (u8, u8), &'static str, bool)>),
    /// A line to build, which of its steps is next, and whether its label
    /// goes under it (when the row above is built on).
    Line(Vec<Step>, usize, bool),
}

#[derive(Default)]
pub struct Guide {
    plan: Option<Vec<Step>>,
    /// The tool the player held last frame, to notice a new pick.
    held: Option<Tool>,
}

impl Guide {
    /// Work out what to show for `snap`. `tool` is the tool in hand; when the
    /// player has just picked the tool for the next step, the way it should
    /// face comes back too.
    pub fn update(
        &mut self,
        content: &Content,
        snap: &Snapshot,
        ore: &OreMap,
        tool: &Tool,
    ) -> (Option<Coach>, Option<Dir>) {
        let picked = self.held.as_ref() != Some(tool);
        self.held = Some(tool.clone());
        let Some(goal) = snap.goal.as_ref() else {
            self.plan = None;
            return (None, None);
        };
        let Some(def) = content.goals.get(usize::from(goal.index)) else {
            return (None, None);
        };
        match &def.want {
            Want::Deliver { item, .. } if goal.index == 0 && item.as_str() == "iron-plate" => {
                self.plan = None;
                (Some(Coach::Labels(labels(content, snap))), None)
            }
            Want::Build { building, .. } if building.as_str() == "drill" => {
                self.line(content, snap, ore, tool, picked, true)
            }
            // The drills goal is met by the line's drill, before its furnace
            // and belt are down; keep showing the rest of that line so it
            // isn't left as a drill with nowhere to put its ore.
            _ if self.plan.is_some() => self.line(content, snap, ore, tool, picked, false),
            _ => (None, None),
        }
    }

    /// The line to build and its next step. With `find_new`, a new line is
    /// worked out when there is none or the last one is finished or built
    /// over; without it, only the line already started is finished.
    fn line(
        &mut self,
        content: &Content,
        snap: &Snapshot,
        ore: &OreMap,
        tool: &Tool,
        picked: bool,
        find_new: bool,
    ) -> (Option<Coach>, Option<Dir>) {
        let occupied = occupied(content, snap);
        let built = |s: &Step, t: &TilePos| {
            snap.entities.iter().any(|e| {
                e.pos == *t
                    && (e.building == s.building
                        || (s.building.as_str() == "belt" && is_belt(content, &e.building)))
            })
        };
        // Keep the line being built while every tile is either done
        // or still free; otherwise find a new one.
        let ok = self.plan.as_ref().is_some_and(|p| {
            p.iter()
                .all(|s| s.tiles.iter().all(|t| built(s, t) || !occupied.contains(t)))
                && !p.iter().all(|s| s.tiles.iter().all(|t| built(s, t)))
        });
        if !ok {
            self.plan = if find_new {
                find(content, snap, ore, &occupied)
            } else {
                None
            };
        }
        let Some(plan) = self.plan.clone() else {
            return (None, None);
        };
        let next = plan
            .iter()
            .position(|s| !s.tiles.iter().all(|t| built(s, t)))
            .unwrap_or(0);
        // Past the drills goal, only a line whose drill is down is worth
        // finishing.
        if !find_new && next == 0 {
            self.plan = None;
            return (None, None);
        }
        let dir = (picked && *tool == plan[next].tool()).then_some(plan[next].dir);
        let below = plan[next]
            .tiles
            .iter()
            .any(|t| occupied.contains(&TilePos::new(t.x, t.y - 1)));
        (Some(Coach::Line(plan, next, below)), dir)
    }
}

fn is_belt(content: &Content, id: &BuildingId) -> bool {
    content
        .buildings
        .iter()
        .any(|b| &b.id == id && matches!(b.role, Role::Belt { .. }))
}

fn size(content: &Content, id: &BuildingId) -> (u8, u8) {
    content
        .buildings
        .iter()
        .find(|b| &b.id == id)
        .map_or((1, 1), |b| b.size)
}

fn occupied(content: &Content, snap: &Snapshot) -> HashSet<TilePos> {
    let mut set = HashSet::new();
    for e in &snap.entities {
        let (w, h) = size(content, &e.building);
        for dy in 0..i32::from(h) {
            for dx in 0..i32::from(w) {
                set.insert(TilePos::new(e.pos.x + dx, e.pos.y + dy));
            }
        }
    }
    set
}

/// Names over the first drill, furnace and hub. The last flag puts the
/// label under the building, so neighbours' labels don't overlap.
fn labels(content: &Content, snap: &Snapshot) -> Vec<(TilePos, (u8, u8), &'static str, bool)> {
    let first = |id: &str| snap.entities.iter().find(|e| e.building.as_str() == id);
    let mut out = Vec::new();
    for (id, text, below) in [
        ("drill", "guide.drill", false),
        ("furnace", "guide.furnace", true),
        ("hub", "guide.hub", false),
    ] {
        if let Some(e) = first(id) {
            out.push((
                e.pos,
                size(content, &e.building),
                mofact_ui::tr(text),
                below,
            ));
        }
    }
    out
}

/// The shortest straight line into a side of the hub: belts from the hub's
/// edge outwards, then a furnace, then a drill on free iron ore, all facing
/// the hub.
fn find(
    content: &Content,
    snap: &Snapshot,
    ore: &OreMap,
    occupied: &HashSet<TilePos>,
) -> Option<Vec<Step>> {
    let hub = snap.entities.iter().find(|e| {
        content
            .buildings
            .iter()
            .any(|b| b.id == e.building && matches!(b.role, Role::Hub))
    })?;
    let (w, h) = size(content, &hub.building);
    let (w, h) = (i32::from(w), i32::from(h));
    let (x0, y0) = (hub.pos.x, hub.pos.y);
    let gen = ore.gen();
    let free = |t: &TilePos| {
        !occupied.contains(t)
            && (mofact_world::obstacle(&gen, *t).is_none() || snap.cleared.binary_search(t).is_ok())
    };
    let iron = |t: &TilePos| ore.ore_at(*t).is_some_and(|i| i.as_str() == "iron-ore");
    let mut best: Option<Vec<TilePos>> = None;
    for out in [Dir::West, Dir::North, Dir::South, Dir::East] {
        let lanes: Vec<TilePos> = match out {
            Dir::West => (0..h).map(|k| TilePos::new(x0 - 1, y0 + k)).collect(),
            Dir::East => (0..h).map(|k| TilePos::new(x0 + w, y0 + k)).collect(),
            Dir::North => (0..w).map(|k| TilePos::new(x0 + k, y0 - 1)).collect(),
            Dir::South => (0..w).map(|k| TilePos::new(x0 + k, y0 + h)).collect(),
        };
        let (dx, dy) = out.offset();
        for start in lanes {
            let mut path = Vec::new();
            for k in 0..REACH {
                let t = TilePos::new(start.x + dx * k, start.y + dy * k);
                if !free(&t) {
                    break;
                }
                path.push(t);
                // The drill needs a furnace tile in front of it.
                if path.len() >= 2 && iron(&t) {
                    if best.as_ref().is_none_or(|b| path.len() < b.len()) {
                        best = Some(path.clone());
                    }
                    break;
                }
            }
        }
    }
    let path = best?;
    let toward = {
        let (a, b) = (path[path.len() - 1], path[path.len() - 2]);
        Dir::ALL
            .into_iter()
            .find(|d| d.offset() == (b.x - a.x, b.y - a.y))?
    };
    let n = path.len();
    let mut steps = vec![
        Step {
            building: "drill".into(),
            tiles: vec![path[n - 1]],
            dir: toward,
        },
        Step {
            building: "furnace".into(),
            tiles: vec![path[n - 2]],
            dir: toward,
        },
    ];
    if n > 2 {
        // Belts listed from the furnace towards the hub, the way the drag goes.
        steps.push(Step {
            building: "belt".into(),
            tiles: path[..n - 2].iter().rev().copied().collect(),
            dir: toward,
        });
    }
    Some(steps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_guided_line_runs_from_free_iron_ore_into_the_hub() {
        let content = crate::bundled::content().unwrap();
        let (sim, mut ore) = crate::game::new_game(&content, 7).unwrap();
        let camera = crate::game::framing(&content, &sim);
        ore.fill(&camera, (2000, 2000));
        let snap = sim.snapshot();
        let taken = occupied(&content, &snap);
        let plan = find(&content, &snap, &ore, &taken).expect("a second line fits");
        assert_eq!(plan[0].building.as_str(), "drill");
        assert_eq!(plan[1].building.as_str(), "furnace");
        let drill = plan[0].tiles[0];
        assert_eq!(ore.ore_at(drill).map(|i| i.as_str()), Some("iron-ore"));
        assert!(plan
            .iter()
            .flat_map(|s| &s.tiles)
            .all(|t| !taken.contains(t)));
        // The last piece sits right against the hub, facing into it.
        let last = plan.last().unwrap();
        let end = *last.tiles.last().unwrap();
        let (dx, dy) = last.dir.offset();
        assert!(taken.contains(&TilePos::new(end.x + dx, end.y + dy)));
    }

    #[test]
    fn a_started_line_is_still_shown_after_the_drills_goal() {
        let content = crate::bundled::content().unwrap();
        let (sim, mut ore) = crate::game::new_game(&content, 7).unwrap();
        let camera = crate::game::framing(&content, &sim);
        ore.fill(&camera, (2000, 2000));
        let mut snap = sim.snapshot();
        let drills = content
            .goals
            .iter()
            .position(
                |g| matches!(&g.want, Want::Build { building, .. } if building.as_str() == "drill"),
            )
            .unwrap();
        snap.goal.as_mut().unwrap().index = drills as u16;
        let mut guide = Guide::default();
        let Some(Coach::Line(plan, 0, _)) = guide.update(&content, &snap, &ore, &Tool::Hand).0
        else {
            panic!("the drills goal shows a line");
        };
        // The next goal comes before anything of the line is built: nothing
        // to finish, so nothing shows.
        let mut next = snap.clone();
        next.goal.as_mut().unwrap().index += 1;
        assert!(guide.update(&content, &next, &ore, &Tool::Hand).0.is_none());

        // With the line's drill down, the goal moving on keeps the line.
        let mut guide = Guide::default();
        guide.update(&content, &snap, &ore, &Tool::Hand);
        let mut drill = snap
            .entities
            .iter()
            .find(|e| e.building.as_str() == "drill")
            .unwrap()
            .clone();
        drill.pos = plan[0].tiles[0];
        next.entities.push(drill);
        let Some(Coach::Line(_, 1, _)) = guide.update(&content, &next, &ore, &Tool::Hand).0 else {
            panic!("the rest of the line still shows");
        };
        // Once every piece is down, the coach is done.
        let model = next.entities.last().unwrap().clone();
        for step in &plan[1..] {
            for t in &step.tiles {
                let mut e = model.clone();
                e.building = step.building.clone();
                e.pos = *t;
                next.entities.push(e);
            }
        }
        assert!(guide.update(&content, &next, &ore, &Tool::Hand).0.is_none());
    }
}
