# Scenarios

Each `.ron` file here is a factory that the headless runner (`tools/headless`, T22)
builds, runs with no renderer, and checks: items per minute delivered to the hub
must be within `tolerance` of `per_minute`. CI runs them all on every pull request:

```sh
cargo run -p mofact-headless -- scenarios data
```

Fields: `stock` (starting items), `ore` (tiles with ore under them), `build`
(`At(...)` for one building, `Belts(...)` for a straight run of belts,
`Line(...)` for a straight run of anything, such as rail, and `Train(...)` for a
train with its stops), `warmup_seconds`,
`measure_seconds` and `expect`. Write the expected rate from the recipe maths
in a comment, so a failing scenario says which number is wrong.
