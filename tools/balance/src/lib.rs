//! Content validator and balance report (T25): reachability, ratios, chain
//! depth and by-product sinks for the content in `data/` (R1, R3, R6).
//!
//! The loader already checks that every id it meets exists. This goes
//! further, over the recipe graph as a whole:
//!
//! - **Reachability:** starting from what miners dig (ore-shaped items no
//!   recipe makes), every item must be makeable by recipes some building
//!   crafts, and every building's cost must be makeable. An item nothing can
//!   make is a problem: CI fails on it.
//! - **Chain depth:** how many crafting steps each item is from the ground.
//! - **By-product sinks:** every output after a recipe's first is a
//!   by-product, and some recipe must consume it, or it would back up and
//!   stop the machine (R5, R6).
//! - **Ratios:** for one of each end product a second, how many machines each
//!   recipe needs and how much ore a second goes in.
//! - **Research order:** starting with what no research locks, each research
//!   must become payable with the recipes and buildings open before it, or
//!   the player could never do it.

use mofact_core::content::{BuildingRole, Content, IconShape, RecipeDef};
use mofact_core::ids::{ItemId, RecipeId, ResearchId};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write;

/// What the report found.
#[derive(Debug, Default)]
pub struct Report {
    /// Broken content: CI fails on these.
    pub problems: Vec<String>,
    /// Crafting steps from the ground, per reachable item.
    pub depth: BTreeMap<ItemId, u32>,
    /// Items made but used by no recipe or building: the factory's end products.
    pub end_products: Vec<ItemId>,
    /// What the ratios are for: end products and research packs.
    pub targets: Vec<ItemId>,
    /// For one of each target a second: machines per recipe.
    pub machines: BTreeMap<RecipeId, f32>,
    /// For one of each end product a second: raw items per second.
    pub raw_per_second: BTreeMap<ItemId, f32>,
    /// Research in an order the player can do it.
    pub research_order: Vec<ResearchId>,
}

/// The crafting speed of the fastest building that makes each category.
fn speeds(content: &Content) -> HashMap<&str, f32> {
    let mut speeds: HashMap<&str, f32> = HashMap::new();
    for b in &content.buildings {
        if let BuildingRole::Crafter { categories, speed } = &b.role {
            for c in categories {
                let s = speeds.entry(c.as_str()).or_insert(0.0);
                *s = s.max(*speed);
            }
        }
    }
    speeds
}

pub fn analyse(content: &Content) -> Report {
    let mut report = Report::default();
    let speeds = speeds(content);
    let craftable: Vec<&RecipeDef> = content
        .recipes
        .iter()
        .filter(|r| speeds.contains_key(r.category.as_str()))
        .collect();
    let made: HashSet<&ItemId> = craftable
        .iter()
        .flat_map(|r| r.outputs.iter().map(|s| &s.item))
        .collect();
    let has_miner = content
        .buildings
        .iter()
        .any(|b| matches!(b.role, BuildingRole::Miner { .. }));

    // Raw resources: ores that no recipe makes, dug by miners.
    for i in &content.items {
        if i.shape == IconShape::Ore && !made.contains(&i.id) && has_miner {
            report.depth.insert(i.id.clone(), 0);
        }
    }

    // Fluids that wells draw from the ground are raw too.
    for b in &content.buildings {
        if let BuildingRole::Well { fluid, .. } = &b.role {
            report.depth.insert(fluid.clone(), 0);
        }
    }

    // Reachability and depth, by relaxing until nothing changes.
    let mut best: HashMap<&ItemId, &RecipeDef> = HashMap::new();
    loop {
        let mut changed = false;
        for r in &craftable {
            let Some(d) = r
                .inputs
                .iter()
                .map(|s| report.depth.get(&s.item).copied())
                .try_fold(0, |acc, d| d.map(|d| acc.max(d)))
            else {
                continue;
            };
            for (k, out) in r.outputs.iter().enumerate() {
                let depth = d + 1;
                if report.depth.get(&out.item).is_none_or(|&old| depth < old) {
                    report.depth.insert(out.item.clone(), depth);
                    if k == 0 {
                        best.insert(&out.item, r);
                    }
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }

    for i in &content.items {
        if !report.depth.contains_key(&i.id) {
            let why = if made.contains(&i.id) {
                "its recipes need items that can't be made"
            } else if i.shape == IconShape::Ore {
                "there is no miner to dig it"
            } else {
                "no recipe that a building crafts makes it"
            };
            report
                .problems
                .push(format!("item `{}` can't be made: {why}", i.id));
        }
    }
    for b in &content.buildings {
        for s in &b.cost {
            if !report.depth.contains_key(&s.item) {
                report.problems.push(format!(
                    "building `{}` costs `{}`, which can't be made",
                    b.id, s.item
                ));
            }
        }
    }
    for r in &content.recipes {
        if !speeds.contains_key(r.category.as_str()) {
            report.problems.push(format!(
                "recipe `{}` has category `{}`, which no building crafts",
                r.id, r.category
            ));
        }
    }

    // By-products must go somewhere.
    let consumed: HashSet<&ItemId> = content
        .recipes
        .iter()
        .flat_map(|r| r.inputs.iter().map(|s| &s.item))
        .chain(
            content
                .buildings
                .iter()
                .flat_map(|b| b.cost.iter().map(|s| &s.item)),
        )
        .chain(
            content
                .research
                .iter()
                .flat_map(|r| r.cost.iter().map(|s| &s.item)),
        )
        // Engines burn their fluid into power.
        .chain(content.buildings.iter().filter_map(|b| match &b.role {
            BuildingRole::Engine { fluid, .. } => Some(fluid),
            _ => None,
        }))
        .collect();
    for r in &content.recipes {
        for s in r.outputs.iter().skip(1) {
            if !consumed.contains(&s.item) {
                report.problems.push(format!(
                    "recipe `{}` makes by-product `{}`, which nothing uses",
                    r.id, s.item
                ));
            }
        }
    }

    // Ratios: one of each end product a second, back to the ground.
    report.end_products = content
        .items
        .iter()
        .filter(|i| made.contains(&i.id) && !consumed.contains(&i.id))
        .map(|i| i.id.clone())
        .collect();
    // Research packs count as end products here: labs are where they end.
    for r in &content.research {
        for s in &r.cost {
            if !report.targets.contains(&s.item) {
                report.targets.push(s.item.clone());
            }
        }
    }
    for i in &report.end_products {
        if !report.targets.contains(i) {
            report.targets.push(i.clone());
        }
    }
    let mut want: Vec<(ItemId, f32)> = report.targets.iter().map(|i| (i.clone(), 1.0)).collect();
    let mut guard = 0;
    while let Some((item, rate)) = want.pop() {
        guard += 1;
        if guard > 10_000 {
            report
                .problems
                .push("the recipe graph loops without end".into());
            break;
        }
        let Some(r) = best.get(&item) else {
            if report.depth.get(&item) == Some(&0) {
                *report.raw_per_second.entry(item).or_insert(0.0) += rate;
            }
            continue;
        };
        let made_per_craft = r.outputs[0].count as f32;
        let crafts = rate / made_per_craft;
        let speed = speeds[r.category.as_str()];
        *report.machines.entry(r.id.clone()).or_insert(0.0) += crafts * r.seconds / speed;
        for s in &r.inputs {
            want.push((s.item.clone(), crafts * s.count as f32));
        }
    }
    research_order(content, &mut report);
    report
}

/// The items makeable with only the `open` recipes and buildings.
fn makeable<'a>(
    content: &'a Content,
    recipes: &HashSet<&str>,
    buildings: &HashSet<&str>,
) -> HashSet<&'a ItemId> {
    let mut categories = HashSet::new();
    let mut miner = false;
    for b in content
        .buildings
        .iter()
        .filter(|b| buildings.contains(b.id.as_str()))
    {
        match &b.role {
            BuildingRole::Crafter { categories: c, .. } => {
                categories.extend(c.iter().map(String::as_str))
            }
            BuildingRole::Miner { .. } => miner = true,
            _ => {}
        }
    }
    let made: HashSet<&ItemId> = content
        .recipes
        .iter()
        .flat_map(|r| r.outputs.iter().map(|s| &s.item))
        .collect();
    let mut have: HashSet<&ItemId> = content
        .items
        .iter()
        .filter(|i| miner && i.shape == IconShape::Ore && !made.contains(&i.id))
        .map(|i| &i.id)
        .collect();
    loop {
        let before = have.len();
        for r in &content.recipes {
            if recipes.contains(r.id.as_str())
                && categories.contains(r.category.as_str())
                && r.inputs.iter().all(|s| have.contains(&s.item))
            {
                have.extend(r.outputs.iter().map(|s| &s.item));
            }
        }
        if have.len() == before {
            return have;
        }
    }
}

fn research_order(content: &Content, report: &mut Report) {
    let locked_recipes: HashSet<&str> = content
        .research
        .iter()
        .flat_map(|r| r.recipes.iter().map(|x| x.as_str()))
        .collect();
    let locked_buildings: HashSet<&str> = content
        .research
        .iter()
        .flat_map(|r| r.buildings.iter().map(|x| x.as_str()))
        .collect();
    let mut recipes: HashSet<&str> = content
        .recipes
        .iter()
        .map(|r| r.id.as_str())
        .filter(|r| !locked_recipes.contains(r))
        .collect();
    let mut buildings: HashSet<&str> = content
        .buildings
        .iter()
        .map(|b| b.id.as_str())
        .filter(|b| !locked_buildings.contains(b))
        .collect();
    let mut done: HashSet<&str> = HashSet::new();
    loop {
        let have = makeable(content, &recipes, &buildings);
        // Research happens in labs, so one must be open to build.
        let lab = content.buildings.iter().any(|b| {
            buildings.contains(b.id.as_str()) && matches!(b.role, BuildingRole::Lab { .. })
        });
        let next = content.research.iter().find(|r| {
            lab && !done.contains(r.id.as_str())
                && r.requires.iter().all(|q| done.contains(q.as_str()))
                && r.cost.iter().all(|s| have.contains(&s.item))
        });
        let Some(r) = next else { break };
        done.insert(r.id.as_str());
        report.research_order.push(r.id.clone());
        recipes.extend(r.recipes.iter().map(|x| x.as_str()));
        buildings.extend(r.buildings.iter().map(|x| x.as_str()));
    }
    for r in &content.research {
        if !done.contains(r.id.as_str()) {
            report.problems.push(format!(
                "research `{}` can never be done: its prerequisites, packs or a lab stay out of reach",
                r.id
            ));
        }
    }
    // Packs must not need what their own research unlocks: checked above, as
    // such a research never becomes payable.
}

impl Report {
    /// The report as text for people.
    pub fn text(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "Chain depth (crafting steps from the ground):");
        let mut by_depth: Vec<_> = self.depth.iter().collect();
        by_depth.sort_by_key(|(id, d)| (**d, id.as_str()));
        for (id, d) in by_depth {
            let _ = writeln!(out, "  {d}  {id}");
        }
        let ends: Vec<&str> = self.targets.iter().map(|i| i.as_str()).collect();
        let _ = writeln!(
            out,
            "\nFor 1/s of each end product and research pack ({}):",
            ends.join(", ")
        );
        for (r, n) in &self.machines {
            let _ = writeln!(out, "  {n:6.2} machines on {r}");
        }
        for (i, n) in &self.raw_per_second {
            let _ = writeln!(out, "  {n:6.2}/s of {i} from the ground");
        }
        if !self.research_order.is_empty() {
            let order: Vec<&str> = self.research_order.iter().map(|r| r.as_str()).collect();
            let _ = writeln!(
                out,
                "\nResearch, in an order that works: {}",
                order.join(" -> ")
            );
        }
        if self.problems.is_empty() {
            let _ = writeln!(out, "\nNo problems.");
        } else {
            let _ = writeln!(out, "\n{} problem(s):", self.problems.len());
            for p in &self.problems {
                let _ = writeln!(out, "  {p}");
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ITEMS: &str = r#"[
        (id: "iron-ore", name: "Iron ore", kind: Solid, shape: Ore, color: "iron-ore", tier: 1),
        (id: "iron-plate", name: "Iron plate", kind: Solid, shape: Plate, color: "iron", tier: 1),
        (id: "gear", name: "Gear", kind: Solid, shape: Gear, color: "iron", tier: 1),
    ]"#;
    const RECIPES: &str = r#"[
        (id: "iron-plate", name: "Iron plate", category: "smelting", seconds: 1.2,
            inputs: [(item: "iron-ore", count: 1)], outputs: [(item: "iron-plate", count: 1)]),
        (id: "gear", name: "Gear", category: "assembling", seconds: 1.0,
            inputs: [(item: "iron-plate", count: 2)], outputs: [(item: "gear", count: 1)]),
    ]"#;
    const BUILDINGS: &str = r#"[
        (id: "drill", name: "Drill", size: (1, 1), role: Miner(seconds_per_ore: 1.2), cost: []),
        (id: "furnace", name: "Furnace", size: (1, 1), role: Crafter(categories: ["smelting"], speed: 1.0), cost: []),
        (id: "assembler", name: "Assembler", size: (1, 1), role: Crafter(categories: ["assembling"], speed: 1.0),
            cost: [(item: "iron-plate", count: 8)]),
    ]"#;

    fn content(items: &str, recipes: &str, buildings: &str) -> Content {
        mofact_content::load_strs(items, recipes, buildings, "[]", "[]").expect("loads")
    }

    #[test]
    fn the_game_data_is_clean() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let report = analyse(&mofact_content::load_dir(&dir).expect("loads"));
        assert!(report.problems.is_empty(), "{}", report.text());
        assert_eq!(report.depth[&ItemId::from("circuit")], 3);
        assert_eq!(report.depth[&ItemId::from("engineering-pack")], 4);
        // Bricks go into labs; research takes the packs.
        assert!(report.end_products.is_empty(), "{:?}", report.end_products);
        let order: Vec<&str> = report.research_order.iter().map(|r| r.as_str()).collect();
        assert_eq!(
            order,
            [
                "steel-working",
                "motors",
                "fast-belts",
                "logistics",
                "express-belts",
                "fluid-handling",
                "steam-power"
            ]
        );
    }

    #[test]
    fn research_paid_with_what_it_unlocks_is_a_problem() {
        let items = format!(
            "{}{}",
            ITEMS.trim_end().trim_end_matches(']'),
            r#"(id: "pack", name: "Pack", kind: Solid, shape: Pack, color: "x", tier: 1), ]"#
        );
        let recipes = format!(
            "{}{}",
            RECIPES.trim_end().trim_end_matches(']'),
            r#"(id: "pack", name: "Pack", category: "assembling", seconds: 1.0,
                inputs: [(item: "gear", count: 1)], outputs: [(item: "pack", count: 1)]), ]"#
        );
        // Packs need gears, and gears are what this research unlocks.
        let research = r#"[(id: "gears", name: "Gears", tier: 1,
            cost: [(item: "pack", count: 1)], units: 5, seconds: 1.0, recipes: ["gear"])]"#;
        let content =
            mofact_content::load_strs(&items, &recipes, BUILDINGS, research, "[]").expect("loads");
        let report = analyse(&content);
        assert!(
            report
                .problems
                .iter()
                .any(|p| p.contains("research `gears` can never be done")),
            "{}",
            report.text()
        );
    }

    #[test]
    fn ratios_and_depth_follow_the_recipes() {
        let report = analyse(&content(ITEMS, RECIPES, BUILDINGS));
        assert!(report.problems.is_empty(), "{}", report.text());
        assert_eq!(report.depth[&ItemId::from("gear")], 2);
        // 1 gear/s takes one assembler, 2 plates/s, so 2.4 furnaces and 2 ore/s.
        let m = |r: &str| report.machines[&RecipeId::from(r)];
        assert!((m("gear") - 1.0).abs() < 1e-5);
        assert!((m("iron-plate") - 2.4).abs() < 1e-5);
        assert!((report.raw_per_second[&ItemId::from("iron-ore")] - 2.0).abs() < 1e-5);
    }

    #[test]
    fn an_unreachable_item_is_a_problem() {
        // Gears now need a "spring" that nothing makes.
        let items = ITEMS.replace(
            "]",
            r#"(id: "spring", name: "Spring", kind: Solid, shape: Wire, color: "iron", tier: 1), ]"#,
        );
        let recipes = RECIPES.replace(
            r#"inputs: [(item: "iron-plate", count: 2)]"#,
            r#"inputs: [(item: "iron-plate", count: 2), (item: "spring", count: 1)]"#,
        );
        let report = analyse(&content(&items, &recipes, BUILDINGS));
        let problems = report.problems.join("\n");
        assert!(
            problems.contains("item `spring` can't be made"),
            "{problems}"
        );
        assert!(problems.contains("item `gear` can't be made"), "{problems}");
    }

    #[test]
    fn a_building_that_costs_something_unmakeable_is_a_problem() {
        let buildings = BUILDINGS.replace(
            r#"cost: [(item: "iron-plate", count: 8)]"#,
            r#"cost: [(item: "spring", count: 8)]"#,
        );
        let items = ITEMS.replace(
            "]",
            r#"(id: "spring", name: "Spring", kind: Solid, shape: Wire, color: "iron", tier: 1), ]"#,
        );
        let report = analyse(&content(&items, RECIPES, &buildings));
        assert!(report
            .problems
            .iter()
            .any(|p| p.contains("building `assembler` costs `spring`")));
    }

    #[test]
    fn a_by_product_nothing_uses_is_a_problem() {
        let items = ITEMS.replace(
            "]",
            r#"(id: "slag", name: "Slag", kind: Solid, shape: Ore, color: "iron-ore", tier: 1), ]"#,
        );
        let recipes = RECIPES.replace(
            r#"outputs: [(item: "iron-plate", count: 1)]"#,
            r#"outputs: [(item: "iron-plate", count: 1), (item: "slag", count: 1)]"#,
        );
        let report = analyse(&content(&items, &recipes, BUILDINGS));
        assert!(report
            .problems
            .iter()
            .any(|p| p.contains("by-product `slag`, which nothing uses")));
    }
}
