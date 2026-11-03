"""Summarize paired, side-swapped team-order evaluation runs."""
import json
from pathlib import Path
import numpy as np

root = Path(__file__).resolve().parents[1] / 'reports/team-order-test'
names = ['original', 'shuffle-1', 'shuffle-2', 'shuffle-3']
reports = [json.loads((root / name / 'evaluation.json').read_text()) for name in names]
scores = np.array([[r['score'] for r in report['results']] for report in reports])
for report in reports[1:]:
    assert [(r['match_seed'], r['learner_side'], r['team_a'], r['team_b']) for r in report['results']] == [(r['match_seed'], r['learner_side'], r['team_a'], r['team_b']) for r in reports[0]['results']]
pairs = scores.reshape(4, -1, 2).mean(axis=2)
rng = np.random.default_rng(37)
samples = rng.integers(0, pairs.shape[1], (10000, pairs.shape[1]))
summary = []
for i, name in enumerate(names):
    delta = pairs[i] - pairs[0]
    ci = np.quantile(delta[samples].mean(axis=1), [.025, .975]) * 100
    summary.append(dict(variant=name, wins=reports[i]['wins'], losses=reports[i]['losses'], draws=reports[i]['draws'], score_percent=scores[i].mean()*100, difference_points=delta.mean()*100, difference_95_ci=ci.tolist()))
delta = pairs[1:].mean(axis=0) - pairs[0]
result = dict(results=summary, pooled_shuffle_score_percent=scores[1:].mean()*100, pooled_difference_points=delta.mean()*100, pooled_difference_95_ci=(np.quantile(delta[samples].mean(axis=1), [.025,.975])*100).tolist(), note='Bootstrap over matched seed pairs, retaining dependence across shuffles. Both team rosters reordered before preview; this is not a same-state prediction invariance test.')
(root / 'summary.json').write_text(json.dumps(result, indent=2))
print(json.dumps(result, indent=2))
