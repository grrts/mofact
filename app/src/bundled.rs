//! The game data and the new-game factory, built into the binary, so a phone
//! build (which has no `data/` folder beside it) and a copied desktop binary
//! both start without files. `--data DIR` and `--scenario FILE` still read
//! from disk, for working on content.

use mofact_core::content::Content;

const ITEMS: &str = include_str!("../../data/items.ron");
const RECIPES: &str = include_str!("../../data/recipes.ron");
const BUILDINGS: &str = include_str!("../../data/buildings.ron");
/// The factory a new game starts with.
pub const STARTER: &str = include_str!("../../scenarios/starter-line.ron");

pub fn content() -> Result<Content, String> {
    mofact_content::load_strs(ITEMS, RECIPES, BUILDINGS).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_bundled_game_loads() {
        let content = super::content().expect("bundled data loads");
        let starter = mofact_headless::parse(super::STARTER).expect("starter parses");
        mofact_headless::build(&content, &starter).expect("starter builds");
    }
}
