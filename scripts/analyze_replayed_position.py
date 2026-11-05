"""Verify the game-325 replay against its archived witness, then decompose it."""
import json
from pathlib import Path
import numpy as np

original=Path('Model2/runs/value-disagreements-20m-20261103')
replay=Path('reports/replay-game325-turn16-20261103')
w=next(json.loads(s) for s in (original/'training/mistake-examples.jsonl').read_text().splitlines()
       if json.loads(s)['game']==325 and json.loads(s)['search']['turn']==16)
got=json.loads((replay/'training/inspected-decision.json').read_text());s=got['search'];old=w['search']
checks={k:got[k]==w[k] for k in ['game','env','update','decision']}
for k in ['turn','side','state','candidates','payoff','policy','opponent_policy','value','duality_gap','chosen_candidate']:
    checks['search_'+k]=s[k]==old[k]
before=[json.loads(x) for x in (original/'training/metrics.jsonl').read_text().splitlines()]
after=[json.loads(x) for x in (replay/'training/metrics.jsonl').read_text().splitlines()]
fields=['games','decisions','wins','losses','draws','policy_loss','value_loss','entropy','kl','mean_matrix_duality_gap']
prefix_mismatches=[(a['update'],k) for a,b in zip(before,after) for k in fields if a.get(k)!=b.get(k)]
checks['prefix_metrics']=not prefix_mismatches
q=np.asarray(s['opponent_policy']);r=np.asarray(s['immediate_payoff']);v=np.asarray(s['bootstrap_payoff']);m=np.asarray(s['payoff'])
checks['component_sum']=bool(np.allclose(np.clip(r+v,-2,2),m,atol=1e-8))
rows=[]
for i in [4,6]:
    rows.append(dict(candidate=i,actions=s['candidates'][i],immediate_reward=float(r[i]@q),future_value=float(v[i]@q),
                     total=float(m[i]@q),policy_probability=s['policy'][i]))
columns=[]
for j,weight in enumerate(q):
    if weight<.005:continue
    columns.append(dict(column=j,probability=float(weight),opponent_actions=s['opponent_candidates'][j],
        dire_claw=dict(reward=r[4,j],future=v[4,j],total=m[4,j]),
        close_combat=dict(reward=r[6,j],future=v[6,j],total=m[6,j]),
        traces=[x for x in s['cell_traces'] if x['row'] in [4,6] and x['column']==j]))
report=dict(exact_match_checks=checks,all_checks_pass=all(checks.values()),replayed_complete_updates=len(after),
            prefix_mismatches=prefix_mismatches,rows=rows,opponent_columns=columns,
            tracker_debug=got['tracker_debug'],timing={k:v for k,v in got.items() if k.endswith('_seconds') or k.endswith('_update')})
(replay/'position-analysis.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({k:v for k,v in report.items() if k not in ['tracker_debug','opponent_columns']},indent=2))
for c in columns:
    print('\nCOLUMN',c['column'],'PROBABILITY',c['probability'],'OPPONENT',c['opponent_actions'])
    print('Dire Claw',c['dire_claw'],'Close Combat',c['close_combat'])
    for t in c['traces']:print('ROW',t['row'],'reward',t['reward'],'events',t['events'])
assert all(checks.values()), 'Replay did not reproduce the original position; do not present as exact.'
