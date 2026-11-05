"""Inventory field mismatches in the corrected frozen suite's losses."""
from pathlib import Path
from collections import Counter,defaultdict
import json
root=Path(__file__).resolve().parents[1]
out=root/'Model2/runs/all-loss-reconstruction-audit-20261105'
counts=Counter(); examples=defaultdict(list); losses=positions=games=0

def flat(x,p=''):
 for k,v in x.items():
  key=p+k
  if isinstance(v,dict):yield from flat(v,key+'.')
  else:yield key,v
for pool in ['training','heldout']:
 for path in sorted((out/f'audit-{pool}/traces').glob('*.json')):
  try:g=json.loads(path.read_text())
  except json.JSONDecodeError:continue
  games+=1
  if g['outcome']!=1:continue
  losses+=1
  for d in g['decisions']:
   s=d.get('search')
   if not s:continue
   positions+=1
   w,a=s['full_world'],s['full_actual'];side=g['learner_side']
   def record(scope,x,y,name=''):
    xx=dict(flat(x));yy=dict(flat(y))
    for k in xx:
     if xx[k]!=yy.get(k):
      key=scope+'.'+k;counts[key]+=1
      if len(examples[key])<30:examples[key].append(dict(pool=pool,game=g['game'],turn=d['turn'],decision=d['decision'],pokemon=name,reconstructed=xx[k],actual=yy.get(k)))
   record('field',w['field'],a['field'])
   for si in range(2):
    rel='own' if si==side else 'foe'
    record(rel+'.conditions',w['sides'][si]['conditions'],a['sides'][si]['conditions'])
    am={m['name']:m for m in a['sides'][si]['mons']}
    for m in w['sides'][si]['mons']:
     if m['name'] not in am or m['is_active']!='true' or m['fainted']=='true':continue
     record(rel,m,am[m['name']],m['name'])
report=dict(games=games,losses=losses,positions=positions,counts=dict(counts.most_common()),examples=examples)
(out/'mismatch-screen.json').write_text(json.dumps(report,indent=2))
print('games',games,'losses',losses,'positions',positions)
for k,n in counts.most_common():print(k,n, str(examples[k][0])[:230])
