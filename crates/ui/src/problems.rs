//! The problems list (T37, R40): everything in the factory that is stuck,
//! grouped by what it is and what's wrong, so a tap can take the camera
//! straight to it.

use crate::View;
use mofact_core::content::BuildingRole;
use mofact_core::ids::{BuildingId, EntityId};
use mofact_core::snapshot::MachineState;

/// What is wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Trouble {
    /// A grid gives less than its machines ask for.
    ShortPower,
    /// A powered machine off any grid, or on one that gives nothing.
    NoPower,
    /// A machine waiting for something to arrive.
    Starved,
    /// A machine that can't get rid of what it made.
    Blocked,
    /// A pipe or tank with nothing in it.
    EmptyPipe,
    /// A crafter with no recipe and nothing arriving to pick one.
    Idle,
}

/// One line of the list: the same trouble with the same kind of building.
#[derive(Clone, Debug, PartialEq)]
pub struct Problem {
    pub trouble: Trouble,
    /// The kind of building, for its icon; none for a power grid.
    pub building: Option<BuildingId>,
    pub label: String,
    /// Where each one is: the entity and its centre, in tiles.
    pub at: Vec<(EntityId, [f32; 2])>,
}

/// Everything stuck in `view`'s snapshot, worst first.
pub fn find(view: &View) -> Vec<Problem> {
    let mut out: Vec<Problem> = Vec::new();
    let snap = view.snapshot;
    let centre = |e: &mofact_core::snapshot::EntityView| {
        let (w, h) = view.building(&e.building).map_or((1, 1), |b| b.size);
        [
            e.pos.x as f32 + f32::from(w) / 2.0,
            e.pos.y as f32 + f32::from(h) / 2.0,
        ]
    };
    // Grids short of power, each at the first machine on it.
    for (g, grid) in snap.grids.iter().enumerate() {
        if grid.demand_kw <= 0.0 || grid.satisfaction >= 0.999 {
            continue;
        }
        let on = snap.entities.iter().filter(|e| e.grid == Some(g as u16));
        let at: Vec<_> = on
            .filter(|e| view.building(&e.building).is_some_and(|b| b.power > 0.0))
            .map(|e| (e.id, centre(e)))
            .collect();
        if at.is_empty() {
            continue;
        }
        out.push(Problem {
            trouble: Trouble::ShortPower,
            building: None,
            label: format!(
                "Power: {:.0} of {:.0} kW · {:.0}% met",
                grid.supply_kw,
                grid.demand_kw,
                grid.satisfaction * 100.0
            ),
            at,
        });
    }
    for e in &snap.entities {
        let Some(def) = view.building(&e.building) else {
            continue;
        };
        let pipe = matches!(
            def.role,
            BuildingRole::Pipe { .. }
                | BuildingRole::Tank { .. }
                | BuildingRole::UndergroundPipe { .. }
        );
        let crafter = matches!(def.role, BuildingRole::Crafter { .. });
        let trouble = match e.state {
            _ if pipe => {
                if e.fluid.is_some() {
                    continue;
                }
                Trouble::EmptyPipe
            }
            // Hubs, belts, poles and the like never stall.
            _ if !view.look.has_badge(&e.building) => continue,
            MachineState::NoPower => Trouble::NoPower,
            MachineState::Starved => Trouble::Starved,
            MachineState::Blocked => Trouble::Blocked,
            MachineState::Idle if crafter => Trouble::Idle,
            _ => continue,
        };
        let here = (e.id, centre(e));
        match out
            .iter_mut()
            .find(|p| p.trouble == trouble && p.building.as_ref() == Some(&e.building))
        {
            Some(p) => p.at.push(here),
            None => {
                let what = match trouble {
                    Trouble::NoPower => "no power",
                    Trouble::Starved => "waiting for input",
                    Trouble::Blocked => "output full",
                    Trouble::EmptyPipe => "empty",
                    _ => "no recipe",
                };
                out.push(Problem {
                    trouble,
                    building: Some(e.building.clone()),
                    label: format!("{} · {what}", def.name),
                    at: vec![here],
                });
            }
        }
    }
    out.sort_by_key(|p| p.trouble);
    out
}
