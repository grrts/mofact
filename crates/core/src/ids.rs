//! Typed ids, so an item id can't be passed where a recipe id is expected.

use serde::{Deserialize, Serialize};

/// Id of a placed entity (building, belt, pole...). Never reused within a save.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EntityId(pub u32);

macro_rules! content_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        ///
        /// Content ids are the stable string keys used in `data/*.ron` and in saves,
        /// such as `"iron-plate"`. The simulation interns them into dense indices.
        #[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn new(key: impl Into<String>) -> Self {
                Self(key.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl From<&str> for $name {
            fn from(key: &str) -> Self {
                Self(key.to_owned())
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

content_id!(
    /// Id of an item or fluid, from `data/items.ron`.
    ItemId
);
content_id!(
    /// Id of a recipe, from `data/recipes.ron`.
    RecipeId
);
content_id!(
    /// Id of a building type, from `data/buildings.ron`.
    BuildingId
);
