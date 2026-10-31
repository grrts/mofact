//! Content lane: loads `data/*.ron` into [`Content`] (R1: depth is data, not code).
//!
//! [`load_dir`] reads `items.ron`, `recipes.ron` and `buildings.ron` and checks
//! that every id is unique and every reference points at something that exists.
//! [`Watcher`] reloads them when a file changes, so dev builds pick up new
//! content without a rebuild. Balance checks (reachability, ratios, chain depth)
//! are the validator's job in `tools/balance` (T25), not the loader's.

use mofact_core::content::{BuildingRole, Content, Stack};
use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The files [`load_dir`] reads, in order.
pub const FILES: [&str; 3] = ["items.ron", "recipes.ron", "buildings.ron"];

#[derive(Debug)]
pub enum LoadError {
    Io {
        file: PathBuf,
        error: std::io::Error,
    },
    Parse {
        file: PathBuf,
        error: ron::error::SpannedError,
    },
    /// The content parsed but doesn't hang together; one message per problem.
    Invalid(Vec<String>),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Io { file, error } => write!(f, "{}: {error}", file.display()),
            LoadError::Parse { file, error } => write!(f, "{}:{error}", file.display()),
            LoadError::Invalid(problems) => {
                write!(f, "content has {} problem(s):", problems.len())?;
                for p in problems {
                    write!(f, "\n  - {p}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for LoadError {}

/// Load and check the content in `dir`.
pub fn load_dir(dir: &Path) -> Result<Content, LoadError> {
    let content = Content {
        items: read(&dir.join(FILES[0]))?,
        recipes: read(&dir.join(FILES[1]))?,
        buildings: read(&dir.join(FILES[2]))?,
    };
    checked(content)
}

/// Parse and check content from the text of the three files, in [`FILES`] order.
/// For builds that embed `data/` instead of reading it, such as the web build.
pub fn load_strs(items: &str, recipes: &str, buildings: &str) -> Result<Content, LoadError> {
    checked(Content {
        items: parse(FILES[0], items)?,
        recipes: parse(FILES[1], recipes)?,
        buildings: parse(FILES[2], buildings)?,
    })
}

fn parse<T: serde::de::DeserializeOwned>(file: &str, text: &str) -> Result<T, LoadError> {
    ron::from_str(text).map_err(|error| LoadError::Parse {
        file: file.into(),
        error,
    })
}

fn checked(content: Content) -> Result<Content, LoadError> {
    let problems = check(&content);
    if problems.is_empty() {
        Ok(content)
    } else {
        Err(LoadError::Invalid(problems))
    }
}

fn read<T: serde::de::DeserializeOwned>(file: &Path) -> Result<T, LoadError> {
    let text = std::fs::read_to_string(file).map_err(|error| LoadError::Io {
        file: file.to_owned(),
        error,
    })?;
    ron::from_str(&text).map_err(|error| LoadError::Parse {
        file: file.to_owned(),
        error,
    })
}

/// Referential checks: unique ids, and every stack, recipe category and cost names something real.
pub fn check(content: &Content) -> Vec<String> {
    let mut problems = Vec::new();
    let mut items = HashSet::new();
    for item in &content.items {
        if !items.insert(item.id.as_str()) {
            problems.push(format!("item `{}` is defined twice", item.id));
        }
    }
    let stacks = |owner: &str, list: &[Stack], problems: &mut Vec<String>| {
        for s in list {
            if !items.contains(s.item.as_str()) {
                problems.push(format!("{owner} uses unknown item `{}`", s.item));
            }
            if s.count == 0 {
                problems.push(format!("{owner} has a stack of 0 `{}`", s.item));
            }
        }
    };

    let mut recipes = HashSet::new();
    let mut categories = HashSet::new();
    for b in &content.buildings {
        if let BuildingRole::Crafter { categories: c, .. } = &b.role {
            categories.extend(c.iter().map(String::as_str));
        }
    }
    for r in &content.recipes {
        let owner = format!("recipe `{}`", r.id);
        if !recipes.insert(r.id.as_str()) {
            problems.push(format!("{owner} is defined twice"));
        }
        if r.outputs.is_empty() {
            problems.push(format!("{owner} has no outputs"));
        }
        if r.seconds <= 0.0 || r.seconds.is_nan() {
            problems.push(format!("{owner} must take more than 0 seconds"));
        }
        if !categories.contains(r.category.as_str()) {
            problems.push(format!(
                "{owner} has category `{}`, which no building crafts",
                r.category
            ));
        }
        stacks(&owner, &r.inputs, &mut problems);
        stacks(&owner, &r.outputs, &mut problems);
    }

    let mut buildings = HashSet::new();
    for b in &content.buildings {
        let owner = format!("building `{}`", b.id);
        if !buildings.insert(b.id.as_str()) {
            problems.push(format!("{owner} is defined twice"));
        }
        if b.size.0 == 0 || b.size.1 == 0 {
            problems.push(format!("{owner} has a zero size"));
        }
        stacks(&owner, &b.cost, &mut problems);
    }
    problems
}

/// Reloads content when any of its files changes. Poll it once a second or so in dev builds.
pub struct Watcher {
    dir: PathBuf,
    stamps: Vec<Option<(SystemTime, u64)>>,
}

impl Watcher {
    /// Start watching `dir`. The current files count as seen, so the first [`poll`](Self::poll) is quiet.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        let stamps = stamps(&dir);
        Self { dir, stamps }
    }

    /// `None` if nothing changed; otherwise the result of reloading.
    /// A failed reload is reported once; the game keeps its last good content.
    pub fn poll(&mut self) -> Option<Result<Content, LoadError>> {
        let now = stamps(&self.dir);
        if now == self.stamps {
            return None;
        }
        self.stamps = now;
        Some(load_dir(&self.dir))
    }
}

fn stamps(dir: &Path) -> Vec<Option<(SystemTime, u64)>> {
    FILES
        .iter()
        .map(|f| {
            let meta = std::fs::metadata(dir.join(f)).ok()?;
            Some((meta.modified().ok()?, meta.len()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mofact_core::ids::{ItemId, RecipeId};

    fn repo_data() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
    }

    /// A scratch copy of the repo's data that a test can edit.
    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("mofact-content-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for f in FILES {
            std::fs::copy(repo_data().join(f), dir.join(f)).unwrap();
        }
        dir
    }

    #[test]
    fn repo_content_is_the_prototype() {
        let c = load_dir(&repo_data()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            (c.items.len(), c.recipes.len(), c.buildings.len()),
            (7, 5, 5)
        );
        let circuit = c
            .recipes
            .iter()
            .find(|r| r.id == RecipeId::from("circuit"))
            .unwrap();
        assert_eq!(circuit.seconds, 1.5);
        assert_eq!(circuit.inputs.len(), 2);
        let wire = c
            .recipes
            .iter()
            .find(|r| r.id == RecipeId::from("wire"))
            .unwrap();
        assert_eq!(wire.outputs[0].count, 2, "one copper plate makes two wire");
    }

    #[test]
    fn unknown_items_and_duplicates_are_reported() {
        let mut c = load_dir(&repo_data()).unwrap();
        c.recipes[0].inputs[0].item = ItemId::from("unobtainium");
        c.items.push(c.items[0].clone());
        let problems = check(&c);
        assert!(
            problems.iter().any(|p| p.contains("unobtainium")),
            "{problems:?}"
        );
        assert!(
            problems.iter().any(|p| p.contains("defined twice")),
            "{problems:?}"
        );
    }

    #[test]
    fn parse_errors_name_the_file() {
        let dir = scratch("parse");
        std::fs::write(dir.join("items.ron"), "[ (id: \"x\", ").unwrap();
        let err = load_dir(&dir).unwrap_err().to_string();
        assert!(err.contains("items.ron"), "{err}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn new_item_appears_without_a_rebuild() {
        let dir = scratch("watch");
        let mut watcher = Watcher::new(&dir);
        assert!(watcher.poll().is_none(), "quiet until something changes");

        let items = std::fs::read_to_string(dir.join("items.ron")).unwrap();
        // Insert after the opening bracket, not "[\n": Windows checkouts may use CRLF.
        let added = items.replacen(
            '[',
            "[(id: \"steel\", name: \"Steel\", kind: Solid, shape: Plate, color: \"iron\", tier: 2),",
            1,
        );
        assert_ne!(added, items);
        std::fs::write(dir.join("items.ron"), added).unwrap();

        let reloaded = watcher.poll().expect("change seen").unwrap();
        assert!(reloaded.items.iter().any(|i| i.id == ItemId::from("steel")));
        assert!(watcher.poll().is_none(), "each change is reported once");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
