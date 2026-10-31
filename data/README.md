# Content data

Items, recipes, buildings, research and biomes live here as RON files that
deserialize into the schema in `crates/core/src/content.rs` (R1: depth is data,
not code). The content lane owns this folder.

- `items.ron`, `recipes.ron`, `buildings.ron`: the loader and hot reload arrive in T12, the prototype's content in T13.
- The validator and balance report (`tools/balance`, T25) run on every change in CI.
