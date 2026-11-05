Review the final 17-hour Model2 model's losses against the HP-only search opponent, and identify other concrete bugs in observation tracking, world reconstruction, action generation, search, or evaluation.

Workspace: C:\Users\main\Documents\AI\poke\njk
Read AGENTS.md and Model1-experimental/AGENTS.md first. The experimental model depends on Model1-experimental/vendor/Ninjask, not directly on the adjacent Ninjask checkout. Preserve checkpoints, frozen evaluation executables, and original traces. Do not train or play additional evaluation games without authorization; use archived traces and isolated diagnostic inference/reconstruction first.

Evidence:
- Model1-experimental/Model2/runs/opponent-suite-insample-heldout-20261104/
- training-hp/traces and heldout-hp/traces: 200 games each; final model lost 71 and 65 respectively. Trace outcome 0=learner win, 1=learner loss, 2=draw. learner_side gives physical side.
- hp-loss-value-swings.json ranks 1,150 consecutive searched-position changes across 136 losses. Terminal transitions without a subsequent search are excluded. These are matrix search returns, NOT raw neural values or win probabilities.
- value-flip-isolation/results.json and subset input/prediction files contain the game #124 feature ablations.
- Frozen final weights, evaluator, source snapshots, dex, training/heldout teams, summary.json and provenance.json are in that suite folder.

Already diagnosed and fixed in both Ninjask/src and Model1-experimental/vendor/Ninjask/src:
Game training #124: Cursed Body disabled Dragon Claw. world.rs incorrectly reconstructed Disable using last_move, so it disabled Rock Slide at turn 8 and Protect at turn 9. Removing only Protect's column from the unchanged turn-8 matrix reproduced virtually all of the -0.8823 to +1.0269 search flip. All 16 combinations of four changed observation features kept the raw value near -0.888. Tracker now stores disabled_move at effect onset, clears it on effect end/switch, preserves it through Illusion field-state copying, and reconstruction uses that field. Tests: tracker_disable.rs. Do not mistake original archived traces for post-fix behavior.

Other findings to investigate, not assume fixed:
- Heldout #12 turn 4: Expanding Force existed in Meowstic's assumed catalog moveset but was absent from eight searched joint responses (four Protect, three Alluring Voice, one Shadow Ball). Six candidates come from policy ranking, two from random sampling. It was not previously revealed. Distinguish shortlist design limitations from code bugs; discarded logits were not archived.
- Training #59 turn 6: Double-Edge absent from responses, double KO.
- Training #181 turn 3: Aqua Jet absent from responses, double KO.
- Heldout #192 turn 14: optimistic value before poison left both own Pokemon at 7 HP. Investigate toxic-stage, residual effects, and reconstruction fidelity.
- Training #123 turn 6: Make It Rain missed Farigiraf, KO'd Incineroar, lowered own SpA, then Sylveon entered. Separate randomness from faulty reconstruction/value estimation.

Method:
1. Confirm trace schema, side/perspective, actual battle events and exact run settings.
2. Rank losses by large value changes and prediction/realized-event discrepancies; include terminal blunders and do not restrict review to the five examples above.
3. Separate raw neural input/value changes from assumed world changes, legal action sets, shortlist composition, chance samples, immediate rewards, bootstrap values, and solver output.
4. For each suspected bug, trace the public evidence to the tracker, reconstructed engine state and decision consequence. Use isolated feature/field substitutions and minimal regression cases. Respect hidden information; do not give the AI private opponent data.
5. Check known move/PP/status/volatile identity and duration, toxic counters, stat changes, speed, target routing, forced replacements, types/immunities, Protect history, move prerequisites, and sampled-set consistency. These are investigation areas, not established defects.
6. Report confirmed bugs separately from hypotheses, unavoidable randomness and search-budget limitations. Give concrete game/turn/event examples and paths/lines. Do not infer strength changes from individual replays or changing-league plots.
7. Propose minimal fixes and meaningful tests. Do not broadly rewrite the engine or retrain as part of this review.

Deliver a prioritized report with evidence, causal isolation, affected scope, and remaining uncertainty. Include any replay result saved under Model1-experimental/Model2/runs/disable-fix-replay-124-20261104 if present.

POST-FIX REPLAY RESULT (important correction to earlier causal interpretation):
The frozen original evaluator exactly reproduced game #124 at seed 781062, side 0, teams 383/26. The rebuilt fixed evaluator still loses on turn 9: Garchomp Rock Slide KOs Gengar before Shadow Ball. Turn-9 search is still optimistic (+1.02196 versus original +1.02690). Thus the Disable bug is real and regression-tested, but fixing it does not eliminate the value flip. Inspect the fixed trace's restored Protect response, two chance samples, inferred stats/speed, and payoff matrix to explain remaining optimism. Do NOT claim Disable alone accounts for post-fix behavior. The side-swapped companion wins in both versions. Both players used the same reconstruction fix. No weights changed. Audits have complete traces and no trace errors; only two games per version, not a strength estimate.

Fixed turn-9 matrix detail: Protect IS restored, alongside Earthquake and Rock Slide; Shadow Ball has +1.032754 immediate reward and no bootstrap contribution against all three. Inspect why simulated terminal wins disagree with actual Rock Slide moving first and KOing Gengar, including inferred speed and two-sample chance outcomes.

FOLLOW-UP CONFIRMED: speed-diagnostic/findings.json and its trace record actual Garchomp Speed 169 vs reconstructed 149; Gengar is 165 in both. Thus simulated Shadow Ball moves first and KOs Garchomp, while real Rock Slide moves first and KOs Gengar. Both chance samples reuse one sampled opponent set. Consecutive Protect fails in both samples, explaining the positive Protect column too. These are terminal rewards with zero neural bootstrap. Tracker bounds remain 109–169 because its conservative speed inference rejects unknown abilities/active volatiles and Cursed Body isn't whitelisted. Reconstruction doesn't consult speed bounds. Instrumented replay is exactly identical to fixed replay after removing added diagnostics. Actual private stats were used only in diagnostic output, never in decisions.

LATEST CHANGE: Model2 search now enumerates tracked min/max opposing Speed combinations and takes per-cell worst total return after averaging chance samples. See techDebt.txt for conservative endpoint limitations. Both learner and HP-only use this. Same setup replay in Model2/runs/speed-bounds-replay-124-20261104 still loses, now turn7; companion wins. The battle diverged earlier, so it does not directly isolate the original turn9. Eight Model2 tests passed and traces are complete, zero search fallbacks. No retraining or weights changes.

Current search now uses probability-weighted Battle::enumerate_outcomes AlmostPrecise; speed-minimum aggregation remains. The replay was killed at the user's request after ~562s with no finished game, versus 4.678s for the sampled pair. See enumerated-replay-124-20261104/performance-review.json. Do not automatically launch more enumeration games; investigate isolated cells with progress instrumentation if authorized.
