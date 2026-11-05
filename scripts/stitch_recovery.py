"""Create a provenance-backed view of retained training across one interruption.

Original runs stay untouched. Discarded unsaved updates are recorded as wasted
compute, never represented as part of the resumed weight trajectory.
"""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import shutil


def read(path):
    return json.loads(path.read_text())


def lines(path):
    return [json.loads(s) for s in path.read_text().splitlines()]


def write(path, value):
    path.write_text(json.dumps(value, indent=2)+'\n')


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('first',type=Path);p.add_argument('second',type=Path)
    p.add_argument('--checkpoint-update',type=int,required=True)
    p.add_argument('--out',type=Path,required=True)
    args=p.parse_args()
    if args.out.exists():
        p.error('Choose a new combined output directory')
    a=lines(args.first/'training/metrics.jsonl')
    b=lines(args.second/'training/metrics.jsonl')
    first=[r for r in a if r['update']<=args.checkpoint_update]
    base=first[-1]
    assert base['update']==args.checkpoint_update
    assert read(args.second/'status.json')['status']=='completed'
    source_checkpoint=args.first/'training'/f'checkpoint-{args.checkpoint_update:06}.bin'
    digest=lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
    assert digest(source_checkpoint)==digest(args.second/'warm-start.bin')
    merged=copy.deepcopy(first)
    snap_a=[s for s in lines(args.first/'training/league-snapshots.jsonl') if s['update']<=args.checkpoint_update]
    offset=len(snap_a)
    for original in b:
        r=copy.deepcopy(original)
        for key in ['update','seconds','decisions','games','training_games_seconds','model_training_seconds']:
            r[key]+=base[key]
        r['league_snapshots']+=offset
        r['online_audit']['lifetime_samples']+=base['online_audit']['lifetime_samples']
        r['online_audit']['measured_audit_seconds']+=base['online_audit']['measured_audit_seconds']
        r['recovery_segment']=2
        merged.append(r)
    args.out.mkdir(parents=True)
    for name in ['training','reports','evaluation']:(args.out/name).mkdir()
    for name in ['dex.json','training-teams.txt','model1.exe']:
        shutil.copy2(args.second/name,args.out/name)
    shutil.copytree(args.second/'source',args.out/'source')
    shutil.copy2(args.first/'training/initial.bin',args.out/'training/initial.bin')
    final=b[-1]['update']; global_final=merged[-1]['update']
    shutil.copy2(args.second/'training'/f'checkpoint-{final:06}.bin',args.out/'training'/f'checkpoint-{global_final:06}.bin')
    (args.out/'training/metrics.jsonl').write_text(''.join(json.dumps(r)+'\n' for r in merged))
    snaps=copy.deepcopy(snap_a)
    for s in lines(args.second/'training/league-snapshots.jsonl'):
        s['update']+=base['update'];s['seconds']+=base['seconds'];s['snapshot']+=offset
        snaps.append(s)
    (args.out/'training/league-snapshots.jsonl').write_text(''.join(json.dumps(s)+'\n' for s in snaps))
    cfg=read(args.second/'training/config.json')
    cfg.update(max_seconds=10800,elapsed_offset_seconds=0,league_weights=[],out=str((args.out/'training').resolve()))
    cfg['combined_report']=True
    write(args.out/'training/config.json',cfg)
    sa=read(args.first/'status.json');sb=read(args.second/'status.json')
    write(args.out/'status.json',{'status':'completed','duration_seconds':10800,
        'started_unix':sa['started_unix'],'finished_unix':sb['finished_unix'],
        'binary_sha256':digest(args.out/'model1.exe'),'interrupted_and_recovered':True,
        'note':'Derived retained-training report; see recovery-ledger.json for physical runs and discarded-tail costs.'})
    write(args.out/'evaluation-index.json',[])
    cost_a=read(args.first/'timing-summary.json')
    cost_b=read(args.second/'timing-summary.json')
    ledger={'first_run':str(args.first.resolve()),'recovery_run':str(args.second.resolve()),
        'retained_checkpoint_update':base['update'],'retained_seconds':base['seconds'],
        'retained_decisions':base['decisions'],'discarded_seconds':a[-1]['seconds']-base['seconds'],
        'discarded_updates':a[-1]['update']-base['update'],
        'retained_training_seconds':merged[-1]['seconds'],
        'actual_training_seconds':cost_a['combined_training_seconds']+cost_b['combined_training_seconds'],
        'actual_training_games_seconds':cost_a['training_games_seconds']+cost_b['training_games_seconds'],
        'actual_model_training_seconds':cost_a['model_training_seconds']+cost_b['model_training_seconds'],
        'actual_evaluation_games_seconds':0,
        'actual_other_seconds':cost_a['training_other_seconds']+cost_b['training_other_seconds'],
        'actual_games':a[-1]['games']+b[-1]['games'],
        'actual_opponent_search_positions':sum(r.get('opponent_matrix_positions',0) for r in a+b),
        'actual_opponent_move_requests':sum(r.get('opponent_move_requests',0) for r in a+b),
        'actual_learner_simulations':sum(r.get('matrix_simulations',0) for r in a+b),
        'actual_opponent_simulations':sum(r.get('opponent_matrix_simulations',0) for r in a+b),
        'restart_note':'Frozen league and LR schedule restored; Adam, environment games and RNG streams restarted.',
        'cause':sa.get('error'),'resumed_checkpoint_sha256':digest(source_checkpoint)}
    write(args.out/'recovery-ledger.json',ledger)
    last=merged[-1]
    write(args.out/'training/timing.json',{'training_wall_seconds':last['seconds'],
        'training_games_seconds':last['training_games_seconds'],'model_training_seconds':last['model_training_seconds'],
        'measured_audit_seconds':last['online_audit']['measured_audit_seconds'],'games':last['games'],
        'method':'Retained trajectory only; actual compute including discarded tail is in recovery-ledger.json'})
    cases=lines(args.second/'training/mistake-examples.jsonl')
    for w in cases:
        w['update']+=base['update'];w['decision']+=base['decisions']
        w['game']='recovery-'+str(w.get('game'));w['source_run']=str(args.second.resolve())
    (args.out/'training/mistake-examples.jsonl').write_text(''.join(json.dumps(w)+'\n' for w in cases))
    shutil.copy2(__file__,args.out/'reports/stitch_recovery.py')
    print(json.dumps(ledger,indent=2))


if __name__=='__main__':main()
