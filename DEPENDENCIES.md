# Dependency snapshots

The active Ninjask dependency is the adjacent `../Ninjask` checkout, including tracked speed bounds, deferred promises, and `PreciceButSame` enumeration. `vendor/Ninjask` retains the older snapshot. Frozen training bundles copy the active engine into their own `build/vendor/Ninjask` and record source hashes. Its source repository is https://github.com/dcabatin/Ninjask and the local fork is https://github.com/CalebBrody/Ninjask.

`rl4burn` is fetched from https://github.com/RPP1011/rl4burn, branch `master`.
Cargo.lock records the exact Git revision. Use `cargo update -p rl4burn` to
intentionally refresh it; ordinary locked builds retain that revision. Upstream
includes the gradient-clipping fix. `vendor/rl4burn` is retained as the historical
0.1.1 snapshot with local patches, but is no longer compiled.

Upstream SyncVecEnv is sequential. Model1 requires `--collection-workers 1`;
the former local parallel extension is not part of the upstream dependency.
Model2 already collects sequentially. The local legal-action sampling retry guard
is also absent upstream; action sampling now uses the upstream implementation.
Model architecture is unchanged.

Team data, generated dex data, training checkpoints, logs and reports are intentionally not committed. Import your teams into `data/`; supply the dex using `--dex PATH` or generate it at `vendor/Ninjask/data/derived/format/gen9championsvgc2026regmc.json` using the Ninjask project. Existing model weights are compatible with the baseline architecture.
