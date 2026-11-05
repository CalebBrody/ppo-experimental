"""Summarize the preserved four-pass reconstruction repair experiment."""
import json,hashlib,re
from pathlib import Path
root=Path(__file__).resolve().parents[1];out=root/'Model2/runs/reconstruction-fix-loop-20261105'
read=lambda p:json.loads(p.read_text())
comparison=read(out/'iteration-4-comparison-baseline.json');validation=read(out/'iteration-4-validation.json')
timing=dict(training_games_seconds=0,model_training_seconds=0,evaluation_games_seconds=0,evaluation_other_seconds=0,played_games=0,normal_matrix_simulations=0,additional_diagnostic_simulations=0)
iterations=[];checks=[]
for n in range(1,5):
 wins=losses=0
 for pool in ['training','heldout']:
  dest=out/f'iteration-{n}-{pool}';e=read(dest/'evaluation.json');t=read(dest/'timing.json');c=read(dest/'issue-check.json')['evaluations'][0]
  wins+=e['wins'];losses+=e['losses'];timing['played_games']+=e['games'];timing['normal_matrix_simulations']+=sum(e['search_simulations'])
  for k in ['evaluation_games_seconds','evaluation_other_seconds']:timing[k]+=t[k]
  checks.append(dict(iteration=n,pool=pool,trace_complete=c['trace_complete'],errors=c['errors'],issues=c['issues'],search_fallbacks=e['search_fallbacks']))
  for f in (dest/'traces').glob('game-*.json'):
   g=read(f)
   timing['additional_diagnostic_simulations']+=sum(r['simulations'] for d in g['decisions'] for r in (d.get('search') or {}).get('causal_repairs',[]))
 iterations.append(dict(iteration=n,wins=wins,losses=losses))
files=[root.parent/'Ninjask'/p for p in ['src/tracker.rs','src/world.rs','src/battle/choices.rs','tests/tracker_reconstruction.rs']]+[root/p for p in ['src/env.rs','src/diagnosis.rs','Model2/runs/opponent-suite-insample-heldout-20261104/final.bin','target/release/model1-reconstruction-loop-4.exe']]
provenance=dict(checkpoint_unchanged=True,seed=781000,matrix_samples=2,regret_iterations=256,search='frozen final17h single-world search, HP-only opponent',architecture_changed=False,files={str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in files},iterations=iterations,checks=checks,timing=timing)
(out/'provenance.json').write_text(json.dumps(provenance,indent=2));(out/'timing-summary.json').write_text(json.dumps(timing,indent=2))
lines=['RECONSTRUCTION REPAIR AND REPLAY REPORT','', 'Original five categories fixed in both Ninjask copies:',
'1. Own nature and stat points survive reconstruction, so Mega/form changes recalculate correct stats.',
'2. Protean/Libero activation history is retained and reset at switch/ability/form transitions.',
'3. Active Unnerve initializes its berry-blocking activation flag.',
'4. Charging move identity, engine-specific durations and known committed target are retained. Own request metadata supplies own targets omitted from public logs.',
'5. Weather/terrain extension uses the setter and item known at onset, not any holder on either team.',
'', 'Further passes found and fixed:',
'6. Previous-action failure tracking for Stomping Tantrum/Temper Flare, including partial spread success and the distinction between failure and Protect blocking.',
'7. Sleep reconstruction respects observed sleep actions and guaranteed wake bounds; Rest/Early Bird and freeze counters have regression coverage.',
'8. Candidate action features and replay labels resolve locked request slot 0 to the executed move and committed target. This encoder change is in Model1-experimental; resolving helpers exist in both Ninjask copies.',
'', 'Validation: 89 passing tests per Ninjask copy, including 24 reconstruction regressions; one frozen-model action-encoder regression passed. The long probability sampler passed earlier in the task and was excluded from the final rerun. No checkpoint retraining or architecture change.',
'', 'Replay protocol: 200 training-pool plus 200 held-out-pool games per pass, seed 781000, 2 matrix samples, 256 regret iterations. Same archived final17h checkpoint and original single-world search. Existing prior Disable/Choice/Unburden/Toxic fixes are in both baseline and fixed runs.',
'Baseline: 274 wins / 126 losses.', *[f"Pass {i['iteration']}: {i['wins']} wins / {i['losses']} losses." for i in iterations],
'', 'Final versus baseline: '+json.dumps(comparison['summary']),
'Choice differences compare underlying requests on identical public histories, not corrected labels. Outcome differences are full matched-setup replays; they are not isolated causal attribution to a single bug.',
'',f"Final sweep: {validation['losses']} losses, {validation['search_positions']} searched positions; {len(validation['observable_errors'])} additional confirmed observable reconstruction/encoding errors in the implemented checks.",
'Unknown opponent charge targets and unrevealed weather-extending items remain hidden-information assumptions. Earlier claims treating omitted opponent charge targets as known public reconstruction errors were too strong.',
'Other state differences were screened with diagnostic-only counterfactuals. Counterfactuals never feed private opponent state to the actual policy. Remaining changed counterfactuals and every checked position are preserved in the comparison/validation JSON.',
'This is a finite audit of these losses, candidate shortlists and one-turn search; it is not proof that the whole engine has no bugs. Policy errors such as immunity attacks or Double Shock without Electric typing can remain with correctly reconstructed state.',
'', 'All outcome flips (pool/game):']
for label,b,a in [('Loss to win',1,0),('Win to loss',0,1)]:
 lines.append(label+': '+', '.join(f"{g['pool']}/{g['game']}" for g in comparison['games'] if g['before_outcome']==b and g['after_outcome']==a))
lines+=['','First changed requests in original losses (full before/after actions):']
for g in comparison['games']:
 if g['before_outcome']==1 and g['first_choice']:lines.append(json.dumps({k:g[k] for k in ['pool','game','before_outcome','after_outcome','first_choice']}))
lines+=['','Timing for these four passes: '+json.dumps(timing), 'Evaluation timing includes inference, matrix simulations, diagnostic simulations and trace writing; initial loading and compilation are outside those timers. Training games: 0 s. Model training: 0 s. Evaluation overhead is recorded separately.','', 'Detailed artifacts: iteration-4-comparison-baseline.json; iteration-4-comparison.json; iteration-4-validation.json; iteration-4-mismatch-screen.json; provenance.json; all preserved trace directories and build snapshots.']
(out/'report.txt').write_text('\n'.join(lines)+'\n')
print(json.dumps(dict(summary=comparison['summary'],validation={k:validation[k] for k in ['games','losses','search_positions','check_counts','observable_errors']},timing=timing),indent=2))
