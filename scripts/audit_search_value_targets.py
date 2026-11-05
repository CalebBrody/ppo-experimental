"""Inspect archived Model2 search targets and outcome-labelled value samples."""
import argparse
import json
from pathlib import Path
import numpy as np

p=argparse.ArgumentParser(description=__doc__);p.add_argument('run',type=Path);args=p.parse_args()
root=args.run/'training'
witnesses=[json.loads(s) for s in (root/'mistake-examples.jsonl').read_text().splitlines()]
games={g['game']:g for g in map(json.loads,(root/'value-outcome-samples.jsonl').read_text().splitlines())}
metrics=[json.loads(s) for s in (root/'metrics.jsonl').read_text().splitlines()]
examples=[];mismatches=[]
for w in witnesses:
    s=w['search'];matrix=np.asarray(s['payoff']);policy=np.asarray(s['policy']);q=np.asarray(s['opponent_policy'])
    expected=float(policy@matrix@q)
    if abs(expected-s['value'])>1e-7:mismatches.append(w['decision'])
    if w['game'] not in [312,325,1101,333]:continue
    i=s['chosen_candidate']
    examples.append(dict(game=w['game'],turn=s['turn'],update=w['update'],outcome=w['game_outcome'],
        target=s['value'],duality_gap=s['duality_gap'],chosen_probability=policy[i],
        chosen_payoff_min=float(matrix[i].min()),chosen_payoff_max=float(matrix[i].max()),
        chosen_actions=s['candidates'][i],state=s['state'],events=w['events'],
        sampled_network_predictions_same_game=games[w['game']]['predictions']))
decisive=[w for w in witnesses if w['game_outcome'] in [0,1]]
terminal=[w for w in decisive if any(s.startswith('|win|') for s in w['events'])]
predictions=[(g['outcome'],v) for g in games.values() if g['outcome'] in [0,1] for v in g['predictions']]
config=json.loads((root/'config.json').read_text())
result=dict(witness_positions=len(witnesses),decisive_witness_positions=len(decisive),
    target_sign_disagrees_with_realized_winner=sum((w['search']['value']>0)!=(w['game_outcome']==0) for w in decisive),
    terminal_witness_positions=len(terminal),terminal_wrong_target_signs=sum((w['search']['value']>0)!=(w['game_outcome']==0) for w in terminal),
    matrix_expectation_mismatches=mismatches,
    mean_training_value_half_mse=float(np.mean([m['value_loss'] for m in metrics])),
    mean_matrix_duality_gap=float(np.mean([w['search']['duality_gap'] for w in decisive])),
    sampled_predictions=len(predictions),sampled_wrong_sign=sum((v['value']>0)!=(o==0) for o,v in predictions),
    learning_rate=3e-4*max(.05,1-config['elapsed_offset_seconds']/config['anneal_seconds']),examples=examples,
    limitations=[
        'Mistake witnesses are deliberately selected, not representative; a realized loss alone does not prove an expected value wrong.',
        'Original raw value samples omit turn/state/search target. Same-game raw predictions cannot be paired to an exact witness position.',
        'Search logs combine immediate reward and bootstrap; no original per-cell decomposition or opponent candidate descriptions exist.',
        'Small matrix duality gaps show local solve convergence, not accurate payoff estimates or complete action coverage.',
        'Actual completed-game outcomes are not used as Model2 value targets; value loss fits bootstrapped matrix values.'
    ])
(args.run/'search-value-target-audit.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps({k:v for k,v in result.items() if k!='examples'},indent=2))
