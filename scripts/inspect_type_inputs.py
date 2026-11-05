"""Inspect recorded, actual model inputs for confirmed known-type immunity events."""
import argparse
import collections
import json
from pathlib import Path

from check_run import analyze_game

TYPES = ['Bug', 'Dark', 'Dragon', 'Electric', 'Fairy', 'Fighting', 'Fire',
         'Flying', 'Ghost', 'Grass', 'Ground', 'Ice', 'Normal', 'Poison',
         'Psychic', 'Rock', 'Steel', 'Water', 'Stellar', '???']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('evaluation', type=Path)
    parser.add_argument('--dex', type=Path, required=True)
    args = parser.parse_args()
    raw = json.loads(args.dex.read_text())
    dex = {'moves': {m['id']: m for m in raw['moves']}, 'typechart': raw['typechart']}
    examples = []
    all_mismatches = []
    decisions_checked = 0
    for file in sorted((args.evaluation / 'traces').glob('game-*.json')):
        game = json.loads(file.read_text())
        for d in game['decisions']:
            obs = d.get('observation_state')
            if obs is None:
                continue
            decisions_checked += 1
            for rel, mons in enumerate(d['state']['sides']):
                for index, mon in enumerate(mons):
                    base = (rel * 6 + index) * 256
                    encoded = [ty for j, ty in enumerate(TYPES) if obs[base + 236 + j] > .5]
                    if set(encoded) != set(mon['types']):
                        all_mismatches.append({'game': game['game'], 'decision': d['decision'],
                                               'name': mon['name'], 'tracked': mon['types'],
                                               'encoded': encoded})
            counts, events, _ = analyze_game({**game, 'decisions': [d]}, dex)
            if not counts['known_type_immunity_events']:
                continue
            for event in events:
                if event['kind'] != 'known_type_immunity_event':
                    continue
                target = event['target']
                target_slot = ord(target[2]) - ord('a')
                foe_index, foe = next((i, m) for i, m in enumerate(d['state']['sides'][1])
                                      if m['slot'] == target_slot)
                action = next(a for a in d['selected'] if a.get('move') == event['move']
                              and a.get('target') == target_slot + 1)
                k = action['actor_slot']
                descriptor = d.get('action_descriptor_width', 32)
                own_index, own = next((i, m) for i, m in enumerate(d['state']['sides'][0])
                                      if m['slot'] == k)
                base = (6 + foe_index) * 256
                move_base = own_index * 256 + 128 + action['move_slot'] * 24
                marginals = d.get('slot_policy', [[], []])[k]
                def same(a, b):
                    return all(a.get(key) == b.get(key) for key in
                               ('move', 'move_slot', 'target', 'mega', 'switch_index', 'pass'))
                selected_prob = next((m['probability'] for m in marginals
                                      if same(m['choice'], action)), None)
                rank = 1 + sum(m['probability'] > selected_prob for m in marginals) if selected_prob is not None else None
                examples.append({
                    'trace_file': str(file.resolve()), 'game': game['game'],
                    'match_seed': game['match_seed'], 'outcome': game['outcome'],
                    'turn': d['turn'], 'decision': d['decision'], 'action': action,
                    'actor_types': own['types'], 'defender': foe['species'],
                    'included_by_training_audit_power_filter': action['power'] > 0,
                    'tracked_types': foe['types'], 'type_override': foe['type_override'],
                    'encoded_types': [ty for j, ty in enumerate(TYPES) if obs[base + 236 + j] > .5],
                    'legacy_encoded_types': [ty for j, ty in enumerate(TYPES[:18]) if obs[base + 50 + j] > .5],
                    'type_feature_values': {str(236+j): obs[base+236+j] for j in range(20)},
                    'encoded_move_types': [ty for j, ty in enumerate(TYPES[:18]) if obs[move_base+j] > .5],
                    'expected_defender_token': 6 + foe_index,
                    'actual_target_token': d['selected_action_features'][descriptor+2+k],
                    'expected_actor_token': own_index,
                    'actual_actor_token': d['selected_action_features'][descriptor+k],
                    'chosen_slot_probability': selected_prob, 'chosen_slot_rank': rank,
                    'best_slot_choices': sorted(marginals, key=lambda x: -x['probability'])[:5],
                    'top_joint_actions': d['policy_top_actions'], 'events': d['events'],
                })
    summary = {
        'provenance': 'New sampled diagnostic games using reset-run final weights; not original training decisions.',
        'decisions_with_actual_input_checked': decisions_checked,
        'tracked_vs_encoded_type_mismatches': all_mismatches,
        'known_type_immunity_events': len(examples),
        'events_passing_training_audit_power_filter': sum(e['included_by_training_audit_power_filter'] for e in examples),
        'counter_note': 'Training audit excludes base_power=0 moves, including variable-power Low Kick; this inspection also reports that omitted case.',
        'wrong_target_routes': sum(e['actual_target_token'] != e['expected_defender_token'] for e in examples),
        'wrong_actor_routes': sum(e['actual_actor_token'] != e['expected_actor_token'] for e in examples),
        'pair_counts': dict(collections.Counter(e['action']['move'] + ' -> ' + e['defender'] for e in examples)),
        'examples': examples,
    }
    output = args.evaluation / 'type-input-inspection.json'
    output.write_text(json.dumps(summary, indent=2) + '\n')
    print(json.dumps({k: v for k, v in summary.items() if k != 'examples'}, indent=2))
    for e in examples[:12]:
        print(json.dumps({k:e[k] for k in ('game','turn','action','defender','encoded_types',
                                           'encoded_move_types','chosen_slot_probability','chosen_slot_rank')}))


if __name__ == '__main__':
    main()
