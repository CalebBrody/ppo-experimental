"""Evaluate a completed new-architecture run and compare its final model to baseline."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import time
import numpy as np

ap = argparse.ArgumentParser()
ap.add_argument('run', type=Path)
ap.add_argument('--baseline', type=Path, required=True)
args = ap.parse_args()
root = Path(__file__).resolve().parents[1]
run = args.run.resolve()
exe = run / 'evaluator.exe'
shutil.copy2(root / 'target/release/model1.exe', exe)
metrics = [json.loads(s) for s in (run / 'training/metrics.jsonl').read_text().splitlines()]
final = metrics[-1]['update']
assert metrics[-1]['seconds'] >= 1200, 'Training has not reached the requested 20 minutes'
baseline_file = args.baseline.with_suffix('.bin')
baseline_hash = hashlib.sha256(baseline_file.read_bytes()).hexdigest()
checkpoints = [(int(p.stem.split('-')[1]), p.with_suffix('')) for p in sorted((run / 'training').glob('checkpoint-*.bin'))]
middle = [p for p in checkpoints if p[0] != final]
if len(middle) > 3:
    middle = [middle[round(i * (len(middle)-1)/2)] for i in range(3)]
paths = [(0, run / 'training/initial'), *middle, (final, run / 'training' / f'checkpoint-{final:06}')]
points = []

def evaluate(label, weights, games, extra):
    dest = run / label
    cmd = [str(exe), '--weights', str(weights), '--teams', 'data/eval-teams.txt', '--evaluate', str(games), '--seed', '381000', '--out', str(dest), *extra]
    with (run / (label+'.log')).open('w') as log:
        subprocess.run(cmd, cwd=root, stdout=log, stderr=subprocess.STDOUT, check=True)
    report = json.loads((dest/'evaluation.json').read_text())
    print(label, report['wins'], report['losses'], report['draws'], flush=True)
    return report, dest/'evaluation.json'

for update, weights in paths:
    for opponent in ['random', 'initial']:
        games = 400 if update in (0, final) else 100
        extra = [] if opponent == 'random' else ['--eval-opponent', str(run/'training/initial')]
        report, path = evaluate(f'eval-{update:06}-{opponent}', weights, games, extra)
        points.append(dict(update=update, opponent=opponent, report=str(path), score=(report['wins']+.5*report['draws'])/games, games=games))
        (run/'evaluation-index.json').write_text(json.dumps(points,indent=2))

report, path = evaluate('head-to-head-baseline', paths[-1][1], 1000, ['--eval-baseline', str(args.baseline)])
values = np.array([r['score'] for r in report['results']]).reshape(-1,2).mean(axis=1)
rng = np.random.default_rng(37)
ci = np.quantile(values[rng.integers(0,len(values),(10000,len(values)))].mean(axis=1), [.025,.975])*100
summary = dict(new_wins=report['wins'], old_wins=report['losses'], draws=report['draws'], games=report['games'], new_score_percent=100*values.mean(), score_ci95=ci.tolist(), baseline=str(args.baseline), baseline_sha256=baseline_hash, new_checkpoint=str(paths[-1][1]), report=str(path), completed_unix=time.time())
assert hashlib.sha256(baseline_file.read_bytes()).hexdigest() == baseline_hash
(run/'comparison-summary.json').write_text(json.dumps(summary,indent=2))
subprocess.run([sys.executable,str(root/'scripts/plot_progress.py'),str(run),'--out',str(root/'reports/experimental-20m-20261103')],cwd=root,check=True)
print(json.dumps(summary,indent=2),flush=True)
