from pathlib import Path
import json,collections,hashlib
root=Path(__file__).resolve().parents[1];out=root/'Model2/runs/all-loss-reconstruction-audit-20261105'
rows=[];verified=0;losses=positions=simulations=0
for pool in ['training','heldout']:
 for p in sorted((out/f'causal-{pool}/traces').glob('*.json')):
  try:g=json.loads(p.read_text())
  except json.JSONDecodeError:continue
  old=json.loads((out/f'audit-{pool}/traces'/p.name).read_text())
  assert g['outcome']==old['outcome'] and len(g['decisions'])==len(old['decisions'])
  for d,e in zip(g['decisions'],old['decisions']):
   assert d['events']==e['events'] and d.get('selected')==e.get('selected')
   if d.get('search'):
    for k in ['payoff','policy','opponent_policy','value','candidates','opponent_candidates','chosen_candidate']:
     assert d['search'][k]==e['search'][k],(pool,g['game'],d['decision'],k)
    simulations+=sum(r['simulations'] for r in d['search']['causal_repairs'])
  verified+=1
  if g['outcome']!=1:continue
  losses+=1
  for d in g['decisions']:
   if not d.get('search'):continue
   positions+=1
   for r in d['search']['causal_repairs']:
    row={k:v for k,v in r.items() if k not in ['payoff','example','before_policy','after_policy','opponent_policy']}
    row.update(pool=pool,game=g['game'],turn=d['turn'],decision=d['decision'])
    row['policy_changed']=r['before_policy']!=r['after_policy']
    row['top_action_changed']=max(range(len(r['before_policy'])),key=r['before_policy'].__getitem__)!=max(range(len(r['after_policy'])),key=r['after_policy'].__getitem__)
    row['evidence']=str(p.relative_to(root))
    rows.append(row)
summary={}
for mode in sorted({r['mode'] for r in rows}):
 rs=[r for r in rows if r['mode']==mode];changed=[r for r in rs if r['features_changed'] or r['rewards_changed']]
 summary[mode]=dict(exposed_positions=len(rs),changed_simulation_positions=len(changed),changed_games=len({(r['pool'],r['game']) for r in changed}),
 value_changed_positions=sum(abs(r['value_delta'])>1e-6 for r in rs),top_action_changed_positions=sum(r['top_action_changed'] for r in rs),
 max_absolute_value_change=max(abs(r['value_delta']) for r in rs),examples=sorted(changed,key=lambda r:abs(r['value_delta']),reverse=True)[:3])
report=dict(verified_games=verified,losses=losses,positions=positions,additional_simulations=simulations,summary=summary,positions_detail=rows)
(out/'causal-summary.json').write_text(json.dumps(report,indent=2))
print('verified',verified,'losses',losses,'positions',positions,'extra simulations',simulations)
for mode,s in summary.items():print(mode,{k:v for k,v in s.items() if k!='examples'},'EXAMPLE',s['examples'][:1])
