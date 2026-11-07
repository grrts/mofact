//! Laying out production as blocks of buildings.
//!
//! A block is a tree of machines that makes one product and puts it out of
//! its east edge, one tile east of its east-most column. A machine faces so
//! that the output wanted goes east; each input comes from a child block
//! stacked to its west, on its west, north or south side; a by-product goes
//! out of the north or south side into a hub of its own, or, for a fluid,
//! into a small block that uses it up.
//!
//! Children are right-aligned against one routing column, and stacked with
//! a gap of empty rows between them, so no two routes cross and no two pipes
//! touch: a pipe joins every pipe beside it.
//!
//! ```text
//!   [north child] ──┐
//!                   ▼
//!   [west child]  ─▶ M ─▶ out
//!                   ▲
//!   [south child] ──┘
//! ```

use mofact_core::geom::{Dir, TilePos};
use mofact_core::ids::{BuildingId, ItemId, RecipeId};

/// Empty rows between stacked blocks.
pub const GAP: i32 = 2;

#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub building: BuildingId,
    pub pos: TilePos,
    pub size: (u8, u8),
    pub dir: Dir,
    pub recipe: Option<RecipeId>,
    /// Ore to put under it first: a drill's deposit.
    pub ore: Option<ItemId>,
    /// Which route a belt or pipe is part of; 0 for everything else.
    pub route: u32,
}

impl Part {
    pub fn new(building: &str, size: (u8, u8), x: i32, y: i32, dir: Dir) -> Self {
        Part {
            building: building.into(),
            pos: TilePos::new(x, y),
            size,
            dir,
            recipe: None,
            ore: None,
            route: 0,
        }
    }

    pub fn tiles(&self) -> impl Iterator<Item = TilePos> + '_ {
        let (w, h) = (i32::from(self.size.0), i32::from(self.size.1));
        (0..h).flat_map(move |dy| {
            (0..w).map(move |dx| TilePos::new(self.pos.x + dx, self.pos.y + dy))
        })
    }
}

/// A machine that draws power: its top-left tile, size and kW.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Powered {
    pub pos: TilePos,
    pub size: (u8, u8),
    pub kw: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Block {
    pub parts: Vec<Part>,
    pub powered: Vec<Powered>,
    /// The tile just east of the block that its product goes into.
    pub out: TilePos,
}

impl Block {
    pub fn shift(&mut self, dx: i32, dy: i32) {
        for p in &mut self.parts {
            p.pos = TilePos::new(p.pos.x + dx, p.pos.y + dy);
        }
        for p in &mut self.powered {
            p.pos = TilePos::new(p.pos.x + dx, p.pos.y + dy);
        }
        self.out = TilePos::new(self.out.x + dx, self.out.y + dy);
    }

    /// The tiles it covers, as (min x, min y, max x, max y), inclusive.
    pub fn bounds(&self) -> (i32, i32, i32, i32) {
        let mut b = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for t in self.parts.iter().flat_map(Part::tiles) {
            b = (b.0.min(t.x), b.1.min(t.y), b.2.max(t.x), b.3.max(t.y));
        }
        b
    }

    /// Two parts on one tile, or two pipes of different routes side by
    /// side, which would join them: a fault in the layout.
    pub fn fault(&self) -> Option<String> {
        let mut at = std::collections::HashMap::new();
        for (i, p) in self.parts.iter().enumerate() {
            for t in p.tiles() {
                if let Some(j) = at.insert(t, i) {
                    return Some(format!("{:?} and {:?} both on {t:?}", self.parts[j], p));
                }
            }
        }
        let pipe = |p: &Part| p.building.as_str() == "pipe";
        for p in self.parts.iter().filter(|p| pipe(p)) {
            for d in Dir::ALL {
                let Some(&j) = at.get(&p.pos.step(d)) else {
                    continue;
                };
                let q = &self.parts[j];
                if pipe(q) && q.route != p.route {
                    return Some(format!("pipes of two routes touch at {:?}", p.pos));
                }
            }
        }
        None
    }

    /// A map of it, one character a tile, for reading a layout.
    pub fn picture(&self) -> String {
        let (x0, y0, x1, y1) = self.bounds();
        let mut rows = vec![vec!['.'; (x1 - x0 + 1) as usize]; (y1 - y0 + 1) as usize];
        for p in &self.parts {
            let c = match p.building.as_str() {
                "belt" => match p.dir {
                    Dir::East => '>',
                    Dir::West => '<',
                    Dir::North => '^',
                    Dir::South => 'v',
                },
                "pipe" => '=',
                "hub" => 'H',
                "drill" => 'D',
                other => other.chars().next().unwrap_or('?').to_ascii_uppercase(),
            };
            for t in p.tiles() {
                rows[(t.y - y0) as usize][(t.x - x0) as usize] = c;
            }
        }
        rows.into_iter()
            .map(|r| r.into_iter().collect::<String>() + "\n")
            .collect()
    }

    /// The route its product leaves by, if that is a pipe: the parent's
    /// pipe on from it joins it, as one route.
    fn out_pipe(&self) -> Option<u32> {
        let from = self.out.step(Dir::West);
        self.parts
            .iter()
            .find(|p| p.pos == from && p.building.as_str() == "pipe")
            .map(|p| p.route)
    }

    fn add(&mut self, other: Block) {
        self.parts.extend(other.parts);
        self.powered.extend(other.powered);
    }

    /// Move it so its east edge is at `x` and its top at `y`.
    fn put(&mut self, east: i32, top: i32) {
        let (_, y0, x1, _) = self.bounds();
        self.shift(east - x1, top - y0);
    }
}

/// A belt or pipe along `tiles`, each facing its direction.
pub fn route(fluid: bool, tiles: &[(TilePos, Dir)]) -> Vec<Part> {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    tiles
        .iter()
        .map(|&(t, d)| {
            let mut p = Part::new(if fluid { "pipe" } else { "belt" }, (1, 1), t.x, t.y, d);
            p.route = id;
            p
        })
        .collect()
}

/// `parts` as part of route `join`, when there is one.
fn joined(mut parts: Vec<Part>, join: Option<u32>) -> Vec<Part> {
    if let Some(id) = join {
        for p in &mut parts {
            p.route = id;
        }
    }
    parts
}

/// What feeds one side of a machine, or takes from it.
#[derive(Clone, Debug)]
pub enum Feed {
    /// A child block, and whether its product is a fluid.
    Child(Block, bool),
    /// A route the caller lays: this side is kept free for it.
    External,
    /// A solid by-product, out into a hub.
    Hub,
    /// A fluid by-product, out into a block that uses it, which takes it
    /// on its side facing this machine. Its machine is at `at` in it.
    Sink(Block, TilePos),
}

/// One machine and what is around it.
pub struct Craft {
    pub building: BuildingId,
    pub size: (u8, u8),
    pub dir: Dir,
    pub recipe: RecipeId,
    pub kw: f32,
    /// Whether the product wanted is a fluid.
    pub fluid: bool,
    pub west: Option<Feed>,
    pub north: Option<Feed>,
    pub south: Option<Feed>,
}

impl Craft {
    /// The block, and where its machine stands in it.
    pub fn layout(self) -> (Block, TilePos) {
        let s = i32::from(self.size.0);
        let mx = 1; // column 0 carries the routes
        let mut b = Block::default();
        let mut y = 0;
        let mut north_row = None;
        if let Some(Feed::Child(mut c, fluid)) = self.north.clone() {
            c.put(-1, y);
            north_row = Some((c.out.y, fluid, c.out_pipe()));
            y = c.bounds().3 + 1 + GAP;
            b.add(c);
        }
        // The west child sets the machine's row.
        let (my, mut below) = match self.west.clone() {
            Some(Feed::Child(mut c, fluid)) => {
                c.put(-1, y);
                let my = c.out.y;
                let bottom = c.bounds().3;
                let r = route(fluid, &[(TilePos::new(0, my), Dir::East)]);
                b.parts.extend(joined(r, c.out_pipe()));
                b.add(c);
                (my, bottom + 1)
            }
            _ => (y, y),
        };
        below = below.max(my + s) + GAP;
        // A north sink sits above everything else in its column.
        match self.north {
            Some(Feed::Hub) => {
                b.parts
                    .push(Part::new("belt", (1, 1), mx, my - 1, Dir::North));
                b.parts
                    .push(Part::new("hub", (3, 3), mx - 1, my - 4, Dir::East));
            }
            Some(Feed::Sink(mut k, at)) => {
                // Above the west child too: it reaches west of the routes.
                let top = b.bounds().1.min(my) - GAP;
                let (_, _, _, y1) = k.bounds();
                k.shift(mx - at.x, top - 1 - y1);
                let at_y = at.y + (top - 1 - y1);
                let pipe: Vec<_> = (at_y + 1..my)
                    .map(|r| (TilePos::new(mx, r), Dir::North))
                    .collect();
                b.parts.extend(route(true, &pipe));
                b.add(k);
            }
            _ => {}
        }
        if let Some((row, fluid, join)) = north_row {
            let mut tiles = vec![(TilePos::new(0, row), Dir::East)];
            tiles.extend((row..my).map(|r| (TilePos::new(mx, r), Dir::South)));
            b.parts.extend(joined(route(fluid, &tiles), join));
        }
        match self.south {
            Some(Feed::Child(mut c, fluid)) => {
                c.put(-1, below);
                let row = c.out.y;
                let mut tiles = vec![(TilePos::new(0, row), Dir::East)];
                tiles.extend(
                    (my + s..=row)
                        .rev()
                        .map(|r| (TilePos::new(mx, r), Dir::North)),
                );
                b.parts.extend(joined(route(fluid, &tiles), c.out_pipe()));
                b.add(c);
            }
            Some(Feed::Hub) => {
                b.parts
                    .push(Part::new("belt", (1, 1), mx, my + s, Dir::South));
                b.parts
                    .push(Part::new("hub", (3, 3), mx - 1, my + s + 1, Dir::East));
            }
            Some(Feed::Sink(mut k, at)) => {
                let (_, y0, _, _) = k.bounds();
                k.shift(mx - at.x, below - y0);
                let at_y = at.y + (below - y0);
                let pipe: Vec<_> = (my + s..at_y)
                    .map(|r| (TilePos::new(mx, r), Dir::South))
                    .collect();
                b.parts.extend(route(true, &pipe));
                b.add(k);
            }
            _ => {}
        }
        let mut m = Part::new(self.building.as_str(), self.size, mx, my, self.dir);
        m.recipe = Some(self.recipe.clone());
        b.parts.push(m);
        if self.kw > 0.0 {
            b.powered.push(Powered {
                pos: TilePos::new(mx, my),
                size: self.size,
                kw: self.kw,
            });
        }
        // Out along the machine's row to the block's east edge.
        let east = b.bounds().2;
        let tiles: Vec<_> = (mx + s..=east)
            .map(|x| (TilePos::new(x, my), Dir::East))
            .collect();
        b.parts.extend(route(self.fluid, &tiles));
        b.out = TilePos::new(east.max(mx + s - 1) + 1, my);
        (b, TilePos::new(mx, my))
    }
}

/// A drill on its ore.
pub fn drill(building: &str, ore: &ItemId, kw: f32) -> Block {
    let mut p = Part::new(building, (1, 1), 0, 0, Dir::East);
    p.ore = Some(ore.clone());
    let powered = (kw > 0.0)
        .then_some(Powered {
            pos: p.pos,
            size: (1, 1),
            kw,
        })
        .into_iter()
        .collect();
    Block {
        parts: vec![p],
        powered,
        out: TilePos::new(1, 0),
    }
}

/// A well, drawing its fluid into the pipe east of it.
pub fn well(building: &str, kw: f32) -> Block {
    let p = Part::new(building, (1, 1), 0, 0, Dir::East);
    Block {
        powered: vec![Powered {
            pos: p.pos,
            size: (1, 1),
            kw,
        }],
        parts: vec![p],
        out: TilePos::new(1, 0),
    }
}

/// The block's product into a hub of its own.
pub fn into_hub(mut b: Block) -> Block {
    let out = b.out;
    b.parts
        .push(Part::new("hub", (3, 3), out.x, out.y - 1, Dir::East));
    b.out = TilePos::new(out.x + 3, out.y);
    b
}

/// `count` buildings of `size` in a row, fed by belts from the blocks in
/// `upper` (into their north sides), `west` (into the first one's west
/// side) and `lower` (into their south sides): labs, or the megaproject.
pub fn fan_in(
    building: &str,
    size: (u8, u8),
    count: usize,
    kw: f32,
    upper: Vec<Block>,
    west: Option<Block>,
    lower: Vec<Block>,
) -> Block {
    let (w, h) = (i32::from(size.0), i32::from(size.1));
    // Six or more columns: the middle one takes no belts, so a pole has
    // room in reach of the middle of the row.
    let span = count as i32 * w;
    let gap = (span >= 6).then_some(1 + span / 2);
    let columns: Vec<i32> = (1..=span).filter(|&c| Some(c) != gap).collect();
    assert!(upper.len() <= columns.len() && lower.len() <= columns.len());
    let mut b = Block::default();
    let mut y = 0;
    // The top child turns down furthest east, so no route crosses another.
    let mut ups = Vec::new();
    for (j, mut c) in upper.into_iter().enumerate() {
        c.put(-1, y);
        y = c.bounds().3 + 1 + GAP;
        ups.push((c.out.y, columns[ups.len().max(j)]));
        b.add(c);
    }
    let n = ups.len();
    for (j, up) in ups.iter_mut().enumerate() {
        up.1 = columns[n - 1 - j];
    }
    let row = match west {
        Some(mut c) => {
            c.put(-1, y);
            let r = c.out.y;
            b.parts
                .extend(route(false, &[(TilePos::new(0, r), Dir::East)]));
            b.add(c);
            r
        }
        None => y,
    };
    let mut below = b.bounds().3.max(row + h - 1) + 1 + GAP;
    if b.parts.is_empty() {
        below = row + h + GAP;
    }
    for (r, col) in ups {
        let mut tiles: Vec<_> = (0..col).map(|x| (TilePos::new(x, r), Dir::East)).collect();
        tiles.extend((r..row).map(|y| (TilePos::new(col, y), Dir::South)));
        b.parts.extend(route(false, &tiles));
    }
    for (j, mut c) in lower.into_iter().enumerate() {
        c.put(-1, below);
        below = c.bounds().3 + 1 + GAP;
        let r = c.out.y;
        let col = columns[j];
        let mut tiles: Vec<_> = (0..col).map(|x| (TilePos::new(x, r), Dir::East)).collect();
        tiles.extend(
            (row + h..=r)
                .rev()
                .map(|y| (TilePos::new(col, y), Dir::North)),
        );
        b.parts.extend(route(false, &tiles));
        b.add(c);
    }
    for i in 0..count as i32 {
        let p = Part::new(building, size, 1 + i * w, row, Dir::East);
        if kw > 0.0 {
            b.powered.push(Powered {
                pos: p.pos,
                size,
                kw,
            });
        }
        b.parts.push(p);
    }
    b.out = TilePos::new(1 + count as i32 * w, row);
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(b: &Block) {
        if let Some(f) = b.fault() {
            panic!("{f}");
        }
    }

    fn leaf() -> Block {
        drill("drill", &"iron-ore".into(), 0.0)
    }

    fn craft(west: Option<Feed>, north: Option<Feed>, south: Option<Feed>) -> Block {
        Craft {
            building: "assembler".into(),
            size: (1, 1),
            dir: Dir::East,
            recipe: "gear".into(),
            kw: 0.0,
            fluid: false,
            west,
            north,
            south,
        }
        .layout()
        .0
    }

    #[test]
    fn a_three_input_machine_takes_one_route_per_side() {
        let c = || Some(Feed::Child(leaf(), false));
        let b = craft(c(), c(), c());
        check(&b);
        // The machine is the east-most part, and puts out just east of it.
        let m = b
            .parts
            .iter()
            .find(|p| p.building.as_str() == "assembler")
            .unwrap();
        assert_eq!(b.out, m.pos.step(Dir::East));
        // Three drills, three routes of belts into it.
        assert_eq!(
            b.parts
                .iter()
                .filter(|p| p.building.as_str() == "drill")
                .count(),
            3
        );
    }

    #[test]
    fn nested_blocks_and_sinks_never_overlap() {
        let c = || Some(Feed::Child(leaf(), false));
        let inner = || Some(Feed::Child(craft(c(), c(), c()), false));
        let (sink, at) = Craft {
            building: "chemical-plant".into(),
            size: (1, 1),
            dir: Dir::East,
            recipe: "plastic".into(),
            kw: 120.0,
            fluid: false,
            west: c(),
            north: Some(Feed::External),
            south: None,
        }
        .layout();
        let b = craft(inner(), inner(), Some(Feed::Sink(into_hub(sink), at)));
        check(&b);
        let b = craft(inner(), Some(Feed::Hub), inner());
        check(&b);
        let big = craft(
            Some(Feed::Child(b.clone(), false)),
            Some(Feed::Child(b.clone(), false)),
            Some(Feed::Child(b, false)),
        );
        check(&big);
        let labs = fan_in(
            "lab",
            (1, 1),
            3,
            60.0,
            vec![big.clone(), leaf(), leaf()],
            Some(leaf()),
            vec![leaf(), big, leaf()],
        );
        check(&labs);
        assert_eq!(labs.powered.len(), 3);
    }

    /// Six labs fed by five belts on each side: every lab keeps a free
    /// tile in a pole's reach.
    #[test]
    fn a_long_row_of_labs_leaves_room_for_poles() {
        let labs = fan_in(
            "lab",
            (1, 1),
            6,
            60.0,
            (0..5).map(|_| leaf()).collect(),
            Some(leaf()),
            (0..5).map(|_| leaf()).collect(),
        );
        check(&labs);
        let taken: std::collections::HashSet<_> =
            labs.parts.iter().flat_map(|p| p.tiles()).collect();
        for m in &labs.powered {
            let room = (m.pos.y - 2..=m.pos.y + 2)
                .any(|y| (m.pos.x - 2..=m.pos.x + 2).any(|x| !taken.contains(&TilePos::new(x, y))));
            assert!(
                room,
                "no room for a pole by {:?}\n{}",
                m.pos,
                labs.picture()
            );
        }
    }
}
