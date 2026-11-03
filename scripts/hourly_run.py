"""One continuous 24h trainer plus hourly checkpoint evaluation and graphs.

No hourly optimizer resets. Windows sleep prevention lasts only while this
supervisor is alive. STOP file requests a stop; existing checkpoints survive.
"""
import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time
import traceback
from hourly_plot import atomic_json,render

def main():
    ap=argparse.ArgumentParser()
    ap.add_argument('--out',type=Path,required=True)
    ap.add_argument('--weights',type=Path,required=True)
    ap.add_argument('--hours',type=int,default=24)
    ap.add_argument('--hour-seconds',type=int,default=3600)
    ap.add_argument('--games',type=int,default=100)
    ap.add_argument('--random-games',type=int,default=200)
    ap.add_argument('--test',action='store_true')
    args=ap.parse_args()
    assert args.hours>0 and args.hour_seconds>0 and args.games>0 and args.games%2==0 and args.random_games%2==0
    root=Path(__file__).resolve().parents[1];out=args.out.resolve()
    out.mkdir(parents=True,exist_ok=False)
    for name in ['source','reports','evaluation','training']:(out/name).mkdir()
    # Freeze the executable and sources so later workspace edits cannot change a run.
    exe=out/'model1.exe';shutil.copy2(root/'target/release/model1.exe',exe)
    for name in ['Cargo.toml','Cargo.lock','src/main.rs','src/env.rs','src/model.rs','scripts/hourly_run.py','scripts/hourly_plot.py']:
        dest=out/'source'/name;dest.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(root/name,dest)
    shutil.copy2(root/'data/teams.txt',out/'training-teams.txt')
    shutil.copy2(root/'data/eval-teams.txt',out/'evaluation-teams.txt')
    dex=root/'vendor/Ninjask/data/derived/format/gen9championsvgc2026regmc.json'
    shutil.copy2(dex,out/'dex.json')
    started=time.time()
    state={'status':'starting','supervisor_pid':os.getpid(),'started_unix':started,'scheduled_end_unix':started+args.hours*args.hour_seconds,
           'hours':args.hours,'hour_seconds':args.hour_seconds,'last_report_hour':-1,'weights':str(args.weights.resolve()),
           'checkpoint_games':args.games,'random_games':args.random_games,'binary_sha256':hashlib.sha256(exe.read_bytes()).hexdigest()}
    atomic_json(out/'status.json',state)
    points=[];atomic_json(out/'evaluations.json',points)
    if os.name=='nt':ctypes.windll.kernel32.SetThreadExecutionState(0x80000001)
    train=None;current=None
    trainlog=(out/'training.log').open('w',encoding='utf-8')
    def save_status(**kw):
        state.update(kw);state['heartbeat_unix']=time.time();atomic_json(out/'status.json',state)
    try:
        command=[str(exe),'--weights',str(args.weights.resolve()),'--dex',str(out/'dex.json'),'--teams',str(out/'training-teams.txt'),
                 '--out',str(out/'training'),'--updates','1000000','--max-seconds',str(args.hours*args.hour_seconds),
                 '--hour-seconds',str(args.hour_seconds),'--envs','8','--steps','64','--batch','32','--epochs','2','--snapshot-every','100','--seed','137']
        if args.test:
            for flag,value in [('--envs','2'),('--steps','4'),('--batch','4'),('--epochs','1')]:command[command.index(flag)+1]=value
        train=subprocess.Popen(command,cwd=root,stdout=trainlog,stderr=subprocess.STDOUT)
        save_status(status='training',trainer_pid=train.pid,command=command)
        for hour in range(args.hours+1):
            marker=out/'training'/f'hour-{hour:03}.json'
            while not marker.exists():
                if (out/'STOP').exists():raise InterruptedError('STOP file requested termination')
                if train.poll() is not None:raise RuntimeError(f'Trainer exited {train.returncode} before hour {hour}; see training.log')
                save_status(status='training',waiting_for_hour=hour)
                time.sleep(2 if args.test else 15)
            for attempt in range(100):
                try:
                    json.loads(marker.read_text())
                    break
                except json.JSONDecodeError:
                    if attempt==99:raise
                    time.sleep(.05)
            path=out/'training'/f'hour-{hour:03}'
            opponents=['random']+[f'hour-{h:03}' for h in (range(hour) if hour else [0])]
            for opponent in opponents:
                save_status(status='evaluating',evaluating_hour=hour,evaluating_opponent=opponent)
                name=f'hour-{hour:03}-vs-{opponent}';dest=out/'evaluation'/name
                cmd=[str(exe),'--weights',str(path),'--dex',str(out/'dex.json'),'--teams',str(out/'evaluation-teams.txt'),
                     '--out',str(dest),'--evaluate',str(args.random_games if opponent=='random' else args.games),'--seed','181000']
                if opponent!='random':cmd+=['--eval-opponent',str(out/'training'/opponent)]
                with (out/'evaluation'/(name+'.log')).open('w',encoding='utf-8') as log:
                    current=subprocess.Popen(cmd,cwd=root,stdout=log,stderr=subprocess.STDOUT)
                    save_status(evaluator_pid=current.pid)
                    while current.poll() is None:
                        if (out/'STOP').exists():raise InterruptedError('STOP file requested termination')
                        if train.poll() not in (None,0):raise RuntimeError(f'Trainer exited {train.returncode}')
                        if train.poll()==0 and hour<args.hours and not (out/'training'/f'hour-{args.hours:03}.json').exists():raise RuntimeError('Trainer exited without final checkpoint')
                        save_status();time.sleep(2 if args.test else 10)
                    if current.returncode:raise RuntimeError(f'Evaluation {name} exited {current.returncode}')
                report=json.loads((dest/'evaluation.json').read_text())
                points.append({'hour':hour,'opponent':opponent,'games':report['games'],
                               'score':(report['wins']+.5*report['draws'])/report['games'],'report':str(dest/'evaluation.json')})
                atomic_json(out/'evaluations.json',points)
            render(out,hour)
            save_status(status='training' if hour<args.hours else 'finishing',last_report_hour=hour,evaluator_pid=None)
            print(f'Hourly report ready: {hour}',flush=True)
        code=train.wait(timeout=120)
        if code:raise RuntimeError(f'Trainer exited {code}')
        save_status(status='completed',finished_unix=time.time())
    except BaseException as ex:
        if current and current.poll() is None:current.terminate();current.wait(timeout=30)
        if train and train.poll() is None:train.terminate();train.wait(timeout=30)
        save_status(status='stopped' if isinstance(ex,InterruptedError) else 'failed',error=str(ex),traceback=traceback.format_exc())
        raise
    finally:
        trainlog.close()
        if os.name=='nt':ctypes.windll.kernel32.SetThreadExecutionState(0x80000000)

if __name__=='__main__':main()
