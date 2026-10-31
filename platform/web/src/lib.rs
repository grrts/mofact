//! The simulation as a WebAssembly module, so the playable web link runs the
//! real engine while the native app shell (T03) and renderer (T16) are built.
//!
//! The page talks to it through one JSON entry point, so there is no
//! generated glue: the page writes a request into memory from [`alloc`],
//! calls [`call`], and reads the JSON reply from the returned pointer and
//! length. The engine is single-threaded here: the page steps it from its
//! frame loop, using the same fixed 60 Hz tick as the native thread.
//!
//! Content is `data/*.ron`, embedded at build time.

#![allow(unsafe_code)] // The exported functions exchange raw pointers with JavaScript.

use mofact_core::command::Command;
use mofact_core::geom::TilePos;
use mofact_core::ids::{EntityId, ItemId};
use mofact_core::snapshot::Snapshot;
use mofact_sim::Sim;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::BTreeMap;

const ITEMS: &str = include_str!("../../../data/items.ron");
const RECIPES: &str = include_str!("../../../data/recipes.ron");
const BUILDINGS: &str = include_str!("../../../data/buildings.ron");

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// Start an empty world. Replies with the content.
    Reset,
    SetOre {
        tiles: Vec<(i32, i32, ItemId)>,
    },
    SetStock {
        stock: BTreeMap<ItemId, u32>,
    },
    Command {
        command: Command,
    },
    Step {
        ticks: u32,
    },
    /// The world, plus delivered totals and what each entity holds.
    Snapshot,
}

#[derive(Serialize)]
pub struct View {
    #[serde(flatten)]
    pub snapshot: Snapshot,
    pub delivered: BTreeMap<ItemId, u64>,
    pub inventories: BTreeMap<u32, Vec<(ItemId, u32)>>,
}

/// The engine behind the JSON entry point. Usable natively, which is how it is tested.
pub struct Engine {
    content: mofact_core::content::Content,
    sim: Sim,
}

impl Engine {
    pub fn new() -> Result<Self, String> {
        let content =
            mofact_content::load_strs(ITEMS, RECIPES, BUILDINGS).map_err(|e| e.to_string())?;
        let sim = Sim::with_content(&content);
        Ok(Self { content, sim })
    }

    pub fn handle(&mut self, req: Request) -> serde_json::Value {
        use serde_json::{json, to_value};
        match req {
            Request::Reset => {
                self.sim = Sim::with_content(&self.content);
                to_value(&self.content).unwrap_or_default()
            }
            Request::SetOre { tiles } => {
                for (x, y, item) in tiles {
                    self.sim.set_ore(TilePos::new(x, y), &item);
                }
                json!(null)
            }
            Request::SetStock { stock } => {
                for (item, n) in stock {
                    self.sim.set_stock(&item, n);
                }
                json!(null)
            }
            Request::Command { command } => to_value(self.sim.apply(&command)).unwrap_or_default(),
            Request::Step { ticks } => {
                for _ in 0..ticks.min(600) {
                    self.sim.step();
                }
                json!(self.sim.tick())
            }
            Request::Snapshot => {
                let snapshot = self.sim.snapshot();
                let delivered = self
                    .content
                    .items
                    .iter()
                    .map(|i| (i.id.clone(), self.sim.delivered(&i.id)))
                    .filter(|(_, n)| *n > 0)
                    .collect();
                let inventories = snapshot
                    .entities
                    .iter()
                    .map(|e| (e.id.0, self.sim.inventory(EntityId(e.id.0))))
                    .filter(|(_, v)| !v.is_empty())
                    .collect();
                to_value(View {
                    snapshot,
                    delivered,
                    inventories,
                })
                .unwrap_or_default()
            }
        }
    }

    /// Handle one JSON request; errors come back as `{"error": "..."}`.
    pub fn handle_json(&mut self, text: &str) -> String {
        let reply = match serde_json::from_str::<Request>(text) {
            Ok(req) => self.handle(req),
            Err(e) => serde_json::json!({ "error": e.to_string() }),
        };
        reply.to_string()
    }
}

thread_local! {
    static ENGINE: RefCell<Option<Engine>> = const { RefCell::new(None) };
    static REPLY: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// Reserve `len` bytes for a request. The page frees nothing: [`call`] takes ownership.
#[no_mangle]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity(len);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

/// Handle the JSON request at `ptr..ptr+len` (from [`alloc`]) and return the
/// reply as `(ptr << 32) | len`. The reply stays valid until the next call.
///
/// # Safety
/// `ptr` must come from `alloc(len)` and hold `len` initialised bytes.
#[no_mangle]
pub unsafe extern "C" fn call(ptr: *mut u8, len: usize) -> u64 {
    // SAFETY: the caller passes a buffer from `alloc(len)` that it filled.
    let bytes = unsafe { Vec::from_raw_parts(ptr, len, len) };
    let text = String::from_utf8_lossy(&bytes);
    let reply = ENGINE.with(|e| {
        let mut e = e.borrow_mut();
        if e.is_none() {
            match Engine::new() {
                Ok(engine) => *e = Some(engine),
                Err(err) => return serde_json::json!({ "error": err }).to_string(),
            }
        }
        e.as_mut().expect("set above").handle_json(&text)
    });
    REPLY.with(|r| {
        let mut r = r.borrow_mut();
        *r = reply.into_bytes();
        ((r.as_ptr() as u64) << 32) | r.len() as u64
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip_runs_the_starter_line() {
        let mut e = Engine::new().unwrap();
        let reply = e.handle_json(r#"{"op":"reset"}"#);
        assert!(reply.contains("iron-plate"));
        e.handle_json(r#"{"op":"set_stock","stock":{"iron-plate":60}}"#);
        e.handle_json(r#"{"op":"set_ore","tiles":[[19,27,"iron-ore"]]}"#);
        let place = |b: &str, x: i32| {
            format!(
                r#"{{"op":"command","command":{{"Place":{{"building":"{b}","pos":{{"x":{x},"y":27}},"dir":"East","recipe":null}}}}}}"#
            )
        };
        let r = e.handle_json(r#"{"op":"command","command":{"Place":{"building":"hub","pos":{"x":26,"y":26},"dir":"East","recipe":null}}}"#);
        assert!(r.contains("Applied"), "{r}");
        for (b, x) in [
            ("drill", 19),
            ("belt", 20),
            ("belt", 21),
            ("furnace", 22),
            ("belt", 23),
            ("belt", 24),
            ("belt", 25),
        ] {
            let r = e.handle_json(&place(b, x));
            assert!(r.contains("Applied"), "{b}: {r}");
        }
        for _ in 0..30 {
            e.handle_json(r#"{"op":"step","ticks":120}"#);
        }
        let snap: serde_json::Value =
            serde_json::from_str(&e.handle_json(r#"{"op":"snapshot"}"#)).unwrap();
        let delivered = snap["delivered"]["iron-plate"].as_u64().unwrap();
        assert!(
            (45..=51).contains(&delivered),
            "{delivered} plates in a minute"
        );
        assert_eq!(snap["entities"].as_array().unwrap().len(), 8);
    }

    #[test]
    fn bad_requests_answer_with_an_error() {
        let mut e = Engine::new().unwrap();
        assert!(e.handle_json("{\"op\":\"fly\"}").contains("error"));
    }
}
