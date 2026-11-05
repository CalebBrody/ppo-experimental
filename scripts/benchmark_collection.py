"""Fixed-work A/B audit-overhead and CPU-worker benchmark. No evaluation games."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import statistics
import subprocess


def main():
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--out',type=Path,required=True)
    ap.add_argument('--weights',type=Path,required=True)
    ap.add_argument('--updates',type=int,default=6)
    ap.add_argument('--workers',type=int,default=4)
    args=ap.parse_args()
    root=Path(__file__).resolve().parents[1]
    if args.out.exists():ap.error('Choose a new output folder')
    args.out.mkdir(parents=True)
    binary=args.out/'model1.exe';shutil.copy2(root/'target/release/model1.exe',binary)
    weights=args.weights.resolve()
    runs=[]
    for label,audit,workers in [('off-1',0,1),('on-1',16,1),('on-2',16,1),('off-2',0,1),('parallel',16,args.workers)]:
        dest=(args.out/label).resolve()
        command=[str(binary.resolve()),'--weights',str(weights),'--updates',str(args.updates),
                 '--envs','8','--steps','64','--batch','32','--epochs','2','--seed','137',
                 '--audit-every',str(audit),'--audit-window','128','--collection-workers',str(workers),'--out',str(dest)]
        print('Starting '+label,flush=True)
        with (args.out/(label+'.log')).open('w') as log:
            subprocess.run(command,cwd=root,stdout=log,stderr=subprocess.STDOUT,check=True)
        metrics=[json.loads(s) for s in (dest/'metrics.jsonl').read_text().splitlines()]
        timing=json.loads((dest/'timing.json').read_text())
        run=dict(label=label,audit_every=audit,workers=workers,
                 steady_collection_seconds=[m['training_games_update_seconds'] for m in metrics[1:]],
                 steady_optimization_seconds=[m['model_training_update_seconds'] for m in metrics[1:]],
                 timing=timing,metrics=metrics)
        runs.append(run)
        print(label+': '+str(statistics.median(run['steady_collection_seconds']))+' s/collection update',flush=True)
        (args.out/'partial.json').write_text(json.dumps(runs,indent=2)+'\n')
    serial=runs[:4]
    off=[v for r in serial if not r['audit_every'] for v in r['steady_collection_seconds']]
    on=[v for r in serial if r['audit_every'] for v in r['steady_collection_seconds']]
    parallel=runs[-1]['steady_collection_seconds']
    signatures=lambda r:[tuple(m.get(k) for k in ['games','wins','losses','draws','mean_episode_return']) for m in r['metrics']]
    parity=all(signatures(r)==signatures(serial[0]) for r in serial[1:])
    hashes={r['label']:hashlib.sha256((args.out/r['label']/f'checkpoint-{args.updates:06}.bin').read_bytes()).hexdigest() for r in runs}
    statistics_signature=lambda r:[tuple(m[k] for k in ['policy_loss','value_loss','entropy','kl']) for m in r['metrics']]
    report=dict(executable_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),updates_per_run=args.updates,
                audit_off_median_seconds=statistics.median(off),audit_on_median_seconds=statistics.median(on),
                estimated_collection_overhead_percent=100*(statistics.median(on)/statistics.median(off)-1),
                directly_measured_audit_fraction_percent=100*sum(r['timing']['measured_audit_seconds'] for r in serial if r['audit_every'])/sum(r['timing']['training_games_seconds'] for r in serial if r['audit_every']),
                serial_audit_on_off_outcome_parity=parity,
                parallel_workers=args.workers,parallel_median_seconds=statistics.median(parallel),
                parallel_speedup=statistics.median(on)/statistics.median(parallel),
                parallel_outcome_parity=signatures(runs[-1])==signatures(serial[0]),
                checkpoint_hashes=hashes,all_checkpoints_identical=len(set(hashes.values()))==1,
                all_ppo_statistics_identical=all(statistics_signature(r)==statistics_signature(runs[0]) for r in runs),
                runs=runs,limitations='One machine/seed; ABBA order, first update excluded for warmup. Timing differences include system noise and are not confidence intervals.')
    (args.out/'summary.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({k:v for k,v in report.items() if k!='runs'},indent=2),flush=True)


if __name__=='__main__':main()
