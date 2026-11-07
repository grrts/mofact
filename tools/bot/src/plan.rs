//! Choosing how to make each item from what research has opened, and
//! turning that choice into a block.
//!
//! Every way of making an item is scored by how many machines its tree
//! takes, counting each input's own tree and whatever uses up a fluid
//! by-product; the cheapest open way wins. A recipe fits a machine when its
//! inputs and by-products fit the three sides that are not the product's.

use crate::layout::{self, Block, Craft, Feed};
use mofact_core::content::{BuildingDef, BuildingRole, Content, IconShape, ItemKind, RecipeDef};
use mofact_core::geom::Dir;
use mofact_core::ids::{BuildingId, ItemId, RecipeId, ResearchId};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, PartialEq)]
pub enum Way {
    /// Dug by a drill.
    Ore,
    /// Drawn by a well.
    Well(BuildingId),
    /// Output `k` of `recipe`, in `building`.
    Craft {
        recipe: usize,
        k: usize,
        building: BuildingId,
    },
}

pub struct Planner<'a> {
    pub content: &'a Content,
    fluid: HashSet<ItemId>,
    recipe_lock: HashMap<RecipeId, ResearchId>,
    building_lock: HashMap<BuildingId, ResearchId>,
    done: HashSet<ResearchId>,
    ways: HashMap<ItemId, (u32, Way)>,
    /// How to use up a fluid by-product: the recipe, and its tree's size.
    sinks: HashMap<ItemId, (u32, usize)>,
}

impl<'a> Planner<'a> {
    pub fn new(content: &'a Content) -> Self {
        let mut p = Planner {
            content,
            fluid: content
                .items
                .iter()
                .filter(|i| i.kind == ItemKind::Fluid)
                .map(|i| i.id.clone())
                .collect(),
            recipe_lock: HashMap::new(),
            building_lock: HashMap::new(),
            done: HashSet::new(),
            ways: HashMap::new(),
            sinks: HashMap::new(),
        };
        for r in &content.research {
            for x in &r.recipes {
                p.recipe_lock.insert(x.clone(), r.id.clone());
            }
            for x in &r.buildings {
                p.building_lock.insert(x.clone(), r.id.clone());
            }
        }
        p.update(HashSet::new());
        p
    }

    pub fn is_fluid(&self, item: &ItemId) -> bool {
        self.fluid.contains(item)
    }

    pub fn building_open(&self, id: &BuildingId) -> bool {
        self.building_lock
            .get(id)
            .is_none_or(|r| self.done.contains(r))
    }

    fn recipe_open(&self, id: &RecipeId) -> bool {
        self.recipe_lock
            .get(id)
            .is_none_or(|r| self.done.contains(r))
    }

    pub fn building(&self, id: &str) -> &'a BuildingDef {
        self.content
            .buildings
            .iter()
            .find(|b| b.id.as_str() == id)
            .unwrap_or_else(|| panic!("no building {id}"))
    }

    /// The open machine for a recipe category: unpowered first, then the
    /// first listed.
    fn machine(&self, category: &str) -> Option<&'a BuildingDef> {
        let mut fit: Vec<_> = self
            .content
            .buildings
            .iter()
            .filter(|b| match &b.role {
                BuildingRole::Crafter { categories, .. } => {
                    categories.iter().any(|c| c == category) && self.building_open(&b.id)
                }
                _ => false,
            })
            .collect();
        fit.sort_by_key(|b| b.power > 0.0);
        fit.first().copied()
    }

    /// Work the choices out again with `done` research finished.
    pub fn update(&mut self, done: HashSet<ResearchId>) {
        self.done = done;
        self.ways.clear();
        self.sinks.clear();
        for item in &self.content.items {
            if item.kind == ItemKind::Solid && item.shape == IconShape::Ore {
                self.ways.insert(item.id.clone(), (1, Way::Ore));
            }
        }
        for b in &self.content.buildings {
            if let BuildingRole::Well { fluid, .. } = &b.role {
                if self.building_open(&b.id) {
                    self.ways
                        .insert(fluid.clone(), (1, Way::Well(b.id.clone())));
                }
            }
        }
        // Relax until nothing gets cheaper: every step is a real tree, so
        // the sizes only fall and the choices never loop.
        for _ in 0..64 {
            let mut changed = false;
            let content = self.content;
            for (ri, r) in content.recipes.iter().enumerate() {
                let Some(m) = self.recipe_fit(r) else {
                    continue;
                };
                // Using up a fluid: every output solid, at most two other inputs.
                let fluids: Vec<_> = r.inputs.iter().filter(|s| self.is_fluid(&s.item)).collect();
                for f in fluids {
                    if r.outputs.iter().any(|o| self.is_fluid(&o.item)) || r.inputs.len() > 3 {
                        continue;
                    }
                    let rest: Option<u32> = r
                        .inputs
                        .iter()
                        .filter(|s| s.item != f.item)
                        .map(|s| self.ways.get(&s.item).map(|w| w.0))
                        .sum();
                    if let Some(n) = rest {
                        let n = n + 1;
                        if self.sinks.get(&f.item).is_none_or(|s| s.0 > n) {
                            self.sinks.insert(f.item.clone(), (n, ri));
                            changed = true;
                        }
                    }
                }
                if r.inputs.len() + r.outputs.len() - 1 > 3 || r.outputs.len() > 3 {
                    continue;
                }
                let inputs: Option<u32> = r
                    .inputs
                    .iter()
                    .map(|s| self.ways.get(&s.item).map(|w| w.0))
                    .sum();
                let Some(inputs) = inputs else { continue };
                for (k, want) in r.outputs.iter().enumerate() {
                    let sinks: Option<u32> = r
                        .outputs
                        .iter()
                        .enumerate()
                        .filter(|&(j, _)| j != k)
                        .map(|(_, o)| {
                            if self.is_fluid(&o.item) {
                                self.sinks.get(&o.item).map(|s| s.0)
                            } else {
                                Some(0)
                            }
                        })
                        .sum();
                    let Some(sinks) = sinks else { continue };
                    let n = 1 + inputs + sinks;
                    let better = self.ways.get(&want.item).is_none_or(|w| w.0 > n);
                    if better
                        && !matches!(
                            self.ways.get(&want.item),
                            Some((_, Way::Ore | Way::Well(_)))
                        )
                    {
                        self.ways.insert(
                            want.item.clone(),
                            (
                                n,
                                Way::Craft {
                                    recipe: ri,
                                    k,
                                    building: m.id.clone(),
                                },
                            ),
                        );
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
    }

    fn recipe_fit(&self, r: &RecipeDef) -> Option<&'a BuildingDef> {
        if !self.recipe_open(&r.id) {
            return None;
        }
        let m = self.machine(&r.category)?;
        // Two-tile machines only where nothing smaller crafts it.
        (m.size.0 <= 2).then_some(m)
    }

    /// The recipe that uses up a fluid by-product.
    pub fn sink(&self, fluid: &ItemId) -> Option<usize> {
        self.sinks.get(fluid).map(|s| s.1)
    }

    pub fn way(&self, item: &ItemId) -> Option<&(u32, Way)> {
        self.ways.get(item)
    }

    /// Whether `item` can be made now.
    pub fn can_make(&self, item: &ItemId) -> bool {
        self.ways.contains_key(item)
    }

    /// The tree that makes `item`, putting it out of its east edge.
    pub fn block(&self, item: &ItemId) -> Result<Block, String> {
        let (_, way) = self
            .ways
            .get(item)
            .ok_or_else(|| format!("no open way to make {item}"))?;
        Ok(match way {
            Way::Ore => {
                let drill = self.building("drill");
                layout::drill(drill.id.as_str(), item, drill.power)
            }
            Way::Well(b) => layout::well(b.as_str(), self.building(b.as_str()).power),
            Way::Craft {
                recipe,
                k,
                building,
            } => {
                self.craft(&self.content.recipes[*recipe], *k, building, None)?
                    .0
            }
        })
    }

    /// Recipe `r` in `building`, putting out output `k`. `external` keeps a
    /// side free for a fluid the caller pipes in.
    fn craft(
        &self,
        r: &RecipeDef,
        k: usize,
        building: &BuildingId,
        external: Option<(Dir, &ItemId)>,
    ) -> Result<(Block, mofact_core::geom::TilePos), String> {
        let b = self.building(building.as_str());
        // Output k leaves by front, right, left: face so it goes east.
        let dir = [Dir::East, Dir::North, Dir::South][k];
        let side = |j: usize| [dir, dir.rotate_cw(), dir.opposite().rotate_cw()][j];
        let mut feeds: HashMap<Dir, Feed> = HashMap::new();
        for (j, o) in r.outputs.iter().enumerate() {
            if j == k {
                continue;
            }
            let feed = if self.is_fluid(&o.item) {
                let (_, sink) = self
                    .sinks
                    .get(&o.item)
                    .ok_or_else(|| format!("nothing uses up {}", o.item))?;
                let s = &self.content.recipes[*sink];
                let m = self.recipe_fit(s).ok_or("sink machine")?;
                // The sink takes the fluid on its side facing this machine.
                let face = side(j).opposite();
                let (blk, at) = self.craft(s, 0, &m.id, Some((face, &o.item)))?;
                Feed::Sink(layout::into_hub(blk), at)
            } else {
                Feed::Hub
            };
            feeds.insert(side(j), feed);
        }
        let mut free: Vec<Dir> = [Dir::West, Dir::North, Dir::South]
            .into_iter()
            .filter(|d| !feeds.contains_key(d))
            .collect();
        if let Some((d, _)) = external {
            free.retain(|&x| x != d);
            feeds.insert(d, Feed::External);
        }
        let inputs: Vec<_> = r
            .inputs
            .iter()
            .filter(|s| external.is_none_or(|(_, f)| &s.item != f))
            .collect();
        if inputs.len() > free.len() {
            return Err(format!("{} has more inputs than sides", r.id));
        }
        for (s, d) in inputs.iter().zip(free) {
            feeds.insert(d, Feed::Child(self.block(&s.item)?, self.is_fluid(&s.item)));
        }
        let want = &r.outputs[k].item;
        Ok(Craft {
            building: b.id.clone(),
            size: b.size,
            dir,
            recipe: r.id.clone(),
            kw: b.power,
            fluid: self.is_fluid(want),
            west: feeds.remove(&Dir::West),
            north: feeds.remove(&Dir::North),
            south: feeds.remove(&Dir::South),
        }
        .layout())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn content() -> Content {
        mofact_content::load_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")).unwrap()
    }

    #[test]
    fn with_everything_researched_every_item_has_a_way() {
        let c = content();
        let mut p = Planner::new(&c);
        assert!(p.can_make(&"iron-plate".into()));
        assert!(!p.can_make(&"steel".into()), "steel needs research");
        p.update(c.research.iter().map(|r| r.id.clone()).collect());
        for item in &c.items {
            assert!(p.can_make(&item.id), "{}", item.id);
            let b = p
                .block(&item.id)
                .unwrap_or_else(|e| panic!("{}: {e}", item.id));
            assert_eq!(b.fault(), None, "{}\n{}", item.id, b.picture());
            let (x0, y0, x1, y1) = b.bounds();
            println!(
                "{:<22} {:>4} parts, {} x {}",
                item.id.as_str(),
                b.parts.len(),
                x1 - x0 + 1,
                y1 - y0 + 1
            );
        }
    }
}
