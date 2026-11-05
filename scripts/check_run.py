"""Audit evaluation traces and training health; never infer correctness from win rate.

python scripts/check_run.py RUN_OR_EVALUATION_DIRECTORY
Writes issue-check.json. Missing traces are reported as untested, not clean.
--fail-on-issues is useful for CI; ordinary audits always produce a report.
"""
import argparse
import collections
import json
import math
import re
from pathlib import Path
from summarize_timing import summarize as summarize_timing


def identifier(text):
    return ''.join(c.lower() for c in text if c.isalnum())


def analyze_game(game, dex):
    counts = collections.Counter(games=1)
    examples = []
    previews = []
    seen = set()
    learner = 'p' + str(game['learner_side'] + 1)
    for decision in game['decisions']:
        immunity_candidates = {}
        chosen = decision.get('selected')
        state = decision.get('state', {})
        if decision.get('request') == 'Move':
            for mon in state.get('sides', [[], []])[0]:
                if mon.get('slot') is None or mon.get('fainted') or mon.get('hp', 0) == 0:
                    continue
                moves = {m['move'] for m in mon.get('moves', []) if (m.get('pp') or 0) > 0}
                if 'Double Shock' in moves and mon.get('types') and 'Electric' not in mon['types']:
                    counts['double_shock_risk_states'] += 1
                if 'Steel Roller' in moves and state.get('terrain') is None:
                    counts['steel_roller_risk_states'] += 1
        if isinstance(chosen, dict) and 'preview' in chosen:
            previews.append(','.join(str(x['index']) for x in chosen['preview']))
        if isinstance(chosen, list):
            own, foes = state.get('sides', [[], []])
            for action in chosen:
                if 'switch' in action:
                    counts['voluntary_switches' if decision['request'] == 'Move' else 'forced_switches'] += 1
                if 'move' not in action:
                    continue
                counts['selected_moves'] += 1
                target = action.get('target', 0)
                counts['target_' + str(target)] += 1
                if action.get('accuracy') is not None and action['accuracy'] < 80:
                    counts['low_accuracy_selections'] += 1
                if action['move'] in ('Protect', 'Detect'):
                    counts['protect_selections'] += 1
                actor = next((m for m in own if m.get('slot') == action.get('actor_slot')), {})
                live_foes = [m for m in foes if m.get('slot') is not None and not m.get('fainted') and m.get('hp', 0) > 0]
                if action.get('both_foe_targets_legal') and len(live_foes) == 2:
                    counts['dual_target_opportunities'] += 1
                    counts['dual_target_' + str(target)] += 1
                flags = []
                if action['move'] == 'Double Shock' and actor.get('types') and 'Electric' not in actor['types']:
                    flags.append('double_shock_without_electric')
                if action['move'] == 'Steel Roller' and state.get('terrain') is None:
                    flags.append('steel_roller_without_terrain')
                if action['move'] in ('Fake Out', 'First Impression') and action.get('active_move_actions', 0) > 0:
                    flags.append('first_turn_move_after_first_action')
                if target < 0 and action.get('power') and action['move'] not in ('Beat Up', 'Pollen Puff'):
                    flags.append('ally_damage_candidate')
                defender = next((m for m in foes if m.get('slot') == target - 1 and not m.get('fainted')), {}) if target > 0 else {}
                move = dex.get('moves', {}).get(identifier(action['move']), {})
                move_type = action.get('type')
                ability = identifier(actor.get('ability') or '')
                converters = {'pixilate':'Fairy','aerilate':'Flying','refrigerate':'Ice','galvanize':'Electric'}
                if move_type == 'Normal' and ability in converters:
                    move_type = converters[ability]
                if ability == 'normalize':
                    move_type = 'Normal'
                bypass = move.get('ignoreImmunity') is True or action['move'] in ('Thousand Arrows', 'Flying Press', 'Revelation Dance', 'Weather Ball', 'Tera Blast', 'Judgment', 'Multi-Attack', 'Techno Blast')
                if move_type in ('Normal', 'Fighting') and ability in ('scrappy', 'mindseye'):
                    bypass = True
                # Candidate only: public items, Gravity and other exceptional effects
                # can bypass type immunity. Actual protocol immunities are counted separately.
                if not bypass and action.get('category') in ('Physical', 'Special') and any(
                        dex.get('typechart', {}).get(ty, {}).get(move_type) == 3 for ty in defender.get('types', [])):
                    flags.append('visible_type_immunity_candidate')
                    immunity_candidates[(action.get('actor_slot'), action['move'], target-1)] = defender.get('name')
                for flag in flags:
                    counts[flag] += 1
                    seen.add(flag)
                    if len(examples) < 30:
                        examples.append(dict(kind=flag, game=game['game'], turn=decision['turn'], action=action, defender=defender.get('species')))
        last_move = None
        for line in decision.get('events', []):
            parts = line.split('|')
            if len(parts) > 4 and parts[1] == 'move':
                last_move = parts
                if parts[2].startswith(learner):
                    counts['executed_move_attempts'] += 1
            if len(parts) > 2 and parts[1] == '-immune' and last_move and last_move[2].startswith(learner):
                counts['observed_immunity_events'] += 1
                seen.add('observed_immunity_events')
                if len(examples) < 30:
                    examples.append(dict(kind='observed_immunity_event', game=game['game'], turn=decision['turn'], move=last_move[3], target=parts[2], line=line))
                source_ident, target_ident = last_move[2], parts[2]
                if len(source_ident) > 2 and len(target_ident) > 2 and target_ident.startswith('p'+str(2-game['learner_side'])):
                    key=(ord(source_ident[2])-ord('a'), last_move[3], ord(target_ident[2])-ord('a'))
                    name=target_ident.partition(': ')[2]
                    if key in immunity_candidates and name and name == immunity_candidates[key]:
                        counts['known_type_immunity_events'] += 1
                        seen.add('known_type_immunity_events')
                        if len(examples) < 30:
                            examples.append(dict(kind='known_type_immunity_event',game=game['game'],turn=decision['turn'],move=last_move[3],target=target_ident,line=line))
            if len(parts) > 2 and parts[1] == '-fail' and last_move and last_move[2].startswith(learner) and parts[2] == last_move[2]:
                # Do not count failed Intimidate/unboost ability messages as failed moves.
                if len(parts) == 3 or parts[3].startswith('move:'):
                    counts['failed_move_events'] += 1
                    counts['failed_move:' + last_move[3]] += 1
            if len(parts) > 2 and parts[1] == '-miss' and parts[2].startswith(learner):
                counts['miss_events'] += 1
    for flag in seen:
        counts['games_with_' + flag] += 1
        if game.get('outcome') == 1:
            counts['lost_games_with_' + flag] += 1
    return counts, examples, previews


def inspect_evaluation(directory, dex):
    result = json.loads((directory / 'evaluation.json').read_text())
    traces = sorted((directory / 'traces').glob('game-*.json'))
    totals = collections.Counter()
    previews = collections.Counter()
    examples, per_game, teams = [], [], set()
    team_counts = collections.defaultdict(collections.Counter)
    expected = {row['game']: row for row in result['results']}
    errors = []
    traced_ids = set()
    for path in traces:
        game = json.loads(path.read_text())
        if game['game'] in traced_ids:
            errors.append(f"Duplicate trace game: {game['game']}")
        traced_ids.add(game['game'])
        counts, samples, orders = analyze_game(game, dex)
        totals.update(counts)
        previews.update(orders)
        for sample in samples:
            sample['outcome'] = game['outcome']
        examples.extend(samples)
        per_game.append(dict(game=game['game'], outcome=game['outcome'], counts=dict(counts)))
        team = game['team_a'] if game['learner_side'] == 0 else game['team_b']
        teams.add(team)
        team_counts[team]['games'] += 1
        team_counts[team][{0:'wins',1:'losses',2:'draws'}[game['outcome']]] += 1
        row = expected.get(game['game'])
        score = {0:1., 1:0., 2:0.5}[game['outcome']]
        if row is None or row['score'] != score:
            errors.append(f"Trace outcome mismatch: game {game['game']}")
        if row is not None:
            for field in ['match_seed','learner_side','team_a','team_b']:
                if field in row and game.get(field) != row[field]:
                    errors.append(f"Trace setup mismatch ({field}): game {game['game']}")
        for decision in game['decisions']:
            if decision.get('selected_index', 0) >= decision.get('legal_actions', 1):
                errors.append(f"Illegal selected action: game {game['game']}")
    if traced_ids != set(expected):
        errors.append(f'Trace game coverage mismatch: {len(traced_ids)} unique traces, {len(expected)} expected')
    complete = len(traces) == result['games'] and not errors
    opportunities = totals['dual_target_opportunities']
    dominant = max(totals['dual_target_1'], totals['dual_target_2']) / opportunities if opportunities else None
    preview_share = sum(n for _, n in previews.most_common(2)) / sum(previews.values()) if previews else None
    collapse = opportunities >= 100 and dominant >= .95
    preview_collapse = len(teams) >= 10 and len(traces) >= 50 and preview_share >= .98
    status = lambda n: 'observed' if n else ('not_observed' if complete else 'untested')
    by_kind = collections.defaultdict(list)
    for sample in sorted(examples, key=lambda x: x['outcome'] != 1):
        if len(by_kind[sample['kind']]) < 10:
            by_kind[sample['kind']].append(sample)
    issues = {
        'immunity_attacks': status(totals['observed_immunity_events']),
        'target_collapse': 'observed' if collapse else ('not_observed' if complete and opportunities >= 100 else 'insufficient_data'),
        'preview_index_collapse': 'observed' if preview_collapse else ('not_observed' if complete and len(teams) >= 10 and len(traces) >= 50 else 'insufficient_data'),
        'double_shock_without_electric': status(totals['double_shock_without_electric']),
        'steel_roller_without_terrain': status(totals['steel_roller_without_terrain']),
        'first_turn_move_after_first_action': status(totals['first_turn_move_after_first_action']),
    }
    for issue,exposure in [('double_shock_without_electric','double_shock_risk_states'),('steel_roller_without_terrain','steel_roller_risk_states')]:
        if not totals[issue] and complete and not totals[exposure]:
            issues[issue] = 'not_exposed'
    return dict(directory=str(directory), weights=result.get('weights'), greedy=result.get('greedy'),
                games=result['games'], wins=result['wins'], losses=result['losses'], draws=result['draws'],
                trace_complete=complete, errors=errors, counts=dict(totals), issues=issues,
                dominant_target_share=dominant, top_two_preview_share=preview_share,
                preview_index_orders=dict(previews),
                team_breakdown=sorted([dict(team_index=t,**dict(n)) for t,n in team_counts.items()],key=lambda r:-r.get('losses',0)),
                immunity_events_per_100_attempts=100*totals['observed_immunity_events']/max(1, totals['executed_move_attempts']),
                examples=[x for group in by_kind.values() for x in group],
                examples_by_kind=dict(by_kind), per_game=per_game)


def online_summary(metrics):
    audit=next((row['online_audit'] for row in reversed(metrics) if row.get('online_audit')),None)
    if audit is None:
        return None
    counts=audit['counts'];n=audit.get('window_samples',0)
    status=lambda key:'observed' if counts.get(key,0) else ('not_observed_in_sample' if n else 'untested')
    dual=counts.get('dual_target_1',0)+counts.get('dual_target_2',0)
    share=max(counts.get('dual_target_1',0),counts.get('dual_target_2',0))/dual if dual else 0
    issues={key:status(key) for key in ['observed_immunity_events','double_shock_without_electric','steel_roller_without_terrain','first_turn_move_after_first_action']}
    issues['target_collapse']='observed' if dual>=100 and share>=.95 else ('not_observed_in_sample' if dual>=100 else 'insufficient_data')
    issues['preview_index_collapse']='observed' if counts.get('previews',0)>=50 and (audit.get('preview_top_two_share_percent') or 0)>=98 else ('not_observed_in_sample' if counts.get('previews',0)>=50 else 'insufficient_data')
    return dict(window=audit,issues=issues,source='Sampled existing learner transitions; no extra evaluation games',
                limitation='Changing self-play opponents and rolling samples; not a fixed-opponent strength estimate. Missing events are not proof of resolution.')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('path', type=Path)
    parser.add_argument('--dex', type=Path)
    parser.add_argument('--out', type=Path)
    parser.add_argument('--fail-on-issues', action='store_true')
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    path = args.path.resolve()
    dex_path = args.dex or (path/'dex.json' if (path/'dex.json').exists() else root/'vendor/Ninjask/data/derived/format/gen9championsvgc2026regmc.json')
    raw = json.loads(dex_path.read_text())
    dex = {'moves':{m['id']:m for m in raw['moves']},'typechart':raw['typechart']}
    paths = [path/'evaluation.json'] if (path/'evaluation.json').exists() else sorted(path.rglob('evaluation.json'))
    evaluations = [inspect_evaluation(p.parent, dex) for p in paths if 'source' not in p.relative_to(path).parts]
    metrics_path = path/'training/metrics.jsonl'
    if not metrics_path.exists():
        metrics_path=path/'metrics.jsonl'
    metrics = [json.loads(line) for line in metrics_path.read_text().splitlines()] if metrics_path.exists() else []
    training_errors = []
    for row in metrics:
        if not all(math.isfinite(row[key]) for key in ('policy_loss','value_loss','entropy','kl')):
            training_errors.append(f"Nonfinite losses at update {row['update']}")
    status_path = path/'status.json'
    run_state = json.loads(status_path.read_text()) if status_path.exists() else {}
    run_status = run_state.get('status')
    if status_path.exists() and run_status != 'completed':
        training_errors.append('Run is not completed: '+str(run_status))
    if status_path.exists() and not metrics:
        training_errors.append('Missing training metrics')
    if metrics:
        if metrics[-1]['seconds'] < run_state.get('duration_seconds',0):
            training_errors.append('Training stopped before the requested duration')
        outcomes = sum(row.get('wins',0)+row.get('losses',0)+row.get('draws',0) for row in metrics)
        if outcomes != metrics[-1]['games']:
            training_errors.append('Outcome counts do not sum to completed games')
    source = path/'source/src/env.rs' if (path/'source/src/env.rs').exists() else root/'src/env.rs'
    encode = source.read_text().split('pub fn encode_state',1)[-1].split('pub fn observe',1)[0]
    type_gap = 'm.types' not in encode or 'm.added_type' not in encode
    source_text = source.read_text()
    reward_delta = bool(re.search(r'hp_advantage\(b,\s*side\)\s*-\s*previous_advantage', source_text))
    source_root = source.parent
    value_source = (source_root/'model.rs').read_text()
    value_assignment = value_source.split('let value =',1)[-1].split(';',1)[0]
    value_head_unbounded = '.tanh()' not in value_assignment
    main_source = (source_root/'main.rs').read_text()
    outcome_metrics_independent = 'previous_outcomes' in main_source and '"wins":completed[0]' in main_source.replace(' ', '')
    report = dict(schema_version=1, evaluations=evaluations, training_errors=training_errors,
                  online_training_audit=online_summary(metrics),
                  timing=summarize_timing(path),
                  run_status=run_status, tracked_type_encoding_gap=type_gap,
                  source_checks={'hp_reward_is_progress_delta':reward_delta,
                                 'value_head_unbounded':value_head_unbounded,
                                 'outcome_metrics_independent_of_reward_sign':outcome_metrics_independent},
                  thresholds={'target_collapse_share':.95,'minimum_dual_target_opportunities':100,'preview_top_two_share':.98,'minimum_preview_teams':10,'minimum_preview_games':50},
                  limitations=['Observed immunity includes switches, abilities and status moves; it is not automatically an avoidable choice.',
                               'Visible type-immunity flags are review candidates with possible exceptional mechanics.',
                               'No observations in a finite sample does not prove an issue is resolved.',
                               'Risk-state counts require an active Pokemon with the move and PP, but do not guarantee legality under Encore/Disable.',
                               'The type-encoding check is a source heuristic and requires review if the encoder is refactored.'])
    output = args.out or path/'issue-check.json'
    output.write_text(json.dumps(report,indent=2)+'\n')
    online=report['online_training_audit']
    print(json.dumps({'report':str(output),'evaluations':len(evaluations),'training_errors':training_errors,'tracked_type_encoding_gap':type_gap,'online_issues':online['issues'] if online else None,'online_window_samples':online['window']['window_samples'] if online else None,'results':[{'path':r['directory'],'games':r['games'],'trace_complete':r['trace_complete'],'issues':r['issues']} for r in evaluations]},indent=2))
    coverage=bool(evaluations) or bool(online and online['window'].get('window_samples',0)>0)
    flagged = training_errors or type_gap or not all(report['source_checks'].values()) or not coverage or bool(online and 'observed' in online['issues'].values()) or any(not r['trace_complete'] or any(v != 'not_observed' for v in r['issues'].values()) for r in evaluations)
    if args.fail_on_issues and flagged:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
