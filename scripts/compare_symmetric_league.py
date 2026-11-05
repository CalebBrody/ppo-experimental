"""Compare the old asymmetric Model2 run with symmetric-search continuation."""
import argparse
import json
from pathlib import Path


def rows(path):
    return [json.loads(s) for s in (path/'training/metrics.jsonl').read_text().splitlines()]


def summarize(records):
    last=records[-1]
    recent=records[-20:]
    n=sum(r['wins']+r['losses']+r['draws'] for r in recent)
    turns=sum(r['completed_battle_turns'] for r in recent)
    outcomes=[sum(r[k] for r in records) for k in ['wins','losses','draws']]
    return {'minutes':last['seconds']/60,'update':last['update'],'games':last['games'],
            'decisions':last['decisions'],'audit':last['online_audit'],
            'score20':100*sum(r['wins']+.5*r['draws'] for r in recent)/n if n else None,
            'score20_games':n,'outcomes':outcomes,
            'score_all':100*(outcomes[0]+.5*outcomes[2])/sum(outcomes),
            'reward20':sum(r['completed_turn_reward_sum'] for r in recent)/turns if turns else None,
            'learner_simulations':sum(r.get('matrix_simulations',0) for r in records),
            'opponent_simulations':sum(r.get('opponent_matrix_simulations',0) for r in records),
            'opponent_search_positions':sum(r.get('opponent_matrix_positions',0) for r in records),
            'opponent_move_requests':sum(r.get('opponent_move_requests',0) for r in records),
            'opponent_search_fallbacks':sum(r.get('opponent_search_fallbacks',0) for r in records),
            'training_games_seconds':last['training_games_seconds'],
            'model_training_seconds':last['model_training_seconds']}


def main():
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('continuation',type=Path)
    ap.add_argument('--baseline',type=Path,default=Path('Model2/runs/matrix-history-20m-20261103'))
    args=ap.parse_args()
    old=rows(args.baseline); new=rows(args.continuation)
    first=next(i for i,r in enumerate(new) if r['online_audit']['window_samples']>=r['online_audit']['window_capacity'])
    summaries=[summarize(old),summarize(new[:first+1]),summarize(new)]
    labels=['20m, policy-only opponents','First full window, matrix opponents','3h continuation, matrix opponents']
    table=['| Metric | '+' | '.join(labels)+' |','|---|---:|---:|---:|']
    for label,key,count in [('All immunity events','immunity_per_100_attempts','observed_immunity_events'),
                            ('Known-type immunity hits','known_type_immunity_per_100_attempts','known_type_immunity_events'),
                            ('Failed moves','failed_moves_per_100_attempts','failed_move_events')]:
        cells=[]
        for s in summaries:
            a=s['audit']; c=a['counts']
            cells.append(f"{a[key]:.2f}% ({c[count]}/{c['executed_move_attempts']})")
        table.append('| '+label+' | '+' | '.join(cells)+' |')
    for label,key in [('Target-slot 2 share','slot2_share_percent'),('Preview top-two share','preview_top_two_share_percent')]:
        table.append('| '+label+' | '+' | '.join(f"{s['audit'][key]:.1f}%" for s in summaries)+' |')
    for label,bad,risk in [('Double Shock without Electric','double_shock_without_electric','double_shock_risk_states'),
                           ('Steel Roller without terrain','steel_roller_without_terrain','steel_roller_risk_states')]:
        table.append('| '+label+' (choices/risk states) | '+' | '.join(
            f"{s['audit']['counts'][bad]}/{s['audit']['counts'][risk]}" if s['audit']['counts'][risk] else 'untested' for s in summaries)+' |')
    table.append('| Training score (20 updates) | '+' | '.join(f"{s['score20']:.1f}%" for s in summaries)+' |')
    table.append('| Training score (all completed games) | '+' | '.join(f"{s['score_all']:.1f}%" for s in summaries)+' |')
    table.append('| Reward/turn (20 updates) | '+' | '.join(f"{s['reward20']:+.4f}" for s in summaries)+' |')
    table.append('| Actual elapsed training minutes | '+' | '.join(f"{s['minutes']:.2f}" for s in summaries)+' |')
    table.append('| Training games | '+' | '.join(f"{s['games']:,}" for s in summaries)+' |')
    table.append('| Learner decisions | '+' | '.join(f"{s['decisions']:,}" for s in summaries)+' |')
    notes=['All event windows contain 1,024 sampled transitions, one in four; these are descriptive training diagnostics.',
           'The continuation warm-started checkpoint 883; Adam and the league restarted. Both sides now use the same matrix-search budget.',
           'Opponent strength/distribution and training time changed. A lower training score is not evidence of weaker play; this is not held-out evaluation.',
           'Known-type counters exclude variable-base-power moves such as Low Kick. Zero observations do not establish resolution.']
    result={'labels':labels,'summaries':summaries,'limitations':notes}
    (args.continuation/'symmetric-comparison.json').write_text(json.dumps(result,indent=2)+'\n')
    content='\n'.join(table)+'\n\n'+'\n\n'.join(notes)+'\n'
    (args.continuation/'symmetric-comparison.md').write_text(content)
    print(content)


if __name__=='__main__':main()
