# Contributing

Most work is done by AI agents in parallel lanes, one task per pull request,
taken from the Task board in the build plan.

## One task, one pull request

- A task touches one lane and stays under about 600 changed lines. Split anything bigger before starting.
- The pull request names its task id (such as T07), its requirement ids and the check that proves it.
- Start a task only when everything it needs is merged.

## Definition of done

A task is done when:

1. CI is green: format, clippy with warnings as errors, and tests on Linux, Windows and macOS.
   Device builds, headless scenarios, golden screenshots and the content validator join CI as their tasks land.
2. The pull request description shows its proof: numbers, a screenshot or a clip.
3. It is merged to `main`.

## Contracts live in `crates/core`

Commands, snapshots, ids and the content schema are shared by every lane, so
they change only through a task of their own. Don't change `crates/core` as a
side effect of lane work; open a contract task instead.

## Simulation rules

- Only the simulation thread changes the world. Everything else sends commands.
- The simulation is deterministic: the same save and inputs give the same result
  on every platform. No wall-clock time, no unordered iteration that affects state,
  no randomness without a seed.
- Systems cost almost nothing when nothing changes: segments and timers, not per-tile, per-tick work.
