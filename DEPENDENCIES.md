# Dependency snapshots

`vendor/Ninjask` is a source snapshot of the local Ninjask checkout, including the user-added tracked speed ranges. It retains its upstream license and notices. Its source repository is https://github.com/dcabatin/Ninjask and the local fork is https://github.com/CalebBrody/Ninjask.

`vendor/rl4burn` is published version 0.1.1 plus the local legal-action sampling retry guard and regression tests. Its MIT license and third-party notices are included.

Team data, generated dex data, training checkpoints, logs and reports are intentionally not committed. Import your teams into `data/`; supply the dex using `--dex PATH` or generate it at `vendor/Ninjask/data/derived/format/gen9championsvgc2026regmc.json` using the Ninjask project. Existing model weights are compatible with the baseline architecture.
