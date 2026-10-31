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
        ///
        /// The key is shared, so cloning an id (as every snapshot does, for
        /// every entity, every tick) costs a reference count, not an allocation.
        #[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(std::sync::Arc<str>);

        impl $name {
            pub fn new(key: impl AsRef<str>) -> Self {
                Self(key.as_ref().into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl From<&str> for $name {
            fn from(key: &str) -> Self {
                Self(key.into())
            }
        }

        impl From<String> for $name {
            fn from(key: String) -> Self {
                Self(key.into())
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
content_id!(
    /// Id of a research, from `data/research.ron`.
    ResearchId
);
