"""Compare cached pre-type-change samples with a continuation, without extra games."""
import argparse
import hashlib
import json
import statistics
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('cache', type=Path)
    parser.add_argument('run', type=Path)
    parser.add_argument('--reset-run', type=Path,
                        help='Add a fresh-weight run as the third results column.')
    args = parser.parse_args()
    manifest = json.loads((args.cache / 'manifest.json').read_text())
    mismatches = [name for name, digest in manifest['sha256'].items()
                  if hashlib.sha256((args.cache / name).read_bytes()).hexdigest() != digest]
    rows = [json.loads(line) for line in
            (args.run / 'training/metrics.jsonl').read_text().splitlines()]
    last = rows[-1]
    recent = rows[-20:]
    baseline = json.loads((args.cache / 'baseline-summary.json').read_text())
    old_rows = [json.loads(line) for line in
                (args.cache / 'run/training/metrics.jsonl').read_text().splitlines()]
    result = {
        'cache_files_verified': len(manifest['sha256']),
        'cache_hash_mismatches': mismatches,
        'baseline_saved_checkpoint_update': 192,
        'baseline': baseline,
        'observed_phase_medians_seconds': {
            name: {key: statistics.median(row[key] for row in records[10:])
                   for key in ('training_games_update_seconds', 'model_training_update_seconds')}
            for name, records in [('cached_run', old_rows), ('current_types_run', rows)]
        },
        'continuation': {
            'update': last['update'], 'games': last['games'], 'seconds': last['seconds'],
            'league_size': last['league_size'], 'online_audit': last['online_audit'],
            'reward_per_turn_20_updates': sum(r['completed_turn_reward_sum'] for r in recent)
                / sum(r['completed_battle_turns'] for r in recent),
            'training_score_20_updates': (sum(r['wins'] + .5 * r['draws'] for r in recent)
                / sum(r['wins'] + r['losses'] + r['draws'] for r in recent)),
        },
        'limitations': [
            'Continuation from saved update 192; old final metrics at 212 have no matching saved weights.',
            'Additional training, changed encoder, restarted optimizer and restarted league are confounded.',
            'Rolling windows describe sampled training games, not held-out strength or win rate vs random.',
            'Adjacent rolling windows overlap; their counts must not be summed as independent samples.',
            'Phase medians exclude the first ten updates, but are not controlled speed measurements.',
        ],
    }
    if args.reset_run:
        reset_rows = [json.loads(line) for line in
                      (args.reset_run / 'training/metrics.jsonl').read_text().splitlines()]
        reset_last = reset_rows[-1]
        reset_recent = reset_rows[-20:]
        reset_status = json.loads((args.reset_run / 'status.json').read_text())
        if '--weights' in reset_status['command']:
            raise SystemExit('Reset run unexpectedly loaded weights')
        result['reset'] = {
            'update': reset_last['update'], 'games': reset_last['games'],
            'seconds': reset_last['seconds'], 'online_audit': reset_last['online_audit'],
            'reward_per_turn_20_updates': sum(r['completed_turn_reward_sum'] for r in reset_recent)
                / sum(r['completed_battle_turns'] for r in reset_recent),
            'weights_loaded': False,
        }
        result['limitations'].append(
            'Fresh and continued runs have different prior training and frozen opponents; this is descriptive, not an encoder-only controlled experiment.')
        columns = [baseline['212'], result['continuation'], result['reset']]
        table = ['| Metric | Before type changes (stopped) | Types, continued 20m | Types, reset 20m |',
                 '|---|---:|---:|---:|']
        for label, key, count_key in [
            ('All immunity events', 'immunity_per_100_attempts', 'observed_immunity_events'),
            ('Known-type immunity hits', 'known_type_immunity_per_100_attempts', 'known_type_immunity_events'),
            ('Failed moves', 'failed_moves_per_100_attempts', 'failed_move_events'),
        ]:
            cells = []
            for record in columns:
                audit = record['online_audit']
                counts = audit['counts']
                cells.append(f"{audit[key]:.2f}% ({counts[count_key]}/{counts['executed_move_attempts']})")
            table.append('| ' + label + ' | ' + ' | '.join(cells) + ' |')
        table.append('| Target-slot 2 share | ' + ' | '.join(
            f"{r['online_audit']['slot2_share_percent']:.1f}%" for r in columns) + ' |')
        table.append('| Average reward/turn (last 20 updates) | ' + ' | '.join(
            f"{r['reward_per_turn_20_updates']:+.4f}" for r in columns) + ' |')
        text = '\n'.join(table) + '\n\nEach event window contains 1,024 sampled learner transitions. No evaluation games.\n'
        (args.reset_run / 'changes-table.md').write_text(text)
        print(text)
    output = (args.reset_run or args.run) / 'type-comparison.json'
    output.write_text(json.dumps(result, indent=2) + '\n')
    print(f'Cache verified: {len(manifest["sha256"])} files; mismatches: {mismatches}')
    for name, record in [('Saved baseline (192)', baseline['192']),
                         ('Stopped baseline (212)', baseline['212']),
                         ('Current types', result['continuation'])]:
        audit = record['online_audit']
        counts = audit['counts']
        print(name, json.dumps({
            'attempts': counts['executed_move_attempts'],
            'immunities': counts['observed_immunity_events'],
            'known_type_immunities': counts['known_type_immunity_events'],
            'failed_moves': counts['failed_move_events'],
            'immunity_rate': audit['immunity_per_100_attempts'],
            'known_type_rate': audit['known_type_immunity_per_100_attempts'],
            'failed_rate': audit['failed_moves_per_100_attempts'],
            'slot2_share': audit['slot2_share_percent'],
            'reward_per_turn': record['reward_per_turn_20_updates'],
        }))
    print(output)
    if mismatches:
        raise SystemExit('Cached artifacts changed')


if __name__ == '__main__':
    main()
