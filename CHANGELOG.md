# Changelog

Each release's entry becomes the notes of its GitHub release. To cut a
release, see "Releasing" in CONTRIBUTING.md.

## Unreleased

- **Ore runs out:** the patches round the hub are thin, so the starter
  iron runs dry in about an hour and a half of a first factory's digging,
  and ore gets richer the further out a patch lies: about six times as
  much a tile a thousand tiles away. A drill digs the ore under it and the
  eight tiles round it, so it runs dry with its patch rather than its tile.
  Drills running low or mined out show in the problems list, the inspect
  sheet says how much ore a drill has left, and a toast says when one runs
  dry.
- **Fixed:** on an iPhone or iPad (Safari), building anything (usually a
  drill, the first thing placed) stopped the web game, because Safari can't
  vibrate. The game now buzzes only where the browser can.
- **Power cables:** wires hang between poles as sagging cables with a
  highlight and a shadow on the ground, tied to an insulator on top of each
  pole and to a socket on every building that uses or makes power.
- **Blueprints:** a Select tool drags a box round buildings to copy, cut,
  delete or save them; a blueprint in hand slides with a finger, turns, and
  builds with one tap, and the library keeps up to 24 on the device.
- **Drones:** after the Drones research, a powered drone port keeps four
  drones that fly 50 tiles across, and ports whose squares meet make one
  network. Drones fill requester chests from provider chests or the hub,
  charging at ports on long trips, and a paste where ports reach is planned
  as ghosts that drones build as the stock comes in.

## 0.1.0

The first playable build of the whole game, from the first plates to the
skyhook.

- **Factory:** belts with splitters, undergrounds and three speed tiers,
  drills, furnaces, assemblers, labs and power grids, and fluids with pipes,
  pumps and steam power.
- **Logistics and content:** rails and trains with signals and schedules,
  oil, chemistry, electronics and light alloys, alternate recipes, and eight
  research tiers ending in the skyhook megaproject.
- **World:** a generated map with biomes, rivers, cliffs, trees to clear and
  ore that runs out, a world map with pins, and day and night.
- **Play:** a guided first hour with goals, a problems list, a recipe browser
  with a ratio calculator, undo and redo, three save slots and world presets.
- **Phones first:** touch controls sized for a finger, a radial build menu,
  haptics, a low-power mode, a colour-blind mode, and saves that survive
  going to the background.
- **Selling:** a free demo up to tier 4 and a one-time unlock. On phones it is
  sold through RevenueCat; elsewhere a free sandbox unlocks it.
- **Builds:** a Windows exe, an Android APK and an iOS app for every commit,
  and the same game on the web link.
