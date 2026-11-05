import argparse,json
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--iteration',type=int,default=1);p.add_argument('--baseline',action='store_true');args=p.parse_args()
root=Path(__file__).resolve().parents[1];out=root/'Model2/runs/reconstruction-fix-loop-20261105';old=root/'Model2/runs/all-loss-reconstruction-audit-20261105'
def request(c):
 if isinstance(c,list): return [request(x) for x in c]
 if not isinstance(c,dict): return c
 if 'move_slot' in c: return {k:c.get(k) for k in ['actor_slot','move_slot','mega']} | {'target':c.get('requested_target',c.get('target'))}
 return {k:request(v) for k,v in c.items() if k not in ['actor','species','name']}
rows=[];diagnostics=[]
for pool in ['training','heldout']:
 for f in sorted((out/f'iteration-{args.iteration}-{pool}/traces').glob('*.json')):
  try:b=json.loads(f.read_text())
  except json.JSONDecodeError:continue
  previous=old/f'audit-{pool}/traces'/f.name if args.iteration==1 or args.baseline else out/f'iteration-{args.iteration-1}-{pool}/traces'/f.name
  a=json.loads(previous.read_text());first_search=first_choice=None;same_history=True;matched=changed_value=0
  for x,y in zip(a['decisions'],b['decisions']):
   if not same_history or x['decision']!=y['decision'] or x['turn']!=y['turn']:break
   if x.get('search') and y.get('search'):
    matched+=1
    if x['search']['value']!=y['search']['value']:changed_value+=1
    if first_search is None and (any(x['search'][k]!=y['search'][k] for k in ['payoff','policy','opponent_policy','value']) or any(request(x['search'][k])!=request(y['search'][k]) for k in ['candidates','opponent_candidates'])):
     first_search=dict(decision=x['decision'],turn=x['turn'],before_value=x['search']['value'],after_value=y['search']['value'])
   if request(x.get('selected'))!=request(y.get('selected')) and first_choice is None:
    first_choice=dict(decision=x['decision'],turn=x['turn'],before=x.get('selected'),after=y.get('selected'))
   same_history=x['events']==y['events']
  rows.append(dict(pool=pool,game=b['game'],before_outcome=a['outcome'],after_outcome=b['outcome'],first_search=first_search,first_choice=first_choice,matched_positions=matched,changed_value_positions=changed_value))
  if b['outcome']==1:
   for d in b['decisions']:
    for r in (d.get('search') or {}).get('causal_repairs',[]):
     diagnostics.append(dict(pool=pool,game=b['game'],decision=d['decision'],turn=d['turn'],mode=r['mode'],features_changed=r['features_changed'],rewards_changed=r['rewards_changed'],value_delta=r['value_delta']))
summary=dict(games=len(rows),before_losses=sum(r['before_outcome']==1 for r in rows),after_losses=sum(r['after_outcome']==1 for r in rows),
 loss_to_win=sum(r['before_outcome']==1 and r['after_outcome']==0 for r in rows),win_to_loss=sum(r['before_outcome']==0 and r['after_outcome']==1 for r in rows),
 old_losses_search_changed=sum(r['before_outcome']==1 and r['first_search'] is not None for r in rows),old_losses_choice_changed=sum(r['before_outcome']==1 and r['first_choice'] is not None for r in rows))
remaining={}
for mode in sorted({r['mode'] for r in diagnostics}):
 rs=[r for r in diagnostics if r['mode']==mode];changed=[r for r in rs if r['features_changed'] or r['rewards_changed']]
 remaining[mode]=dict(exposed=len(rs),changed_positions=len(changed),value_changed=sum(abs(r['value_delta'])>1e-6 for r in rs),examples=changed[:5])
report=dict(iteration=args.iteration,summary=summary,remaining_counterfactuals=remaining,games=rows,remaining_positions=diagnostics)
(out/f'iteration-{args.iteration}-comparison{"-baseline" if args.baseline else ""}.json').write_text(json.dumps(report,indent=2))
print(json.dumps(dict(summary=summary,remaining_counterfactuals=remaining),indent=2))
