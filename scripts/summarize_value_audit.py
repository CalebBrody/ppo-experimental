"""Verify online value/winner counters against completed-game raw samples."""
import argparse
import json
from collections import deque
from pathlib import Path
import numpy as np

KEYS=['all_disagreements','hp_behind_value_positive','hp_ahead_value_negative','material_disagreements_hp10pp_value03']
def member(p):
    v,h=p['value'],p['hp_difference']; disagree=v*h<0
    return [disagree,disagree and h<0,disagree and h>0,disagree and abs(h)>=.1 and abs(v)>=.3]
def counts(samples):
    result={}
    for i,key in enumerate(KEYS):
        selected=[p for p in samples if member(p)[i]]
        correct=sum((p['value']>0)==(p['outcome']==0) for p in selected)
        result[key]={'positions':len(selected),'value_correct':correct,'accuracy_percent':100*correct/len(selected) if selected else None}
    return result
def describe(samples):
    result=counts(samples);rng=np.random.default_rng(138)
    for i,key in enumerate(KEYS):
        selected=[p for p in samples if member(p)[i]]
        ids=sorted({p['game'] for p in selected});result[key]['games']=len(ids)
        if not ids:continue
        totals=np.array([[sum((p['value']>0)==(p['outcome']==0) for p in selected if p['game']==g),sum(p['game']==g for p in selected)] for g in ids])
        boot=totals[rng.integers(len(ids),size=(4000,len(ids)))].sum(axis=1)
        result[key]['bootstrap95_percent']=np.quantile(100*boot[:,0]/boot[:,1],[.025,.975]).tolist()
    return result
def main():
    ap=argparse.ArgumentParser(description=__doc__);ap.add_argument('run',type=Path);args=ap.parse_args()
    metrics=[json.loads(s) for s in (args.run/'training/metrics.jsonl').read_text().splitlines()]
    games=[json.loads(s) for s in (args.run/'training/value-outcome-samples.jsonl').read_text().splitlines()]
    pending={}
    for g in games:pending.setdefault(g['update'],[]).append(g)
    window=deque(maxlen=metrics[-1]['value_outcome_audit']['window_capacity'])
    first=None; first_time=None
    for m in metrics:
        for g in pending.get(m['update'],[]):
            if g['outcome']==2:continue
            for p in g['predictions']:window.append(dict(p,game=g['game'],outcome=g['outcome']))
        expected=counts(window);actual=m['value_outcome_audit']
        assert actual['completed_positions']==len(window),(m['update'],'window size')
        for key in KEYS:
            for field in ['positions','value_correct','accuracy_percent']:
                a,b=expected[key][field],actual[key][field]
                assert a==b or (a is not None and b is not None and abs(a-b)<1e-5),(m['update'],key,field,a,b)
        if first is None and len(window)==window.maxlen:
            first=list(window);first_time=m['seconds']
    last=metrics[-1]
    result={'verified_metric_rows':len(metrics),'first_full_window_seconds':first_time,
            'first_full_window':describe(first) if first else None,'final_window':describe(list(window)),
            'extra_value_inference_seconds':last['value_outcome_audit']['measured_seconds'],
            'extra_value_inference_fraction_percent':100*last['value_outcome_audit']['measured_seconds']/last['seconds'],
            'method':'Counter reconstruction from completed-game records. First/full and final rolling windows; game-cluster bootstrap intervals. Changing training opponents and possibly overlapping windows; descriptive, not a fixed-distribution causal test.'}
    (args.run/'value-audit-summary.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(result,indent=2))
if __name__=='__main__':main()
