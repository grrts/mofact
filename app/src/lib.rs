//! The Mofact app. [`Game`] is the game itself; two shells run it: SDL3
//! (desktop and phones, with the simulation on its own thread, T03) and the
//! browser canvas in `platform/web`, which ticks the simulation on the frame
//! loop. Both run the same game from the same data.
//!
//! Going to the background (a phone app switch, or a minimised desktop
//! window) drops the GPU surface and pauses the simulation; coming back makes
//! a new surface and resumes. The GPU device and everything on it survive.
//!
//! `mofact --frames N` presents N frames and exits, going to the background
//! and back halfway through. CI runs it to prove the shell works on a real
//! (software) GPU. `mofact --screenshot DIR` writes the scene at three zooms
//! as PNGs instead of opening a window. `--scenario FILE` starts from one of
//! the headless scenarios instead of the saved game.
//!
//! The game autosaves every 30 seconds and on going to the background, to
//! `--save FILE` (by default the platform's per-user data folder), and opens
//! that save next time.

#[cfg(not(target_arch = "wasm32"))]
mod autosave;
mod built;
mod bundled;
pub mod game;
#[cfg(not(target_arch = "wasm32"))]
mod native;
mod ore;
#[cfg(test)]
mod replay;
pub mod sound;
#[cfg(not(target_arch = "wasm32"))]
mod surface;

pub use bundled::content;
pub use game::Game;
#[cfg(not(target_arch = "wasm32"))]
pub use native::{main, run};
