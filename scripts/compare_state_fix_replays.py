"""Compare archived, Disable-fixed control, and three-fix replays without conflating them."""
import json,pathlib,hashlib,collections
root=pathlib.Path(__file__).resolve().parents[1];suite=root/'Model2/runs/opponent-suite-insample-heldout-20261104';out=root/'Model2/runs/state-fixes-replay-20261104'
screen=json.loads((root/'Model2/runs/hp-loss-state-audit-20261104/loss-screen.json').read_text())
confirmed={(x['pool'],x['game']) for k in ['unburden_own_positions','toxic_history_positions'] for x in screen[k]}
def actual_choice(v):
 if isinstance(v,list):return [{k:a[k] for k in ['actor','move','move_slot','target','mega','switch_index','switch','pass'] if k in a} for a in v]
 return v
def first_change(a,b,field):
 for x,y in zip(a['decisions'],b['decisions']):
  left,right=x.get(field),y.get(field)
  if field=='selected':left,right=actual_choice(left),actual_choice(right)
  if left!=right:return {'decision':x['decision'],'control_turn':x['turn'],'fixed_turn':y['turn'],'before':x.get(field),'after':y.get(field)}
 if len(a['decisions'])!=len(b['decisions']):return {'length_before':len(a['decisions']),'length_after':len(b['decisions'])}
 return None
def compact_search(a,b):
 for x,y in zip(a['decisions'],b['decisions']):
  if x.get('search')!=y.get('search'):
   s=x.get('search') or {};t=y.get('search') or {}
   return {'decision':x['decision'],'control_turn':x['turn'],'fixed_turn':y['turn'],'value_before':s.get('value'),'value_after':t.get('value'),'changed_fields':[k for k in set(s)|set(t) if s.get(k)!=t.get(k)]}
 return None
rows=[];pools=[]
for pool in ['training','heldout']:
 stats={phase:json.loads((folder/'evaluation.json').read_text()) for phase,folder in [('archive',suite/f'{pool}-hp'),('control',out/f'control-{pool}'),('fixed',out/f'fixed-{pool}')]}
 for phase in ['control','fixed']:
  r=stats[phase];assert all(n<=positions*128 for n,positions in zip(r['search_simulations'],r['search_positions'])),(pool,phase,'unexpected search budget')
 pools.append({'pool':pool,'results':{phase:{k:r.get(k) for k in ['wins','losses','draws','search_requests','search_positions','search_fallbacks','search_simulations','evaluation_games_seconds']} for phase,r in stats.items()}})
 for game in range(200):
  filename=f'game-{game:04}.json'
  old=json.loads((suite/f'{pool}-hp/traces'/filename).read_text());ctl=json.loads((out/f'control-{pool}/traces'/filename).read_text());fix=json.loads((out/f'fixed-{pool}/traces'/filename).read_text())
  for key in ['game','match_seed','learner_side','team_a','team_b']:assert old[key]==ctl[key]==fix[key],(pool,game,key)
  row={'pool':pool,'game':game,'match_seed':old['match_seed'],'learner_side':old['learner_side'],'archived_outcome':old['outcome'],'control_outcome':ctl['outcome'],'fixed_outcome':fix['outcome'],'confirmed_prior_exposure':(pool,game) in confirmed,'archive_control_exact_match':old==ctl,'first_search_change':compact_search(ctl,fix),'first_choice_change':first_change(ctl,fix,'selected'),'first_events_change':first_change(ctl,fix,'events'),'control_last_turn':ctl['decisions'][-1]['turn'],'fixed_last_turn':fix['decisions'][-1]['turn']}
  rows.append(row)
def summary(rs):
 return {'games':len(rs),'changed_search':sum(r['first_search_change'] is not None for r in rs),'changed_choices':sum(r['first_choice_change'] is not None for r in rs),'changed_outcomes':sum(r['control_outcome']!=r['fixed_outcome'] for r in rs),'loss_to_win':sum(r['control_outcome']==1 and r['fixed_outcome']==0 for r in rs),'win_to_loss':sum(r['control_outcome']==0 and r['fixed_outcome']==1 for r in rs),'fixed_wins':sum(r['fixed_outcome']==0 for r in rs),'fixed_losses':sum(r['fixed_outcome']==1 for r in rs),'fixed_draws':sum(r['fixed_outcome']==2 for r in rs)}
report={'scope':'Same frozen 17-hour weights, teams, match seeds and budgets; both players receive fixes. Control already includes Disable fix. No training. All outcomes learner-relative: 0 win, 1 loss, 2 draw. Combined-fix replays do not isolate causal contributions of individual fixes.','pools':pools,'summary':summary(rows),'archived_losses':summary([r for r in rows if r['archived_outcome']==1]),'control_losses':summary([r for r in rows if r['control_outcome']==1]),'confirmed_prior_exposure':summary([r for r in rows if r['confirmed_prior_exposure']]),'archive_control_exact_matches':sum(r['archive_control_exact_match'] for r in rows),'games':rows}
(out/'comparison.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({k:v for k,v in report.items() if k!='games'},indent=2))
