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

## Releasing

There is one version number, the workspace's in `Cargo.toml`. Android's
`versionName` reads it, and `Info.plist` must match it (a test checks).
Build numbers come from the CI run number.

1. Move the entries under `## Unreleased` in `CHANGELOG.md` into a new
   `## X.Y.Z` heading, and set `version = "X.Y.Z"` in `Cargo.toml` and
   `CFBundleShortVersionString` in `platform/ios/Info.plist`.
2. Merge that, then tag the merge commit `vX.Y.Z` and push the tag.
3. The Builds workflow builds all three platforms. Its Release job checks the
   tag against `Cargo.toml` and the changelog, then publishes a GitHub release
   with the builds and that changelog entry. Store uploads join once the
   signing keys exist (see `docs/decisions.md` and the platform READMEs).

## Words the player reads

Every word on screen comes from `crates/ui/lang/<code>.txt` through `tr("key")`
or `trf("key", &[..])`; content names come from `data/*.ron` in English and are
translated by id (`item.<id>`, `building.<id>`, …). A pull request that adds a
string or content name adds it to all seven files. CI fails on a missing
translation, a name left untranslated, English showing on a Japanese or Chinese
screen, or a toast or label written straight into the code. New Japanese or
Chinese characters need `tools/fonts/subset.py` run again (the
`every_character_has_a_glyph` test says when).
