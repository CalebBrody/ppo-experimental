"""Read-only compact training heartbeat; tolerates an in-progress metrics write."""
import argparse
import json
import math
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('run', type=Path)
    args = parser.parse_args()
    state = json.loads((args.run / 'status.json').read_text())
    rows = []
    for line in (args.run / 'training/metrics.jsonl').read_text().splitlines():
        try:
            rows.append(json.loads(line))
        except json.JSONDecodeError:
            pass
    if not rows:
        print(json.dumps({'status': state['status'], 'minutes': 0}))
        return
    last = rows[-1]
    audit = last.get('online_audit') or {}
    recent = rows[-20:]
    games = sum(r['wins'] + r['losses'] + r['draws'] for r in recent)
    requests = sum(r.get('opponent_move_requests', 0) for r in rows)
    searched = sum(r.get('opponent_matrix_positions', 0) for r in rows)
    print(json.dumps({'status': state['status'], 'minutes': round(last['seconds']/60, 2),
                      'target_minutes': state['duration_seconds']/60,
                      'update': last['update'], 'games': last['games'], 'decisions': last['decisions'],
                      'score20': round(100 * sum(r['wins']+.5*r['draws'] for r in recent)/games, 2) if games else None,
                      'known_immunity': audit.get('known_type_immunity_per_100_attempts'),
                      'failed_moves': audit.get('failed_moves_per_100_attempts'),
                      'samples': audit.get('window_samples'),
                      'finite_losses': all(math.isfinite(r[k]) for r in rows for k in ['policy_loss','value_loss','entropy','kl']),
                      'league_size': last.get('league_size'),
                      'opponent_search': [searched, requests],
                      'latest_report_update': state.get('last_report_update')}))


if __name__ == '__main__':
    main()
