# Changelog

Each release's entry becomes the notes of its GitHub release. To cut a
release, see "Releasing" in CONTRIBUTING.md.

## Unreleased

- **Hub orders:** once the guided goals are done, the hub posts an order
  against the clock, such as 150 gears in 10 minutes, picked from the
  latest things the factory makes. Shipping it takes the items out of the
  stock and pays a bigger bundle of other parts. Each order shipped in a
  row asks a fifth more and pays a fifth more, up to three times as much;
  a missed one starts the streak over. The card shows the count, the
  clock (red in the last minute) and the pay.
- **Warehouses:** Logistics also unlocks a warehouse, a two-by-two store
  for up to 1,600 items between belts. Belts feed it from any side but its
  arrow, and it hands what it holds back out of its arrow, every kind in
  turn, or only the one item you pick in its sheet. Use it to buffer a line
  or to keep a stock part-way along the factory. What it holds is not your
  stock: only the hub's counts. Drones take from it like a provider chest.
- **More splitters:** Logistics now also unlocks a three-way splitter
  (left, ahead and right in turn), a priority splitter (fills the line ahead
  first and only sends the overflow to the sides) and a filter splitter (one
  chosen item goes ahead, everything else to the sides). Tap a filter
  splitter to pick its item from big item buttons. Each kind has its own
  colour so they are easy to tell apart.
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
