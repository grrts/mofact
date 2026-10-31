//! Tile coordinates and directions on the infinite grid.

use crate::CHUNK_SIZE;
use serde::{Deserialize, Serialize};

/// One of the four directions a building or belt can face.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Dir {
    #[default]
    East,
    South,
    West,
    North,
}

impl Dir {
    pub const ALL: [Dir; 4] = [Dir::East, Dir::South, Dir::West, Dir::North];

    /// Tile offset of one step in this direction (y grows southwards, as on screen).
    pub const fn offset(self) -> (i32, i32) {
        match self {
            Dir::East => (1, 0),
            Dir::South => (0, 1),
            Dir::West => (-1, 0),
            Dir::North => (0, -1),
        }
    }

    /// The next direction clockwise, as the rotate tool steps.
    pub const fn rotate_cw(self) -> Dir {
        match self {
            Dir::East => Dir::South,
            Dir::South => Dir::West,
            Dir::West => Dir::North,
            Dir::North => Dir::East,
        }
    }

    pub const fn opposite(self) -> Dir {
        self.rotate_cw().rotate_cw()
    }
}

/// A tile position in world coordinates.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct TilePos {
    pub x: i32,
    pub y: i32,
}

impl TilePos {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    /// The neighbouring tile in `dir`.
    pub const fn step(self, dir: Dir) -> Self {
        let (dx, dy) = dir.offset();
        Self::new(self.x + dx, self.y + dy)
    }

    /// The chunk that contains this tile. Rounds towards negative infinity.
    pub const fn chunk(self) -> ChunkPos {
        ChunkPos {
            x: self.x.div_euclid(CHUNK_SIZE),
            y: self.y.div_euclid(CHUNK_SIZE),
        }
    }
}

/// A chunk position: chunk (0, 0) holds tiles 0..64 on both axes.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct ChunkPos {
    pub x: i32,
    pub y: i32,
}

impl ChunkPos {
    /// The top-left tile of this chunk.
    pub const fn origin(self) -> TilePos {
        TilePos::new(self.x * CHUNK_SIZE, self.y * CHUNK_SIZE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotating_four_times_is_identity() {
        for d in Dir::ALL {
            assert_eq!(d.rotate_cw().rotate_cw().rotate_cw().rotate_cw(), d);
            assert_eq!(d.opposite().opposite(), d);
        }
    }

    #[test]
    fn step_and_back() {
        let p = TilePos::new(3, -7);
        for d in Dir::ALL {
            assert_eq!(p.step(d).step(d.opposite()), p);
        }
    }

    #[test]
    fn chunks_round_towards_negative_infinity() {
        assert_eq!(TilePos::new(0, 63).chunk(), ChunkPos { x: 0, y: 0 });
        assert_eq!(TilePos::new(64, 0).chunk(), ChunkPos { x: 1, y: 0 });
        assert_eq!(TilePos::new(-1, -64).chunk(), ChunkPos { x: -1, y: -1 });
        assert_eq!(TilePos::new(-65, 0).chunk(), ChunkPos { x: -2, y: 0 });
        assert_eq!(ChunkPos { x: -1, y: 2 }.origin(), TilePos::new(-64, 128));
    }
}
