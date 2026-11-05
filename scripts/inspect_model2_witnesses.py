"""Summarize retained concrete Model2 mistakes; these are not a rate estimate."""
import argparse
import collections
import json
from pathlib import Path
from check_run import analyze_game


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('run', type=Path)
    args = parser.parse_args()
    raw = json.loads((args.run / 'dex.json').read_text())
    dex = {'moves': {m['id']: m for m in raw['moves']}, 'typechart': raw['typechart']}
    witnesses = [json.loads(line) for line in (args.run / 'training/mistake-examples.jsonl').read_text().splitlines()]
    cases = []
    failures = collections.Counter()
    terminal_losses = []
    for witness in witnesses:
        search = witness['search']
        chosen = search['chosen_candidate']
        decision = {'turn': search['turn'], 'request': 'Move', 'state': search['state'],
                    'selected': search['candidates'][chosen], 'events': witness['events']}
        counts, examples, _ = analyze_game({'game': witness['decision'], 'learner_side': search['side'],
                                            'outcome': 0, 'decisions': [decision]}, dex)
        failures.update({k: v for k, v in counts.items() if k.startswith('failed_move:')})
        if witness.get('game_outcome')==1 or '|win|p' + str(2-search['side']) in witness['events']:
            terminal_losses.append(witness['decision'])
        for example in examples:
            if example['kind'] in ['known_type_immunity_event', 'double_shock_without_electric', 'steel_roller_without_terrain']:
                example.pop('game', None)  # This identifier is a global learner decision, not a game ID.
                cases.append({**example, 'update': witness['update'], 'decision': witness['decision'],
                              'game': witness.get('game'), 'game_outcome': witness.get('game_outcome'),
                              'chosen_probability': search['policy'][chosen],
                              'chosen_joint_action': search['candidates'][chosen],
                              'candidate_probabilities': search['policy'],
                              'candidate_min_payoffs': [min(row) for row in search['payoff']],
                              'public_state': search['state'], 'events': witness['events']})
    report = {'retained_witnesses': len(witnesses), 'cases': cases,
              'failure_types_within_witnesses': dict(failures),
              'terminal_loss_decisions': terminal_losses,
              'limitation': ('Rolling last 128 witnesses, saved at snapshots; unfinished games have unknown outcomes. '
                            if witnesses and 'game_outcome' in witnesses[0] else
                            'First 128 witnesses; nonterminal cases do not retain eventual outcomes. ')
                            + 'Not representative rates; use online audit for rates.'}
    (args.run / 'witness-summary.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({k: v for k, v in report.items() if k != 'cases'}, indent=2))
    print('Case kinds:', dict(collections.Counter(c['kind'] for c in cases)))


if __name__ == '__main__':
    main()
