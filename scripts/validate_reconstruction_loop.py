"""Audit observable mechanics separately from unknown opponent information."""
import argparse,json,collections,re
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--iteration',type=int,default=3);args=p.parse_args()
root=Path(__file__).resolve().parents[1];out=root/'Model2/runs/reconstruction-fix-loop-20261105'
dex=json.loads((root/'Model2/runs/opponent-suite-insample-heldout-20261104/dex.json').read_text())
errors=[];hidden=[];weather=[];counts=collections.Counter();positions=losses=games=0
for pool in ['training','heldout']:
 for path in sorted((out/f'iteration-{args.iteration}-{pool}/traces').glob('*.json')):
  try:g=json.loads(path.read_text())
  except json.JSONDecodeError:continue
  games+=1
  if g['outcome']!=1:continue
  losses+=1
  for d in g['decisions']:
   s=d.get('search')
   if not s:continue
   positions+=1;side=g['learner_side'];ctx=dict(pool=pool,game=g['game'],decision=d['decision'],turn=d['turn'])
   if args.iteration>=4:
    for key,si in [('candidates',side),('opponent_candidates',1-side)]:
     for candidate in s[key]:
      if not isinstance(candidate,list):continue
      for action in candidate:
       if 'move_slot' not in action:continue
       m=s['full_world']['sides'][si]['mons'][action['actor_slot']];v=m['vol']
       lock=next((v[k] for k in ['two_turn','locked_move'] if v[k]!='None'),None)
       if not lock:continue
       counts['locked_action_encoding']+=1
       ident=int(re.search(r'\d+',lock).group());expected=dex['moves'][ident]['name']
       if action['move']!=expected:errors.append(dict(ctx,field='locked_action_identity',world=action['move'],actual=expected))
       if v['charging']!='None' and int(v['charging_target_loc'])!=0 and action['target']!=int(v['charging_target_loc']):errors.append(dict(ctx,field='locked_action_target',world=action['target'],actual=v['charging_target_loc']))
   for si in range(2):
    am={m['name']:m for m in s['full_actual']['sides'][si]['mons']}
    tm={json.dumps(m['name']):m for m in s['full_tracker']['sides'][si]['pokemon']}
    for m in s['full_world']['sides'][si]['mons']:
     a=am.get(m['name']);t=tm.get(m['name'])
     if not a or not t or m['is_active']!='true' or m['fainted']=='true':continue
     if si==side:
      for field in ['points','nature_plus','nature_minus','stats','hp','maxhp','ability','item','types','boosts']:
       counts['own_'+field]+=1
       if m[field]!=a[field]:errors.append(dict(ctx,pokemon=m['name'],field=field,world=m[field],actual=a[field]))
     if si==side:
      for field in ['unburden','choice_lock','disable','encore']:
       counts['own_volatile_'+field]+=1
       if m['vol'][field]!=a['vol'][field]:errors.append(dict(ctx,pokemon=m['name'],field=field,world=m['vol'][field],actual=a['vol'][field]))
      if t.get('status')=='tox':
       counts['own_toxic_stage']+=1
       if m['toxic_stage']!=a['toxic_stage']:errors.append(dict(ctx,pokemon=m['name'],field='toxic_stage',world=m['toxic_stage'],actual=a['toxic_stage']))
     if m['ability']==a['ability']:
      for field in ['protean','libero','unnerved']:
       # None and false both mean inactive. Check the semantic activation flag.
       counts['activation_'+field]+=1
       if (m['ability_state'][field]=='Some(true)')!=(a['ability_state'][field]=='Some(true)'):
        errors.append(dict(ctx,pokemon=m['name'],field=field,world=m['ability_state'][field],actual=a['ability_state'][field]))
     if m['vol']['two_turn']!='None':
      for field in ['two_turn','charging','two_turn_duration','charging_duration','charging_target_loc']:
       known=field!='charging_target_loc' or si==side or (t.get('charging') and t['charging'][1]!=0)
       if known:
        counts['charge_'+field]+=1
        if m['vol'][field]!=a['vol'][field]:errors.append(dict(ctx,pokemon=m['name'],field=field,world=m['vol'][field],actual=a['vol'][field]))
       elif m['vol'][field]!=a['vol'][field]:hidden.append(dict(ctx,pokemon=m['name'],field=field,reason='opponent committed target omitted from public log'))
     if t.get('status')=='slp' and t.get('statusActions',0)>=2:
      counts['guaranteed_wake']+=1
      if m['status_time']!='1':errors.append(dict(ctx,pokemon=m['name'],field='sleep_bound',world=m['status_time'],actual=a['status_time']))
   if s['full_world']['field']['weather_turns']!=s['full_actual']['field']['weather_turns']:
    source=None;last_move=None
    for prev in g['decisions']:
     if prev['decision']>=d['decision']:break
     for line in prev['events']:
      parts=line.split('|')
      if len(parts)>2 and parts[1]=='move':last_move=parts[2]
      if len(parts)>2 and parts[1]=='-weather' and '[upkeep]' not in line:
       source=next((x[len('[of] '):] for x in parts if x.startswith('[of] ')),last_move)
    entry=dict(ctx,source=source,world=s['full_world']['field']['weather_turns'],actual=s['full_actual']['field']['weather_turns'])
    weather.append(entry)
    if source and source.startswith('p'+str(side+1)):
     errors.append(dict(entry,field='own_setter_weather'))
   for r in s['causal_repairs']:
    if r['features_changed'] or r['rewards_changed']:
     if r['mode'] not in ['charge','field_timers']:
      errors.append(dict(ctx,field='causal_'+r['mode'],value_delta=r['value_delta'],features_changed=r['features_changed'],rewards_changed=r['rewards_changed']))
report=dict(games=games,losses=losses,search_positions=positions,check_counts=dict(counts),observable_errors=errors,hidden_target_mismatches=hidden,weather_mismatches=weather)
(out/f'iteration-{args.iteration}-validation.json').write_text(json.dumps(report,indent=2))
print(json.dumps(report,indent=2))
