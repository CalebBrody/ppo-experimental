import json,pathlib,collections,hashlib
root=pathlib.Path(__file__).resolve().parents[1]
suite=root/'Model2/runs/opponent-suite-insample-heldout-20261104'
out=root/'Model2/runs/hp-loss-state-audit-20261104'
report={'scope':'Archived 17-hour checkpoint HP-only suite; outcome 1 means learner loss regardless of learner_side. No new games. State-bug exposure is not proof a corrected game wins.','pools':{},'unburden_own_positions':[],'unburden_species_conditional_positions':[],'toxic_history_positions':[]}
for pool in ['training','heldout']:
 games=[json.loads(f.read_text()) for f in sorted((suite/f'{pool}-hp/traces').glob('*.json'))]
 losses=[g for g in games if g['outcome']==1]
 report['pools'][pool]={'games':len(games),'losses':len(losses)}
 for g in losses:
  consumed={}; toxic={}
  for d in g['decisions']:
   if d.get('search'):
    for rel,side in enumerate(d['state']['sides']):
     abs_side=g['learner_side'] if rel==0 else 1-g['learner_side']
     for m in side:
      key=f"p{abs_side+1}: {m['name']}"
      if m['slot'] is None or m['fainted']:continue
      base={'pool':pool,'game':g['game'],'turn':d['turn'],'decision':d['decision'],'pokemon':key,'species':m['species'],'search_value':d['search']['value']}
      if key in consumed and (m['ability']=='Unburden' or (m['species']=='Sneasler' and m['ability'] is None)):
       dest='unburden_own_positions' if rel==0 and m['ability']=='Unburden' else 'unburden_species_conditional_positions'
       report[dest].append(dict(base,consumption=consumed[key],ability=m['ability']))
      if key in toxic and toxic[key]!=1:
       report['toxic_history_positions'].append(dict(base,ticks_since_entry=toxic[key],reconstructed_stage=1,hp=m['hp'],maxhp=m['maxhp']))
   for e in d['events']:
    parts=e.split('|'); kind=parts[1] if len(parts)>1 else ''; who=parts[2] if len(parts)>2 else ''
    key=who[:2]+': '+who.split(': ',1)[1] if ': ' in who else who
    if kind in ['switch','drag','faint']:
     consumed.pop(key,None);toxic.pop(key,None)
     if kind in ['switch','drag'] and len(parts)>4 and ' tox' in parts[4]:toxic[key]=0
    if kind=='-status' and len(parts)>3 and parts[3]=='tox':toxic[key]=0
    if kind=='-curestatus':toxic.pop(key,None)
    if kind=='-damage' and '[from] psn' in e and key in toxic:toxic[key]+=1
    if kind=='-enditem' and '[weaken]' not in e and '[from] stealeat' not in e:consumed[key]={'turn':d['turn'],'event':e}
    if kind=='-item':consumed.pop(key,None)
for field in ['unburden_own_positions','unburden_species_conditional_positions','toxic_history_positions']:
 report[field+'_counts']={'positions':len(report[field]),'games':len({(x['pool'],x['game']) for x in report[field]})}
out.mkdir(exist_ok=True)
(out/'loss-screen.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({k:v for k,v in report.items() if not isinstance(v,list)},indent=2))
