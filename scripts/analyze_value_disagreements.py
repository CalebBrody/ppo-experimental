"""Inspect value-versus-HP disagreements in existing fixed-weight game traces."""
import json
from pathlib import Path
import numpy as np
from analyze_value_quality import auc

root=Path('reports/value-outcome-diagnostics-20261103')
games=[json.loads(s) for s in (root/'diagnostic-games.jsonl').read_text().splitlines()]
rows=[]
for g in games:
    if g['outcome']==2: continue
    for d in g['decisions']:
        if d['request']!='Move' or 'network_value' not in d:continue
        rows.append(dict(game=g['game'],turn=d['turn'],v=d['network_value'],hp=2*d['hp_potential'],win=int(g['outcome']==0),decision=d))
rng=np.random.default_rng(481001)
def summary(rs,key='v'):
    ids=sorted({r['game'] for r in rs})
    if not ids:return None
    totals=np.array([[sum((r[key]>0)==r['win'] for r in rs if r['game']==g),sum(r['game']==g for r in rs)] for g in ids])
    boot=totals[rng.integers(len(ids),size=(4000,len(ids)))].sum(axis=1)
    return dict(positions=len(rs),games=len(ids),value_correct=int(totals[:,0].sum()),value_accuracy=float(totals[:,0].sum()/totals[:,1].sum()),bootstrap95=np.quantile(boot[:,0]/boot[:,1],[.025,.975]).tolist())
for r in rows:r['adjusted']=r['v']+r['hp']/2
results={}
for key in ['v','adjusted']:
    disagree=[r for r in rows if r['hp']*r[key]<0]
    results[key]={
        'all_disagreements':summary(disagree,key),
        'hp_behind_value_positive':summary([r for r in disagree if r['hp']<0],key),
        'hp_ahead_value_negative':summary([r for r in disagree if r['hp']>0],key),
        'material_disagreements_hp10pp_value03':summary([r for r in disagree if abs(r['hp'])>=.1 and abs(r[key])>=.3],key)}
# Compare rankings among positions with approximately equal HP advantage.
bins=[]
for lo in np.arange(-.8,.8001,.1):
    rs=[r for r in rows if lo-1e-8<=r['hp']<lo+.1-1e-8]
    if not rs:continue
    bins.append(dict(hp_low=round(float(lo),2),n=len(rs),games=len({r['game'] for r in rs}),value_auc=auc([r['v'] for r in rs],[r['win'] for r in rs]),hp_auc=auc([r['hp'] for r in rs],[r['win'] for r in rs])))
results['similar_hp_bins']=bins
# Game-separated cross validation: simple logistic baselines, with nonlinear HP
# and turn terms in BOTH models. No same-game positions across train/test.
ids=np.array(sorted({r['game'] for r in rows}));rng.shuffle(ids)
y=np.array([r['win'] for r in rows]);h=np.array([r['hp'] for r in rows]);v=np.array([r['v'] for r in rows]);t=np.array([r['turn']/10 for r in rows])
base=np.column_stack([np.ones(len(rows)),h,h*h,h*h*h,t,t*t,h*t])
preds={}
for name,x in [('hp_and_turn',base),('hp_turn_and_value',np.column_stack([base,v,v*v,v*h]))]:
    out=np.zeros(len(rows))
    for fold in np.array_split(ids,5):
        test=np.array([r['game'] in fold for r in rows]);train=~test
        mu=x[train].mean(axis=0);sd=x[train].std(axis=0);mu[0]=0;sd[sd<1e-8]=1
        z=(x-mu)/sd;w=np.zeros(x.shape[1])
        penalty=np.eye(len(w))*.01;penalty[0,0]=0
        for _ in range(60):
            p=1/(1+np.exp(-np.clip(z[train]@w,-30,30)))
            grad=z[train].T@(p-y[train])/train.sum()+penalty@w
            hes=(z[train].T*(p*(1-p)))@z[train]/train.sum()+penalty+np.eye(len(w))*1e-7
            step=np.linalg.solve(hes,grad);w-=step
            if np.max(np.abs(step))<1e-7:break
        out[test]=1/(1+np.exp(-np.clip(z[test]@w,-30,30)))
    preds[name]=out
    results[name]=dict(auc=auc(out,y),accuracy=float(((out>.5)==y).mean()),brier=float(((out-y)**2).mean()),logloss=float(-(y*np.log(out)+(1-y)*np.log(1-out)).mean()))
improvement=(preds['hp_and_turn']-y)**2-(preds['hp_turn_and_value']-y)**2
totals=np.array([[sum(improvement[i] for i,r in enumerate(rows) if r['game']==g),sum(r['game']==g for r in rows)] for g in ids])
boot=totals[rng.integers(len(ids),size=(4000,len(ids)))].sum(axis=1)
results['brier_improvement_game_bootstrap95']=np.quantile(boot[:,0]/boot[:,1],[.025,.975]).tolist()
examples=[]
for correct in [True,False]:
    candidates=[r for r in rows if r['v']*r['hp']<0 and abs(r['hp'])>=.1 and abs(r['v'])>=.3 and ((r['v']>0)==r['win'])==correct]
    candidates.sort(key=lambda r:abs(r['hp']*r['v']),reverse=True)
    seen=set()
    for r in candidates:
        if r['game'] in seen:continue
        seen.add(r['game']);d=r['decision']
        examples.append(dict(game=r['game'],turn=r['turn'],value=r['v'],hp_difference=r['hp'],win=r['win'],value_correct=correct,state=d['state'],selected=d['selected']))
        if len(seen)>=3:break
results['examples']=examples
results['method']='Fixed checkpoint self-play; decision-weighted; disagreement intervals resample games. HP labels use engine actual brought teams; value sees Tracker only. Logistic comparison uses identical shuffled five game folds, same HP polynomial and turn terms, adds value features; exploratory diagnostic, not an independent final test.'
(root/'value-disagreements.json').write_text(json.dumps(results,indent=2)+'\n')
print(json.dumps({k:v for k,v in results.items() if k not in ['examples','similar_hp_bins']},indent=2))
for r in examples:
    print('\nEXAMPLE', {k:v for k,v in r.items() if k not in ['state','selected']})
    for side in r['state']['sides']:
        print([(p['species'],f"{p['hp']}/{p['maxhp']}",p['slot'],p['volatiles']) for p in side])
