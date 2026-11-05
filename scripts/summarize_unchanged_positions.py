"""Validate deterministic replays and summarize cell-level state ablations."""
from pathlib import Path
from collections import Counter
import hashlib
import json

root = Path(__file__).resolve().parents[1]
base = root / 'Model2/runs/state-fixes-replay-20261104'
out = base / 'unchanged-position-audit'
targets = json.loads((out / 'targets.json').read_text())
modes = ['own_unburden_off', 'all_unburden_off', 'legacy_three_fields']
keys = ['events_equal', 'action_order_equal', 'leaf_features_equal', 'reward_equal']
rows = []
cache = {}
verified_games = 0
extra_simulations = 0
for pool, pair in sorted({(r['pool'], r['game']//2) for r in targets}):
    for local in range(2):
        game = pair*2+local
        a = json.loads((out/f'{pool}-pair-{pair:03}/traces/game-{local:04}.json').read_text())
        b = json.loads((base/f'fixed-{pool}/traces/game-{game:04}.json').read_text())
        for k in ['match_seed', 'learner_side', 'team_a', 'team_b', 'outcome']:
            assert a[k] == b[k], (pool, game, k)
        assert len(a['decisions']) == len(b['decisions'])
        for x, y in zip(a['decisions'], b['decisions']):
            for k, v in y.items():
                if k != 'search':
                    assert x[k] == v, (pool, game, x['decision'], k)
            if y.get('search'):
                for k, v in y['search'].items():
                    if k not in ['cell_traces', 'diagnostic_reconstructed', 'diagnostic_actual']:
                        assert x['search'][k] == v, (pool, game, x['decision'], k)
            for c in (x.get('search') or {}).get('cell_traces') or []:
                extra_simulations += len(c.get('ablations', []))
        cache[pool, game] = a
        verified_games += 1

for r in targets:
    a = cache[r['pool'], r['game']]
    s = next(d['search'] for d in a['decisions'] if d['decision'] == r['decision'])
    side = s['side']
    active = []
    for i, m in enumerate(s['audit_world'][side]):
        if m['active'] and m['unburden']:
            actual = s['audit_actual'][side][i]
            assert actual['unburden'] and m['effective_speed'] == actual['effective_speed']
            active.append(dict(name=m['name'], fixed_speed=m['effective_speed'],
                               actual_speed=actual['effective_speed'], stats_speed=m['stats'][5]))
    assert active, r
    cells = s['cell_traces']
    counts = {}
    examples = []
    for mode in modes:
        variants = [next(z for z in c['ablations'] if z['mode'] == mode) for c in cells]
        counts[mode] = {k.replace('_equal', '_changed'): sum(not z[k] for z in variants) for k in keys}
        if mode == 'legacy_three_fields':
            assert all(z['leaf_features_equal'] and z['reward_equal'] for z in variants)
            for c, z in zip(cells, variants):
                if not z['events_equal']:
                    examples.append(dict(row=c['row'], column=c['column'], seed=c['sample_seed'],
                                         fixed_events=c['events'], legacy_events=z['events_if_different']))
    legacy = counts['legacy_three_fields']
    category = ('identical_events' if not legacy['events_changed'] else
                'action_order_only' if legacy['action_order_changed'] else 'residual_order_only')
    rows.append(dict(pool=r['pool'], game=r['game'], turn=r['turn'], decision=r['decision'],
                     fix_verified=True, own_active_unburden=active, simulations=len(cells),
                     category=category, ablations=counts, changed_event_examples=examples))

timings = [json.loads(p.read_text()) for p in out.glob('*-pair-*/timing.json')]
evaluations = [json.loads(p.read_text()) for p in out.glob('*-pair-*/evaluation.json')]
report = dict(
    methodology='Same reconstructed world, candidate pair, and sample RNG seed; rerun each cell with own Unburden off, both sides Unburden off, or all three restored fields reset. No diagnostic ablation consumes the normal search RNG.',
    limitations='Covers the 21 unchanged exposed positions and their sampled candidate cells; does not establish equivalence for unsearched actions or deeper turns. Leaf encoder has no explicit Unburden activation or effective-speed feature.',
    verified_replay_games=verified_games, positions=len(rows),
    target_simulations=sum(r['simulations'] for r in rows),
    categories=dict(Counter(r['category'] for r in rows)),
    training_games_seconds=0, model_training_seconds=0,
    evaluation_games_seconds=sum(t['evaluation_games_seconds'] for t in timings),
    evaluation_other_seconds=sum(t['evaluation_other_seconds'] for t in timings),
    normal_search_simulations=sum(sum(e['search_simulations']) for e in evaluations),
    additional_ablation_simulations=extra_simulations,
    search_fallbacks=sum(sum(e['search_fallbacks']) for e in evaluations),
    positions_detail=rows)
report['sha256'] = {str(p.relative_to(root)): hashlib.sha256(p.read_bytes()).hexdigest() for p in [
    root/'target/release/model1-unchanged-audit.exe', out/'build/src/model2.rs', out/'build/src/env.rs']}
(out/'summary.json').write_text(json.dumps(report, indent=2)+'\n')
print(json.dumps({k:v for k,v in report.items() if k not in ['positions_detail','sha256']}, indent=2))
