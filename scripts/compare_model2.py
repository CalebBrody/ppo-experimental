"""Compare Model2 online training diagnostics with preserved Model1 runs."""
import argparse
import json
from pathlib import Path


def read_run(path):
    rows = [json.loads(line) for line in (path / 'training/metrics.jsonl').read_text().splitlines()]
    last = rows[-1]
    window = rows[-20:]
    turns = sum(r['completed_battle_turns'] for r in window)
    return {'path': str(path.resolve()), 'seconds': last['seconds'], 'games': last['games'],
            'updates': last['update'], 'decisions': last['decisions'],
            'audit': last['online_audit'],
            'reward_per_turn_last20updates': sum(r['completed_turn_reward_sum'] for r in window) / turns if turns else None,
            'matrix_positions': sum(r.get('matrix_positions', 0) for r in rows),
            'matrix_simulations': sum(r.get('matrix_simulations', 0) for r in rows),
            'unsearched_decisions': sum(r.get('unsearched_preview_or_replacement_decisions', 0) for r in rows),
            'mean_local_duality_gap': sum((r.get('mean_matrix_duality_gap') or 0) * r.get('matrix_positions', 0) for r in rows)
                / max(1, sum(r.get('matrix_positions', 0) for r in rows)),
            'training_games_seconds': last['training_games_seconds'],
            'model_training_seconds': last['model_training_seconds']}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('model2', type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    runs = [read_run(root / 'runs/fresh-continuation-30m-20261103'),
            read_run(root / 'runs/current-types-20m-20261103'),
            read_run(root / 'runs/current-types-reset-20m-20261103'), read_run(args.model2)]
    labels = ['Before type changes', 'Types, continued', 'Types, reset', 'Model2, reset']
    table = ['| Metric | ' + ' | '.join(labels) + ' |', '|---|' + '---:|' * 4]
    for label, key, count in [
        ('All immunity events', 'immunity_per_100_attempts', 'observed_immunity_events'),
        ('Known-type immunity hits', 'known_type_immunity_per_100_attempts', 'known_type_immunity_events'),
        ('Failed moves', 'failed_moves_per_100_attempts', 'failed_move_events')]:
        cells = []
        for run in runs:
            a = run['audit']; n = a['counts']
            cells.append(f"{a[key]:.2f}% ({n[count]}/{n['executed_move_attempts']})")
        table.append('| ' + label + ' | ' + ' | '.join(cells) + ' |')
    table.append('| Target-slot 2 share | ' + ' | '.join(f"{r['audit']['slot2_share_percent']:.1f}%" for r in runs) + ' |')
    for label, bad, risk in [
        ('Double Shock without Electric (choices/risk states)', 'double_shock_without_electric', 'double_shock_risk_states'),
        ('Steel Roller without terrain (choices/risk states)', 'steel_roller_without_terrain', 'steel_roller_risk_states')]:
        table.append('| ' + label + ' | ' + ' | '.join(
            f"{r['audit']['counts'][bad]}/{r['audit']['counts'][risk]}" if r['audit']['counts'][risk] else 'untested (0 risk states)' for r in runs) + ' |')
    table.append('| Average reward/turn (20 updates) | ' + ' | '.join(f"{r['reward_per_turn_last20updates']:+.4f}" for r in runs) + ' |')
    table.append('| Training games | ' + ' | '.join(f"{r['games']:,}" for r in runs) + ' |')
    table.append('| Learner decisions | ' + ' | '.join(f"{r['decisions']:,}" for r in runs) + ' |')
    notes = [
        'Each event window contains 1,024 randomly sampled learner transitions, one in four. Counts are not held-out strength estimates.',
        'Model2 diagnostics include matrix search when selecting actions; historical Model1 diagnostics use the sampled neural policy alone.',
        'Model2 also includes direct candidate move-type features (145,858 parameters); the historical runs have 143,298 parameters.',
        'The first run was stopped at 22.3 minutes; the remaining runs have 20-minute budgets. Continued weights have more prior training.',
        'Reward windows contain 20 updates, with different decisions/update and opponent mixtures; reward differences do not establish strength.',
        'The common known-type training counter excludes variable-base-power moves such as Low Kick; it undercounts those cases.',
    ]
    result = {'labels': labels, 'runs': runs, 'limitations': notes}
    (args.model2 / 'comparison.json').write_text(json.dumps(result, indent=2) + '\n')
    content = '\n'.join(table) + '\n\n' + '\n\n'.join(notes) + '\n'
    (args.model2 / 'changes-table.md').write_text(content)
    # Compare progress on the common wall-clock budget, using recorded samples.
    import matplotlib
    matplotlib.use('Agg')
    import matplotlib.pyplot as plt
    fig, axes = plt.subplots(1, 3, figsize=(15, 4.5))
    for path, label, color in [
        (root / 'runs/current-types-reset-20m-20261103', 'Model1, reset', '#7654b5'),
        (args.model2, 'Model2, reset (search-assisted)', '#007f86')]:
        rows = [json.loads(line) for line in (path / 'training/metrics.jsonl').read_text().splitlines()]
        x = [r['seconds'] / 60 for r in rows]
        for ax, key, title in zip(axes[:2],
                ['known_type_immunity_per_100_attempts', 'failed_moves_per_100_attempts'],
                ['Known-type immunity hits', 'Failed moves']):
            ax.plot(x, [r['online_audit'][key] for r in rows], label=label, color=color)
            ax.set(title=title, xlabel='Training minutes', ylabel='Events / 100 executed attempts')
        reward = []
        for i in range(len(rows)):
            window = rows[max(0, i-19):i+1]
            turns = sum(r['completed_battle_turns'] for r in window)
            reward.append(sum(r['completed_turn_reward_sum'] for r in window) / turns if turns else float('nan'))
        axes[2].plot(x, reward, label=label, color=color)
    axes[2].set(title='Average reward per completed turn', xlabel='Training minutes', ylabel='Reward / turn (20 updates)')
    for ax in axes:
        ax.grid(alpha=.15)
        ax.spines[['top', 'right']].set_visible(False)
    axes[0].legend(fontsize=8, frameon=False)
    fig.suptitle('Model1 vs Model2: equal training-time budgets, changing frozen opponents', fontsize=14)
    fig.text(.02, .015, 'Online training diagnostics, not held-out strength. Model2 also changes action encoding. Reward windows cover different numbers of decisions.', fontsize=9)
    fig.tight_layout(rect=[0, .055, 1, .94])
    fig.savefig(args.model2 / 'comparison.png', dpi=170, facecolor='white')
    print(content)


if __name__ == '__main__':
    main()
