"""Continuous timed training with periodic checkpoint evaluations and progress graphs.

Example: python scripts/train_timed.py --duration 20m --interval 5m --out E:/tmp/new-run
Requires NumPy and Matplotlib. Existing output folders are never overwritten.
"""
import argparse
import ctypes
import hashlib
import json
import math
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time


def duration(text):
    match = re.fullmatch(r'(\d+(?:\.\d+)?)(s|m|h|d)?', text.strip().lower())
    if not match:
        raise argparse.ArgumentTypeError('Use a positive duration such as 30s, 20m, 4h, or 1d')
    value = math.ceil(float(match[1]) * {'s': 1, 'm': 60, 'h': 3600, 'd': 86400}[match[2] or 's'])
    if value < 1:
        raise argparse.ArgumentTypeError('Duration must be positive')
    return value


def write_json(path, value):
    temp = path.with_suffix('.tmp')
    temp.write_text(json.dumps(value, indent=2), encoding='utf-8')
    # Windows readers/antivirus can briefly deny replacement despite valid ACLs.
    for attempt in range(40):
        try:
            temp.replace(path)
            return
        except PermissionError:
            if attempt == 39:
                raise
            time.sleep(.05)


def read_json(path):
    try:
        return json.loads(path.read_text())
    except (FileNotFoundError, json.JSONDecodeError):
        return None  # A running trainer may still be writing its ready marker.


def main():
    root = Path(__file__).resolve().parents[1]
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--duration', type=duration, required=True)
    ap.add_argument('--interval', type=duration, default=300)
    ap.add_argument('--out', type=Path, required=True)
    ap.add_argument('--weights', type=Path, help='Optional compatible warm-start weights, without .bin')
    ap.add_argument('--league-weights', type=Path, nargs='+', default=[], help='Restore frozen Model2 league checkpoints, without .bin')
    ap.add_argument('--elapsed-offset-seconds', type=float, default=0, help='Model2 learning-rate schedule offset for an interrupted continuation')
    ap.add_argument('--anneal-seconds', type=float, default=0, help='Model2 original total learning-rate schedule duration')
    ap.add_argument('--trainer', choices=['ppo', 'model2'], default='ppo')
    ap.add_argument('--matrix-samples', type=int, default=2)
    ap.add_argument('--regret-iterations', type=int, default=256)
    ap.add_argument('--matrix-opponents', action=argparse.BooleanOptionalAction, default=True,
                    help='Model2 frozen opponents use the same matrix search (default enabled)')
    ap.add_argument('--games', type=int, default=0, help='Even evaluation games per opponent; default 0 uses online training diagnostics only')
    ap.add_argument('--audit-every',type=int,default=16,help='Sample one existing learner transition in N; 0 disables auditing')
    ap.add_argument('--audit-window',type=int,default=2048)
    ap.add_argument('--collection-workers',type=int,default=1)
    ap.add_argument('--league-interval',type=duration,default=300,help='Wall-clock spacing of frozen league opponents; default 5m')
    ap.add_argument('--teams', type=Path, default=root/'data/teams.txt')
    ap.add_argument('--eval-teams', type=Path, default=root/'data/eval-teams.txt')
    ap.add_argument('--dex', type=Path, default=root/'vendor/Ninjask/data/derived/format/gen9championsvgc2026regmc.json')
    ap.add_argument('--seed', type=int, default=137)
    ap.add_argument('--eval-seed', type=int, default=481000)
    ap.add_argument('--envs', type=int, default=8)
    ap.add_argument('--steps', type=int, default=64)
    ap.add_argument('--batch', type=int, default=32)
    ap.add_argument('--epochs', type=int, default=2)
    ap.add_argument('--skip-build', action='store_true', help='Use an already verified, current release executable')
    args = ap.parse_args()
    if args.games < 0 or args.games % 2:
        ap.error('--games must be zero (online only) or positive and even')
    if args.audit_every<0 or args.audit_window<1 or args.collection_workers<1:
        ap.error('Audit rate must be nonnegative; window and workers must be positive')
    if min(args.envs, args.steps, args.epochs) < 1 or args.batch < 2:
        ap.error('Counts must be positive; batch must be at least 2')
    out = args.out.resolve()
    if out.exists():
        ap.error(f'Refusing to overwrite existing output folder: {out}')
    # Fail before launching training if plotting dependencies are absent.
    import numpy  # noqa: F401
    import matplotlib  # noqa: F401
    inputs=[(args.teams,'training-teams.txt'),(args.dex,'dex.json')]
    if args.games:
        inputs.append((args.eval_teams,'evaluation-teams.txt'))
    for path,_ in inputs:
        if not path.is_file():
            ap.error(f'Missing input: {path}')
    if args.weights and not args.weights.with_suffix('.bin').is_file():
        ap.error('--weights must point to an existing compatible checkpoint without .bin')
    if any(not p.with_suffix('.bin').is_file() for p in args.league_weights):
        ap.error('Missing --league-weights checkpoint')
    if not args.skip_build:
        subprocess.run(['cargo', 'build', '--release', '--locked'], cwd=root, check=True)
    binary = root/'target/release'/('model1.exe' if os.name == 'nt' else 'model1')
    if not binary.is_file():
        ap.error(f'Build executable first: {binary}')
    out.mkdir(parents=True, exist_ok=False)
    for name in ['training', 'evaluation', 'reports', 'source']:
        (out/name).mkdir()
    exe = out/binary.name
    shutil.copy2(binary, exe)
    for source, name in inputs:
        shutil.copy2(source, out/name)
    for folder in ['src', 'scripts']:
        shutil.copytree(root/folder, out/'source'/folder, ignore=shutil.ignore_patterns('__pycache__'))
    for name in ['Cargo.toml', 'Cargo.lock']:
        shutil.copy2(root/name, out/'source'/name)
    weights = None
    if args.weights:
        weights = out/'warm-start'
        shutil.copy2(args.weights.with_suffix('.bin'), weights.with_suffix('.bin'))
    league_weights=[]
    if args.league_weights:
        (out/'league-warm-start').mkdir()
        for i,path in enumerate(args.league_weights):
            dest=out/'league-warm-start'/f'opponent-{i:03}'
            shutil.copy2(path.with_suffix('.bin'),dest.with_suffix('.bin'))
            league_weights.append(dest)
    state = dict(status='starting', supervisor_pid=os.getpid(), duration_seconds=args.duration,
                 interval_seconds=args.interval, started_unix=time.time(), last_report_update=None,
                 binary_sha256=hashlib.sha256(exe.read_bytes()).hexdigest())
    points = []
    write_json(out/'evaluation-index.json', points)
    trainer = evaluator = None

    def status(**values):
        state.update(values)
        state['heartbeat_unix'] = time.time()
        write_json(out/'status.json', state)

    def check():
        if (out/'STOP').exists():
            raise InterruptedError('STOP file requested termination')
        if trainer and trainer.poll() not in (None, 0):
            raise RuntimeError(f'Trainer failed ({trainer.returncode}); see training.log')

    def report(update, checkpoint):
        nonlocal evaluator
        for opponent in (['random', 'initial'] if args.games else []):
            check()
            dest = out/'evaluation'/f'update-{update:06}-{opponent}'
            command = [str(exe), '--weights', str(checkpoint), '--dex', str(out/'dex.json'),
                       '--teams', str(out/'evaluation-teams.txt'), '--evaluate', str(args.games),
                       '--seed', str(args.eval_seed), '--out', str(dest), '--eval-traces']
            if opponent == 'initial':
                command += ['--eval-opponent', str(out/'training/hour-000')]
            status(status='evaluating', evaluating_update=update, opponent=opponent)
            with dest.with_suffix('.log').open('w') as log:
                evaluator = subprocess.Popen(command, cwd=root, stdout=log, stderr=subprocess.STDOUT)
                while evaluator.poll() is None:
                    check()
                    status(evaluator_pid=evaluator.pid)
                    time.sleep(1)
                if evaluator.returncode:
                    raise RuntimeError(f'Evaluation failed: {dest.with_suffix(".log")}')
            result = read_json(dest/'evaluation.json')
            with (out/'issue-check.log').open('a') as audit_log:
                subprocess.run([sys.executable, str(out/'source/scripts/check_run.py'), str(dest),
                                '--dex', str(out/'dex.json')], cwd=root,
                               stdout=audit_log, stderr=subprocess.STDOUT, check=True)
            points.append(dict(update=update, opponent=opponent, report=str(dest/'evaluation.json'),
                               games=args.games, score=(result['wins']+.5*result['draws'])/args.games))
            write_json(out/'evaluation-index.json', points)
        dest = out/'reports'/f'update-{update:06}'
        with (out/'plot.log').open('a') as log:
            subprocess.run([sys.executable, str(out/'source/scripts/plot_interval_run.py'), str(out),
                            '--out', str(dest), '--through-update', str(update)],
                           cwd=root, stdout=log, stderr=subprocess.STDOUT, check=True)
        latest = read_json(dest/'summary.json')
        latest['progress_image'] = str(dest/'progress.png')
        write_json(out/'latest-report.json', latest)
        status(status='training', last_report_update=update, evaluator_pid=None)
        print(f'Report ready: update {update}: {dest / "progress.png"}', flush=True)

    try:
        if os.name == 'nt':
            ctypes.windll.kernel32.SetThreadExecutionState(0x80000001)
        command = [str(exe), '--updates', '1000000000', '--max-seconds', str(args.duration),
                   '--hour-seconds', str(args.interval), '--envs', str(args.envs), '--steps', str(args.steps),
                   '--batch', str(args.batch), '--epochs', str(args.epochs), '--snapshot-every', '100',
                   '--snapshot-seconds',str(args.league_interval),
                   '--audit-every',str(args.audit_every),'--audit-window',str(args.audit_window),
                   '--collection-workers',str(args.collection_workers),
                   '--seed', str(args.seed), '--dex', str(out/'dex.json'), '--teams', str(out/'training-teams.txt'),
                   '--out', str(out/'training')]
        if weights:
            command += ['--weights', str(weights)]
        if args.trainer == 'model2':
            command += ['--model2', '--matrix-samples', str(args.matrix_samples),
                        '--regret-iterations', str(args.regret_iterations),
                        '--matrix-opponents', str(args.matrix_opponents).lower()]
            command += ['--elapsed-offset-seconds',str(args.elapsed_offset_seconds),
                        '--anneal-seconds',str(args.anneal_seconds)]
            if league_weights:
                command += ['--league-weights',*[str(p) for p in league_weights]]
        with (out/'training.log').open('w') as log:
            trainer = subprocess.Popen(command, cwd=root, stdout=log, stderr=subprocess.STDOUT)
            status(status='training', trainer_pid=trainer.pid, command=command)
            done = set()
            while True:
                check()
                # Scan actual ready markers: slow PPO updates may cross multiple intervals.
                markers = sorted((out/'training').glob('hour-*.json'))
                for marker in markers:
                    meta = read_json(marker)
                    if meta is not None and meta['update'] not in done:
                        report(meta['update'], marker.with_suffix(''))
                        done.add(meta['update'])
                if trainer.poll() == 0:
                    remaining = [read_json(p) for p in (out/'training').glob('hour-*.json')]
                    if any(m is not None and m['update'] not in done for m in remaining):
                        continue
                    rows = [json.loads(line) for line in (out/'training/metrics.jsonl').read_text().splitlines()]
                    last = rows[-1]
                    if last['seconds'] < args.duration:
                        raise RuntimeError('Trainer stopped before requested duration')
                    if last['update'] not in done:
                        report(last['update'], out/'training'/f'checkpoint-{last["update"]:06}')
                    break
                status()
                time.sleep(1)
        status(status='completed', finished_unix=time.time())
        with (out/'issue-check.log').open('a') as audit_log:
            subprocess.run([sys.executable, str(out/'source/scripts/check_run.py'), str(out)],
                           cwd=root, stdout=audit_log, stderr=subprocess.STDOUT, check=True)
    except BaseException as error:
        for child in [evaluator, trainer]:
            if child and child.poll() is None:
                child.terminate()
                try:
                    child.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    child.kill()
                    child.wait()
        status(status='stopped' if isinstance(error, (InterruptedError, KeyboardInterrupt)) else 'failed', error=str(error))
        raise
    finally:
        if os.name == 'nt':
            ctypes.windll.kernel32.SetThreadExecutionState(0x80000000)


if __name__ == '__main__':
    main()
