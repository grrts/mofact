//! The ratio calculator (S6, R41): for a target rate of one item, how many
//! of each machine the whole chain under it needs, and how many belts carry
//! the result.

use mofact_core::content::{BuildingRole, Content, IconShape, ItemKind, RecipeDef};
use mofact_core::ids::{BuildingId, ItemId, RecipeId};

/// The rates the calculator steps through, in items a minute.
pub const RATES: [u32; 8] = [6, 15, 30, 60, 120, 240, 600, 1200];

/// The rate it starts on, as an index into [`RATES`].
pub const START: usize = 3;

/// Items a belt lane carries per tile: one every 0.34 tiles, as the sim
/// spaces them.
const PER_TILE: f32 = 1.0 / 0.34;

/// How one item in the chain is had.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// Crafted by `machines` of `building`, set to `recipe`.
    Craft {
        recipe: RecipeId,
        building: BuildingId,
        machines: u32,
    },
    /// Mined, or drawn from the ground, by `machines` of `building`.
    Ground { building: BuildingId, machines: u32 },
    /// Nothing open makes it: it comes from elsewhere, such as a boiler.
    Bought,
}

/// One item of the chain and how much of it a second the target needs.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub item: ItemId,
    pub per_second: f32,
    pub source: Source,
}

/// The whole chain for a target rate, the target first, then what goes
/// into it, deepest last.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Plan {
    pub steps: Vec<Step>,
    /// The belt that carries the target and how many of it, for a solid.
    pub belts: Option<(BuildingId, u32)>,
}

impl Plan {
    /// The machines of each building the chain needs, in total.
    pub fn machines(&self) -> Vec<(BuildingId, u32)> {
        let mut out: Vec<(BuildingId, u32)> = Vec::new();
        for s in &self.steps {
            let (Source::Craft {
                building, machines, ..
            }
            | Source::Ground { building, machines }) = &s.source
            else {
                continue;
            };
            match out.iter_mut().find(|(b, _)| b == building) {
                Some((_, n)) => *n += machines,
                None => out.push((building.clone(), *machines)),
            }
        }
        out
    }
}

/// How many of a thing doing `each` a second meet `need`, rounded up but
/// forgiving the float noise of an exact fit.
fn count(need: f32, each: f32) -> u32 {
    if need <= 0.0 || each <= 0.0 {
        return 0;
    }
    (need / each - 1e-4).ceil().max(1.0) as u32
}

/// The plan for `per_minute` of `item`, using only what `open` allows: the
/// first open recipe that makes each item, on the fastest open machine.
pub fn plan(
    content: &Content,
    item: &ItemId,
    per_minute: f32,
    recipe_open: impl Fn(&RecipeId) -> bool,
    building_open: impl Fn(&BuildingId) -> bool,
) -> Plan {
    let mut plan = Plan::default();
    let mut chain = Vec::new();
    walk(
        content,
        item,
        per_minute / 60.0,
        &recipe_open,
        &building_open,
        &mut chain,
        &mut plan.steps,
    );
    // Machines for the summed rate of each item, not per branch.
    for s in &mut plan.steps {
        s.source = source(content, &s.item, s.per_second, &recipe_open, &building_open);
    }
    let solid = content
        .items
        .iter()
        .find(|i| &i.id == item)
        .is_some_and(|i| i.kind == ItemKind::Solid);
    if solid {
        let belts = content.buildings.iter().filter_map(|b| match b.role {
            BuildingRole::Belt { tiles_per_second } if building_open(&b.id) => {
                Some((b.id.clone(), tiles_per_second * PER_TILE))
            }
            _ => None,
        });
        // The slowest belt that carries it all, else as many of the fastest as it takes.
        let need = per_minute / 60.0;
        let mut belts: Vec<_> = belts.collect();
        belts.sort_by(|a, b| a.1.total_cmp(&b.1));
        plan.belts = belts
            .iter()
            .find(|(_, cap)| *cap >= need)
            .or(belts.last())
            .map(|(b, cap)| (b.clone(), count(need, *cap)));
    }
    plan
}

/// The recipe that makes `item`: the first open one whose main output it is,
/// else the first open one that makes it at all.
fn recipe_for<'a>(
    content: &'a Content,
    item: &ItemId,
    open: &impl Fn(&RecipeId) -> bool,
) -> Option<&'a RecipeDef> {
    let makes = |r: &&RecipeDef| open(&r.id) && r.outputs.iter().any(|s| &s.item == item);
    let main = |r: &&RecipeDef| r.outputs.first().is_some_and(|s| &s.item == item);
    let mut found = content.recipes.iter().filter(makes);
    let first = found.clone().next();
    found.find(main).or(first)
}

/// The fastest open crafter for `recipe`, and its speed.
fn crafter(
    content: &Content,
    recipe: &RecipeDef,
    open: &impl Fn(&BuildingId) -> bool,
) -> Option<(BuildingId, f32)> {
    content
        .buildings
        .iter()
        .filter_map(|b| match &b.role {
            BuildingRole::Crafter { categories, speed }
                if open(&b.id) && categories.contains(&recipe.category) =>
            {
                Some((b.id.clone(), *speed))
            }
            _ => None,
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
}

/// The fastest open drill for an ore, or well for a fluid, and how much
/// one gives a second. The map holds ores, so they are dug even where a
/// recipe also gives some.
fn ground(
    content: &Content,
    item: &ItemId,
    open: &impl Fn(&BuildingId) -> bool,
) -> Option<(BuildingId, f32)> {
    let ore = content
        .items
        .iter()
        .find(|i| &i.id == item)
        .is_some_and(|i| i.kind == ItemKind::Solid && i.shape == IconShape::Ore);
    content
        .buildings
        .iter()
        .filter(|b| open(&b.id))
        .filter_map(|b| match &b.role {
            BuildingRole::Miner { seconds_per_ore } if ore => {
                Some((b.id.clone(), 1.0 / seconds_per_ore.max(1e-3)))
            }
            BuildingRole::Well { fluid, per_second } if fluid == item => {
                Some((b.id.clone(), *per_second))
            }
            _ => None,
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
}

fn source(
    content: &Content,
    item: &ItemId,
    per_second: f32,
    recipe_open: &impl Fn(&RecipeId) -> bool,
    building_open: &impl Fn(&BuildingId) -> bool,
) -> Source {
    if let Some((building, each)) = ground(content, item, building_open) {
        return Source::Ground {
            building,
            machines: count(per_second, each),
        };
    }
    if let Some(r) = recipe_for(content, item, recipe_open) {
        if let Some((building, speed)) = crafter(content, r, building_open) {
            let out = r
                .outputs
                .iter()
                .find(|s| &s.item == item)
                .map_or(1, |s| s.count);
            let each = out as f32 * speed / r.seconds.max(1e-3);
            return Source::Craft {
                recipe: r.id.clone(),
                building,
                machines: count(per_second, each),
            };
        }
    }
    Source::Bought
}

/// Add `per_second` of `item` and all that goes into it to `steps`.
/// `chain` is the items above it, so a loop ends rather than recursing.
fn walk(
    content: &Content,
    item: &ItemId,
    per_second: f32,
    recipe_open: &impl Fn(&RecipeId) -> bool,
    building_open: &impl Fn(&BuildingId) -> bool,
    chain: &mut Vec<ItemId>,
    steps: &mut Vec<Step>,
) {
    match steps.iter_mut().find(|s| &s.item == item) {
        Some(s) => s.per_second += per_second,
        None => steps.push(Step {
            item: item.clone(),
            per_second,
            source: Source::Bought,
        }),
    }
    if chain.contains(item) || ground(content, item, building_open).is_some() {
        return;
    }
    let Some(r) = recipe_for(content, item, recipe_open) else {
        return;
    };
    if crafter(content, r, building_open).is_none() {
        return;
    }
    let out = r
        .outputs
        .iter()
        .find(|s| &s.item == item)
        .map_or(1, |s| s.count)
        .max(1);
    let crafts = per_second / out as f32;
    chain.push(item.clone());
    for input in &r.inputs {
        walk(
            content,
            &input.item,
            crafts * input.count as f32,
            recipe_open,
            building_open,
            chain,
            steps,
        );
    }
    chain.pop();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content() -> Content {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        mofact_content::load_dir(&dir).expect("the game data loads")
    }

    /// What a new game can build.
    const BASIC: [&str; 4] = ["belt", "drill", "furnace", "assembler"];

    fn id(s: &str) -> ItemId {
        ItemId::new(s)
    }

    #[test]
    fn a_minute_of_circuits_needs_the_whole_chain_down_to_the_drills() {
        let content = content();
        let p = plan(
            &content,
            &id("circuit"),
            60.0,
            |_| true,
            |b| {
                // The starting set.
                BASIC.contains(&b.as_str())
            },
        );
        let step = |i: &str| p.steps.iter().find(|s| s.item == id(i)).unwrap();
        assert_eq!(p.steps[0].item, id("circuit"));
        assert!((step("circuit").per_second - 1.0).abs() < 1e-4);
        // Each circuit takes plates through its gear and wire, all the way down.
        let ore = step("iron-ore");
        assert!(ore.per_second > step("circuit").per_second);
        assert!(matches!(&ore.source, Source::Ground { building, machines }
            if building.as_str() == "drill" && *machines >= 1));
        assert!(
            matches!(&step("circuit").source, Source::Craft { building, .. }
            if building.as_str() == "assembler")
        );
        // One a second fits on a plain belt.
        assert_eq!(
            p.belts.as_ref().map(|(b, n)| (b.as_str(), *n)),
            Some(("belt", 1))
        );
        assert!(p.machines().iter().all(|(_, n)| *n >= 1));
    }

    #[test]
    fn more_wanted_needs_more_machines_and_better_ones_need_fewer() {
        let content = content();
        let total = |rate: f32, open: &dyn Fn(&BuildingId) -> bool| -> u32 {
            plan(&content, &id("gear"), rate, |_| true, open)
                .machines()
                .iter()
                .map(|(_, n)| n)
                .sum()
        };
        let basic = |b: &BuildingId| BASIC.contains(&b.as_str());
        assert!(total(600.0, &basic) > total(60.0, &basic));
        assert!(total(600.0, &|_| true) < total(600.0, &basic));
        // A lot more than a belt carries takes several.
        let p = plan(&content, &id("gear"), 1200.0, |_| true, basic);
        assert!(p.belts.unwrap().1 > 1);
    }

    #[test]
    fn a_locked_recipe_is_not_planned_on() {
        let content = content();
        let p = plan(
            &content,
            &id("circuit"),
            60.0,
            |r| !r.as_str().contains("circuit"),
            |_| true,
        );
        assert_eq!(p.steps.len(), 1, "{p:?}");
        assert_eq!(p.steps[0].source, Source::Bought);
    }
}
