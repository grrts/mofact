# Store listing copy

One listing, worded for each store's fields and limits. The character counts
in brackets were checked with `python3 tools/store/check.py` and must stay
under each store's limit. Prices are in [docs/decisions.md](../decisions.md):
$9.99, $7.99 in launch week, one unlock and nothing else for sale.

What the copy may claim is what the game ships today: 8 tiers, 33 research,
33 buildings, 53 items and 64 recipes, ending in the skyhook (about 8 hours
for the playthrough bot; a person takes longer). If more tiers land, update
the numbers here and in the long descriptions.

## Shared lines

- **Name:** Mofact
- **Tagline:** Build a factory that runs itself
- **Genre:** Simulation / Strategy (factory building, automation)
- **Age rating:** Everyone / 4+ / PEGI 3: no violence, no chat, no ads, no
  data collected.

## Google Play

**App name** (limit 30): `Mofact: Factory Builder` [23]

**Short description** (limit 80):

`Mine, smelt and belt your way to a factory that runs itself. Try 4 tiers free.` [78]

**Full description** (limit 4000):

```
Build a factory that runs itself.

Mofact is a factory automation game made for phones. Start with one drill and one furnace. Lay belts, chain machines into production lines, research new technology and grow a sprawling factory, until it builds the skyhook and lifts your work into orbit.

BUILT FOR TOUCH
• Drag to build a belt, tap to place a machine, press and hold for the radial menu
• Clear sheets for building, research, recipes and trains, sized for a thumb
• Undo and redo for every change
• Plays offline, anywhere. No account, no ads, no timers

A FACTORY THAT REALLY RUNS
• Every item on every belt is simulated, 60 times a second
• Drills, furnaces, assemblers, splitters and underground belts
• Pipes, pumps and tanks for water, steam and oil
• Power from wind, steam and the sun, carried on pole networks
• Trains on rails with signals and stops, to bring ore from far away

KNOW WHAT IS GOING ON
• A problems list that shows every starved or blocked machine, and takes you there
• A recipe browser with a ratio calculator: say how many circuits a minute you want, and see the machines it takes
• A world map with ore patches and pins

A WORLD TO EXPAND INTO
• Generated maps with forests, rivers, lakes, cliffs, desert, tundra and volcanic land
• Map presets: standard, rich ore, lakes or dry
• Day and night, with your machines glowing in the dark

FROM ONE DRILL TO ORBIT
• 8 tiers, 33 research, 33 buildings, 53 items and 64 recipes
• Steel, motors, oil, plastics, silicon, titanium, superconductors, AI cores and fusion
• Goals that teach the game as you play, then step aside
• The skyhook: a four-stage megaproject that ends the game

FOR EVERYONE
• Colour-blind mode: safer colours, and marks on items that look alike
• Haptics on supported phones
• Three save slots

TRY IT, THEN UNLOCK IT
The first 4 tiers are free to play. One purchase unlocks the full game, tiers 5 to 8 and the skyhook. No ads, no gems, no energy, nothing else to buy. Ever.
```


**Category:** Simulation. **Tags:** Simulation, Strategy, Building, Offline.
**Contains ads:** No. **In-app purchases:** Yes (one, the full game).

## App Store

**Name** (limit 30): `Mofact: Factory Builder` [23]

**Subtitle** (limit 30): `Automate, research, expand` [26]

**Promotional text** (limit 170, can change without a review):

`Launch week: the full game is $7.99 instead of $9.99. Start with one drill and grow a factory that builds a skyhook. The first four tiers are free.` [147]

After launch week:

`Start with one drill and grow a factory that builds a skyhook. The first four tiers are free; one purchase unlocks the rest, with no ads and nothing else to buy.` [161]

**Keywords** (limit 100, commas, no spaces, no words already in the name):

`automation,simulation,crafting,belts,logistics,industry,strategy,offline,trains,tycoon,sandbox,idle` [99]

The name's words (Mofact, Factory, Builder) already count, so they are left
out. Other games' names are left out too: Apple rejects them (guideline 2.3.7).

**Description** (limit 4000): the Google Play full description above, word for
word. The App Store shows plain text the same way.

**Primary category:** Games, Simulation. **Secondary:** Games, Strategy.

**In-app purchase:** "Full game", non-consumable, $9.99 (tier 10), product
attached to RevenueCat's `full_game` entitlement. Display name (limit 30)
`Full game` [9]; description (limit 45) `Tiers 5 to 8 and the skyhook. No ads.` [37]

## Steam

**Short description** (limit 300):

`Build a factory that runs itself. Mine ore, belt it into furnaces and assemblers, research new technology and grow a sprawling, fully simulated factory, from one drill to a skyhook into orbit. Made for touch first, and just as at home with a mouse.` [248]

**About this game** (no limit that matters; about 1,400 characters):

```
Mofact is a factory automation game. Start with one drill and one furnace, lay belts, chain machines into production lines and research your way through eight tiers of technology, until your factory builds the skyhook.

A FACTORY THAT REALLY RUNS
Every item on every belt is simulated, 60 times a second. Build drills, furnaces, assemblers, splitters and underground belts; run water, steam and oil through pipes; power it all with wind, steam and solar on pole networks; and bring ore in from far away by train.

KNOW WHAT IS GOING ON
A problems list finds every starved or blocked machine. The recipe browser has a ratio calculator: ask for 60 circuits a minute and see every machine it takes. The world map shows your factory, ore patches and pins.

A WORLD TO EXPAND INTO
Generated maps with forests, rivers, lakes, cliffs, desert, tundra and volcanic land, with presets for rich ore, lakes or dry land, and day and night.

FROM ONE DRILL TO ORBIT
8 tiers, 33 research, 33 buildings, 53 items and 64 recipes: steel, motors, oil, plastics, silicon, titanium, superconductors, AI cores and fusion, ending in a four-stage megaproject.

MADE FOR TOUCH, AT HOME ON A DESK
Mofact was designed on phones, so every action is a tap or a drag. On a PC, the mouse and hotkeys do the same.

FOR EVERYONE
Colour-blind mode, three save slots, plays offline. No account, no ads, nothing else to buy.

Try the demo: the first four tiers, free.
```

**Genre:** Simulation, Strategy, Indie. **Tags:** Automation, Base Building,
Factory, Crafting, Resource Management, Simulation, Strategy, Building,
Logistics, Sandbox, Singleplayer, 2D, Top-Down, Casual, Relaxing, Indie,
Management, Trains, Exploration, Procedural Generation.

**Features:** Single-player, Steam Cloud (once Steamworks turns it on), Full
controller support: no.
