//! What the game sounds like (T41): the machines in view hum by kind, a
//! machine that stalls in view says how, and building, removing, a refusal,
//! a goal met and research finished each have a cue. The synth itself is in
//! `mofact_audio`; the platform shell pulls its samples.

use mofact_audio::{Audio, Cue, Hum, Machine, Soundscape, State};
use mofact_core::command::{Command, CommandResult};
use mofact_core::content::{BuildingRole, Content};
use mofact_core::geom::TilePos;
use mofact_core::ids::BuildingId;
use mofact_core::snapshot::{MachineState, Snapshot};
use mofact_render::Camera;
use std::collections::{HashMap, HashSet};
use web_time::Instant;

/// What each building sounds like while working.
fn hum_for(role: &BuildingRole) -> Option<Hum> {
    Some(match role {
        BuildingRole::Belt { .. } | BuildingRole::Splitter | BuildingRole::Underground { .. } => {
            Hum::Belt
        }
        BuildingRole::Miner { .. } => Hum::Drill,
        BuildingRole::Crafter { categories, .. } => {
            let has = |k: &str| categories.iter().any(|c| c == k);
            if has("chemistry") {
                Hum::Chemistry
            } else if has("smelting") || has("boiling") || has("refining") {
                Hum::Fire
            } else {
                Hum::Assembler
            }
        }
        BuildingRole::Lab { .. } => Hum::Lab,
        BuildingRole::Engine { .. } => Hum::Engine,
        BuildingRole::Pump { .. } | BuildingRole::Well { .. } => Hum::Pump,
        BuildingRole::Generator { .. } => Hum::Wind,
        _ => return None,
    })
}

pub struct Sound {
    audio: Audio,
    scape: Soundscape,
    /// Each building's hum and size.
    kinds: HashMap<BuildingId, (Option<Hum>, (u8, u8))>,
    last: Option<Instant>,
    /// The goal and research count last frame, to hear them change.
    goal: Option<Option<u16>>,
    done: usize,
}

impl Sound {
    pub fn new(content: &Content) -> Self {
        Self {
            audio: Audio::default(),
            scape: Soundscape::new(),
            kinds: content
                .buildings
                .iter()
                .map(|b| (b.id.clone(), (hum_for(&b.role), b.size)))
                .collect(),
            last: None,
            goal: None,
            done: 0,
        }
    }

    /// The synth, for the platform's audio callback.
    pub fn audio(&self) -> Audio {
        self.audio.clone()
    }

    /// Use `audio` instead, as a platform does when its output runs at
    /// another sample rate. Muting carries over.
    pub fn use_audio(&mut self, audio: Audio) {
        let muted = self.audio.lock().muted();
        audio.lock().set_muted(muted);
        self.audio = audio;
    }

    pub fn set_muted(&self, muted: bool) {
        self.audio.lock().set_muted(muted);
    }

    /// A cue for what the latest commands did: a build, a removal (whose
    /// undo is a build) or a refusal. One of each a frame at most.
    pub fn results(&self, results: &[CommandResult]) {
        let mut cues = Vec::new();
        for r in results {
            let cue = match r {
                CommandResult::Applied {
                    created: Some(_), ..
                } => Cue::Place,
                CommandResult::Applied { undo, .. }
                    if undo.iter().any(|c| matches!(c, Command::Place { .. })) =>
                {
                    Cue::Remove
                }
                CommandResult::Refused(_) => Cue::Refused,
                CommandResult::Applied { .. } => continue,
            };
            if !cues.contains(&cue) {
                cues.push(cue);
            }
        }
        let mut synth = self.audio.lock();
        for cue in cues {
            synth.play(cue, 0.0);
        }
    }

    /// A tap on the HUD did something.
    pub fn tap(&self) {
        self.audio.lock().play(Cue::Tap, 0.0);
    }

    /// One frame: set the hums from what's in view of `camera` on a screen
    /// `size` pixels across, and chime for a goal met or research done.
    pub fn frame(&mut self, snap: &Snapshot, camera: &Camera, size: (u32, u32)) {
        let now = Instant::now();
        let dt = self
            .last
            .map_or(0.0, |t| now.duration_since(t).as_secs_f32().min(0.5));
        self.last = Some(now);

        let ppt = camera.pixels_per_tile.max(1.0);
        let half = [size.0 as f32 / 2.0 / ppt, size.1 as f32 / 2.0 / ppt];
        // Zoomed out, the factory is further away and each machine counts less.
        let zoom = (ppt / 32.0).clamp(0.3, 1.0);
        let moving: HashSet<TilePos> = snap.belt_items.iter().map(|i| i.tile).collect();
        let machines = snap.entities.iter().filter_map(|e| {
            let &(hum, (w, h)) = self.kinds.get(&e.building)?;
            let cx = e.pos.x as f32 + f32::from(w) / 2.0;
            let cy = e.pos.y as f32 + f32::from(h) / 2.0;
            let nx = (cx - camera.center[0]) / half[0].max(0.5);
            let ny = (cy - camera.center[1]) / half[1].max(0.5);
            let near = (1.25 - nx.abs().max(ny.abs())).clamp(0.0, 1.0) * zoom;
            let state = if hum == Some(Hum::Belt) {
                // A belt works while it carries something.
                if moving.contains(&e.pos) {
                    State::Working
                } else {
                    State::Idle
                }
            } else {
                match e.state {
                    MachineState::Idle => State::Idle,
                    MachineState::Working => State::Working,
                    MachineState::Starved => State::Starved,
                    MachineState::Blocked => State::Blocked,
                    MachineState::NoPower => State::NoPower,
                }
            };
            Some(Machine {
                id: e.id.0,
                hum,
                state,
                pan: nx.clamp(-1.0, 1.0),
                near,
            })
        });
        let mut synth = self.audio.lock();
        self.scape.update(&mut synth, dt, machines);

        let goal = snap.goal.as_ref().map(|g| g.index);
        let done = snap.research.done.len();
        if let Some(was) = self.goal {
            let met = match (was, goal) {
                (Some(a), Some(b)) => b > a,
                (Some(_), None) => true,
                _ => false,
            };
            if done > self.done {
                synth.play(Cue::Research, 0.0);
            } else if met {
                synth.play(Cue::Goal, 0.0);
            }
        }
        self.goal = Some(goal);
        self.done = done;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mofact_core::geom::Dir;
    use mofact_core::ids::EntityId;
    use mofact_core::snapshot::{EntityView, GoalView};

    fn drill(id: u32, x: i32, state: MachineState) -> EntityView {
        EntityView {
            id: EntityId(id),
            building: "drill".into(),
            pos: TilePos::new(x, 0),
            dir: Dir::East,
            recipe: None,
            ore: None,
            grid: None,
            exit: false,
            fluid: None,
            links: 0,
            state,
            progress: 0.0,
        }
    }

    fn camera() -> Camera {
        Camera {
            center: [0.0, 0.0],
            pixels_per_tile: 32.0,
        }
    }

    #[test]
    fn working_drills_in_view_hum_on_their_side() {
        let content = crate::content().unwrap();
        let mut sound = Sound::new(&content);
        let mut snap = Snapshot {
            entities: vec![drill(1, 8, MachineState::Working)],
            ..Snapshot::default()
        };
        sound.frame(&snap, &camera(), (1280, 720));
        let audio = sound.audio();
        assert!(audio.lock().level(Hum::Drill) > 0.2);
        // On the right: louder on the right.
        let mut out = vec![0.0; 48_000];
        audio.render(&mut out);
        let side = |c: usize| out[24_000..].chunks(2).map(|f| f[c].abs()).sum::<f32>();
        assert!(side(1) > side(0), "{} {}", side(0), side(1));
        // Far off screen it is quiet.
        snap.entities = vec![drill(1, 500, MachineState::Working)];
        sound.frame(&snap, &camera(), (1280, 720));
        assert_eq!(audio.lock().level(Hum::Drill), 0.0);
    }

    #[test]
    fn a_goal_met_chimes_and_muting_holds_across_a_new_output() {
        let content = crate::content().unwrap();
        let mut sound = Sound::new(&content);
        let mut snap = Snapshot {
            goal: Some(GoalView {
                index: 0,
                have: 0,
                need: 1,
            }),
            ..Snapshot::default()
        };
        sound.frame(&snap, &camera(), (1280, 720));
        assert_eq!(
            sound.audio().lock().playing(),
            0,
            "no chime on the first frame"
        );
        snap.goal = Some(GoalView {
            index: 1,
            have: 0,
            need: 1,
        });
        sound.frame(&snap, &camera(), (1280, 720));
        assert_eq!(sound.audio().lock().playing(), 1);

        sound.set_muted(true);
        sound.use_audio(Audio::new(44_100));
        assert!(sound.audio().lock().muted());
        assert_eq!(sound.audio().lock().rate(), 44_100);
    }

    #[test]
    fn builds_removals_and_refusals_each_cue_once() {
        let content = crate::content().unwrap();
        let sound = Sound::new(&content);
        let placed = CommandResult::Applied {
            undo: Vec::new(),
            created: Some(EntityId(1)),
        };
        sound.results(&[placed.clone(), placed]);
        assert_eq!(sound.audio().lock().playing(), 1);
        sound.results(&[CommandResult::Refused(
            mofact_core::command::CommandError::Blocked,
        )]);
        assert_eq!(sound.audio().lock().playing(), 2);
    }
}
