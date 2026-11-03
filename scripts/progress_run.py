"""Bounded GPU training followed by fixed-opponent checkpoint evaluation."""
import argparse
import json
from pathlib import Path
import subprocess
import time
import shutil
import hashlib

def main():
    ap=argparse.ArgumentParser()
    ap.add_argument('--out',type=Path,required=True)
    ap.add_argument('--seconds',type=int,default=900)
    ap.add_argument('--endpoint-games',type=int,default=400)
    ap.add_argument('--middle-games',type=int,default=100)
    args=ap.parse_args()
    root=Path(__file__).resolve().parents[1]
    exe=root/'target/release/model1.exe'
    out=args.out.resolve()
    out.mkdir(parents=True,exist_ok=False)
    training=out/'training'
    source=out/'source'
    source.mkdir()
    for name in ['Cargo.toml','Cargo.lock','src/main.rs','src/model.rs','src/env.rs']:
        dest=source/name
        dest.parent.mkdir(parents=True,exist_ok=True)
        shutil.copy2(root/name,dest)
    manifest={'training_seconds_budget':args.seconds,'endpoint_games':args.endpoint_games,'middle_games':args.middle_games,
              'binary_sha256':hashlib.sha256(exe.read_bytes()).hexdigest(),
              'evaluation_seed':81000,'started_unix':time.time()}
    (out/'manifest.json').write_text(json.dumps(manifest,indent=2))
    def run(label,options):
        print(label,flush=True)
        with (out/(label+'.log')).open('w',encoding='utf-8') as log:
            subprocess.run([str(exe),*map(str,options)],cwd=root,stdout=log,stderr=subprocess.STDOUT,check=True)
    run('train',['--updates',500,'--max-seconds',args.seconds,'--envs',8,'--steps',64,'--batch',32,'--epochs',2,
                 '--snapshot-every',50,'--out',training])
    metrics=[json.loads(line) for line in (training/'metrics.jsonl').read_text().splitlines()]
    final=metrics[-1]['update']
    all_paths=[(0,training/'initial')]+[(int(p.stem.split('-')[1]),p.with_suffix('')) for p in sorted(training.glob('checkpoint-*.bin'))]
    # At most five middle checkpoints, distributed across the completed run.
    middle=[p for p in all_paths if p[0] not in (0,final)]
    if len(middle)>5:
        middle=[middle[round(i*(len(middle)-1)/4)] for i in range(5)]
    paths=[all_paths[0],*middle,all_paths[-1]]
    points=[]
    for update,path in paths:
        games=args.endpoint_games if update in (0,final) else args.middle_games
        for opponent in ['random','initial']:
            label=f'eval-{update:06}-{opponent}'
            options=['--weights',path,'--teams','data/eval-teams.txt','--evaluate',games,'--seed',81000,'--out',out/label]
            if opponent=='initial': options+=['--eval-opponent',training/'initial']
            run(label,options)
            result=json.loads((out/label/'evaluation.json').read_text())
            points.append({'update':update,'opponent':opponent,'report':str(out/label/'evaluation.json'),
                           'score':(result['wins']+0.5*result['draws'])/games,'games':games})
            (out/'evaluation-index.json').write_text(json.dumps(points,indent=2))
            print(f"update={update} opponent={opponent} score={points[-1]['score']:.3f} n={games}",flush=True)
    manifest['finished_unix']=time.time()
    manifest['completed_updates']=final
    (out/'manifest.json').write_text(json.dumps(manifest,indent=2))
    print('COMPLETE '+str(out),flush=True)

if __name__=='__main__':main()
