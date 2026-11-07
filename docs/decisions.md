# Decisions

Product decisions that code depends on, with the date they were made.

## Price (2026-11-07)

- A free demo and one one-time unlock. No ads, no gems and no other in-app
  purchases.
- **$9.99** for the full game, cut to **$7.99 for launch week**. Other
  currencies use each store's matching price tier (T69).
- This follows the art style and price research of 2026-10-28. Balatro and
  Slay the Spire sell at $9.99 on mobile, and shapez at $6.99 with less
  content.
- The demo is tiers 1 to 4 (`mofact::shop::DEMO_TIER`). The full game opens
  tiers 5 to 8 and the skyhook.
- Plan for 3–5% of demo players buying, not the plan's original 8%.

## The web link (2026-11-07)

- The web link plays the **full game, unlocked**, until a store build is live,
  so that anyone with the link can play everything.
- Once the game is in a store, the web link becomes the demo: it gets the
  `Sandbox` shop removed and a shop that sends players to the store pages
  instead of unlocking.
