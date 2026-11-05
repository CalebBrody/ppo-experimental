"""Find paired immunity/Close Combat action rows in decomposed diagnostics."""
import argparse,json
from pathlib import Path
import numpy as np
p=argparse.ArgumentParser();p.add_argument('directory',type=Path);args=p.parse_args()
results=[];decisions=0;errors=[]
for line in (args.directory/'diagnostic-games.jsonl').open():
    game=json.loads(line)
    for d in game['decisions']:
        s=d.get('search')
        if not s or 'immediate_payoff' not in s:continue
        decisions+=1;q=np.asarray(s['opponent_policy']);m=np.asarray(s['payoff']);r=np.asarray(s['immediate_payoff']);b=np.asarray(s['bootstrap_payoff'])
        if not np.allclose(np.clip(r+b,-2,2),m,atol=1e-7):errors.append([game['game'],d['turn']])
        values=m@q;rewards=r@q;boots=b@q
        for i,a in enumerate(s['candidates']):
            for actor,move in enumerate(a):
                if move.get('move') not in ['Dire Claw','Gunk Shot']:continue
                target=move.get('target',0)
                foe=next((mon for mon in s['state']['sides'][1] if mon['slot']==target-1),None) if target>0 else None
                if not foe or 'Steel' not in foe['types']:continue
                for j,c in enumerate(s['candidates']):
                    if len(a)!=len(c) or c[actor].get('move')!='Close Combat' or c[actor].get('target')!=target:continue
                    if any(a[k]!=c[k] for k in range(len(a)) if k!=actor):continue
                    results.append(dict(game=game['game'],turn=d['turn'],outcome=game['outcome'],actor=move['actor'],target=foe['species'],
                        chosen_candidate=s['chosen_candidate'],immunity_candidate=i,combat_candidate=j,
                        immediate_reward_better_for_combat=bool(rewards[j]>rewards[i]),total_prefers_immunity=bool(values[i]>values[j]),
                        immunity=dict(move=move['move'],reward=float(rewards[i]),bootstrap=float(boots[i]),total=float(values[i]),probability=s['policy'][i]),
                        close_combat=dict(reward=float(rewards[j]),bootstrap=float(boots[j]),total=float(values[j]),probability=s['policy'][j]),
                        state=s['state'],events=d['events'],partner=[x for k,x in enumerate(a) if k!=actor]))
results.sort(key=lambda x:(x['immediate_reward_better_for_combat'] and x['total_prefers_immunity'],x['chosen_candidate']==x['immunity_candidate'],x['immunity']['total']-x['close_combat']['total']),reverse=True)
result=dict(searched_positions=decisions,decomposition_errors=errors,paired_positions=len(results),
            combat_reward_better_but_total_worse=sum(x['immediate_reward_better_for_combat'] and x['total_prefers_immunity'] for x in results),examples=results)
(args.directory/'payoff-decomposition-analysis.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps({k:v for k,v in result.items() if k!='examples'},indent=2))
for x in results[:5]:print(json.dumps({k:v for k,v in x.items() if k not in ['state','events','partner']},indent=2))
