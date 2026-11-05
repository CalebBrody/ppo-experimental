"""Plot fixed-opponent evaluation and PPO diagnostics; no strength claim from loss."""
import argparse
import json
from collections import defaultdict, deque
from pathlib import Path
import numpy as np
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt

def bootstrap_pairs(values, rng):
    values=np.asarray(values,dtype=float)
    assert len(values)%2==0
    clusters=values.reshape(-1,2).mean(axis=1)
    samples=clusters[rng.integers(0,len(clusters),(10000,len(clusters)))].mean(axis=1)
    return np.quantile(samples,[.025,.975])

def smooth(values, width):
    return np.array([np.mean(values[max(0,i-width+1):i+1]) for i in range(len(values))])

# Fixed from fresh-capped16-5h-20261105 completed decisive samples through
# update 479: n=3030, mean=0.03257668109260278, population std=0.4451371566017857.
VALUE_SIGN_THRESHOLD = 0.22256857830089286

def winner_sign_series(metrics, sample_path):
    """Reconstruct the completed-position window; zero signs earn half credit."""
    pending = defaultdict(list)
    if sample_path.exists():
        for line in sample_path.read_text().splitlines():
            game = json.loads(line)
            if game['outcome'] != 2:
                pending[game['update']].extend(
                    dict(p, outcome=game['outcome'], last_sample=(i == len(game['predictions']) - 1))
                    for i, p in enumerate(game['predictions']))
    window = deque()
    rows = []
    for metric in metrics:
        window.extend(pending.get(metric['update'], []))
        capacity = (metric.get('value_outcome_audit') or {}).get('window_capacity', 1024)
        while len(window) > capacity:
            window.popleft()
        row = {'positions': len(window)}
        for key in ('value', 'hp_difference'):
            credit = sum(.5 if p[key] == 0 else float((p[key] > 0) == (p['outcome'] == 0)) for p in window)
            row[key] = {'correct_credit': credit,
                        'ties': sum(p[key] == 0 for p in window),
                        'accuracy_percent': 100 * credit / len(window) if window else None}
        filtered = [p for p in window if abs(p['value']) >= VALUE_SIGN_THRESHOLD]
        correct = sum((p['value'] > 0) == (p['outcome'] == 0) for p in filtered)
        row['value_filtered'] = {'positions': len(filtered), 'correct_credit': correct,
                                 'threshold': VALUE_SIGN_THRESHOLD,
                                 'accuracy_percent': 100 * correct / len(filtered) if filtered else None}
        last_samples = [p for p in window if p['last_sample']]
        for key in ('value', 'hp_difference'):
            credit = sum(.5 if p[key] == 0 else float((p[key] > 0) == (p['outcome'] == 0)) for p in last_samples)
            row[key + '_last_sample'] = {
                'positions': len(last_samples), 'correct_credit': credit,
                'ties': sum(p[key] == 0 for p in last_samples),
                'accuracy_percent': 100 * credit / len(last_samples) if last_samples else None}
        rows.append(row)
    return rows

def main():
    ap=argparse.ArgumentParser()
    ap.add_argument('run',type=Path)
    ap.add_argument('--through-update', type=int)
    ap.add_argument('--out',type=Path,default=Path('reports/progress'))
    args=ap.parse_args();args.out.mkdir(parents=True,exist_ok=True)
    metrics=[json.loads(x) for x in (args.run/'training/metrics.jsonl').read_text().splitlines()]
    if args.through_update is not None:
        metrics=[m for m in metrics if m['update'] <= args.through_update]
    if not metrics:
        metrics=[dict(update=0,decisions=0,games=0,seconds=0,wins=0,losses=0,draws=0,value_loss=0)]
    points=json.loads((args.run/'evaluation-index.json').read_text())
    rng=np.random.default_rng(37)
    last=metrics[-1]
    config_path=args.run/'training/config.json'
    config=json.loads(config_path.read_text()) if config_path.exists() else {}
    is_model2=config.get('model2',False)
    model_label='Model2 matrix search' if is_model2 else 'Model1 experimental'
    update_label='distillation' if is_model2 else 'PPO'
    league_count=last.get('league_size',max(1,len(config.get('league_weights',[]))))
    initial_opponent_only=league_count==1 and (last['league_snapshots']==0 if 'league_snapshots' in last else bool(config) and last['update']<=config.get('snapshot_every',0))
    decisions={0:0,**{m['update']:m['decisions'] for m in metrics}}
    summary={'run':str(args.run),'updates':last['update'],'decisions':last['decisions'],
             'training_games':last['games'],'training_seconds':last['seconds'],'evaluations':{},
             'self_play_wins':sum(m['wins'] for m in metrics),
             'self_play_losses':sum(m['losses'] for m in metrics),
             'self_play_draws':sum(m['draws'] for m in metrics),
             'method':'95% bootstrap intervals, resampling same-team side-swapped game pairs; draws count as half a win'}
    summary['online_audit']=last.get('online_audit')
    summary['initial_opponent_only']=initial_opponent_only
    snapshot_path=args.run/'training/league-snapshots.jsonl'
    snapshots=[json.loads(line) for line in snapshot_path.read_text().splitlines()] if snapshot_path.exists() else []
    snapshots=[s for s in snapshots if s['update'] in decisions and s['update']<=last['update']]
    summary['league_snapshot_markers']=[{**s,'decisions':decisions[s['update']]} for s in snapshots]
    if not points:
        summary['method']='Rolling ratios of event counts on randomly sampled existing learner transitions; '+('frozen initial-network opponent' if initial_opponent_only else 'changing self-play opponents')+'; not a held-out strength measurement'
    plt.rcParams.update({'font.family':'DejaVu Sans','font.size':11,'axes.spines.top':False,'axes.spines.right':False})
    has_value_audit=any('value_outcome_audit' in m for m in metrics)
    fig=plt.figure(figsize=(13,15 if has_value_audit else 12))
    grid=fig.add_gridspec(3,2,height_ratios=[1.15,1,1 if has_value_audit else .7])
    axes=np.array([[fig.add_subplot(grid[0,0]),fig.add_subplot(grid[0,1])],
                   [fig.add_subplot(grid[1,0]),fig.add_subplot(grid[1,1])]])
    reward_ax=fig.add_subplot(grid[2,0] if has_value_audit else grid[2,:])
    value_ax=fig.add_subplot(grid[2,1]) if has_value_audit else None
    colors={'random':'#007f86','initial':'#7654b5'}
    for ax,name,title in zip(axes[0],['random','initial'],['Against random legal play','Against the frozen starting model']):
        ps=sorted([p for p in points if p['opponent']==name],key=lambda p:p['update'])
        if not ps:
            x=np.array([m['decisions']/1000 for m in metrics])
            series=([('immunity_per_100_attempts','All immunity events','#007f86'),('known_type_immunity_per_100_attempts','Known type, same target','#7654b5')]
                    if name=='random' else [('failed_moves_per_100_attempts','All failed moves','#3d566e'),('double_shock_failures_per_100_attempts','Double Shock','#7654b5'),('steel_roller_failures_per_100_attempts','Steel Roller','#b85a25')])
            any_data=False
            for key,label,color in series:
                y=np.array([(m.get('online_audit') or {}).get(key) if (m.get('online_audit') or {}).get(key) is not None else np.nan for m in metrics],dtype=float)
                if np.isfinite(y).any():
                    any_data=True;ax.plot(x,y,label=label,color=color,linewidth=1.7)
            ax.set(title='Rolling immunity attacks' if name=='random' else 'Rolling failed moves',xlabel='Training decisions (thousands)',ylabel='Events / 100 sampled move attempts')
            ax.set_ylim(bottom=0);ax.grid(alpha=.15)
            if any_data:ax.legend(frameon=False,fontsize=9)
            else:ax.text(.5,.5,'Awaiting sampled training transitions',transform=ax.transAxes,ha='center',va='center',fontsize=10)
            continue
        values=[];cis=[];xs=[];reports=[]
        for p in ps:
            report=json.loads(Path(p['report']).read_text());reports.append(report)
            scores=[r['score'] for r in report['results']]
            values.append(np.mean(scores)*100);cis.append(bootstrap_pairs(scores,rng)*100)
            xs.append(decisions[p['update']]/1000)
        values=np.asarray(values);cis=np.asarray(cis)
        ax.errorbar(xs,values,yerr=np.stack([values-cis[:,0],cis[:,1]-values]),color=colors[name],marker='o',linewidth=2,capsize=4)
        ax.axhline(50,color='#99a3ad',linestyle='--',linewidth=1)
        ax.set(title=title,xlabel='Training decisions (thousands)',ylabel='Evaluation score (%)',ylim=(0,100))
        ax.grid(alpha=.15)
        first,last_report=reports[0],reports[-1]
        assert [(r['match_seed'],r['learner_side'],r['team_a'],r['team_b']) for r in first['results']]==[(r['match_seed'],r['learner_side'],r['team_a'],r['team_b']) for r in last_report['results']]
        deltas=[b['score']-a['score'] for a,b in zip(first['results'],last_report['results'])]
        delta=float(np.mean(deltas)*100);ci=bootstrap_pairs(deltas,rng)*100
        summary['evaluations'][name]={'initial_score_percent':values[0], 'final_score_percent':values[-1],
            'change_percentage_points':delta,'change_ci95':ci.tolist(),'initial_games':first['games'],'final_games':last_report['games'],
            'final_wins':last_report['wins'],'final_losses':last_report['losses'],'final_draws':last_report['draws']}
        ax.text(.03,.06,f'{values[0]:.1f}% → {values[-1]:.1f}%\nChange {delta:+.1f} points (95% CI {ci[0]:+.1f} to {ci[1]:+.1f})',transform=ax.transAxes,va='bottom',fontsize=10,
                bbox={'facecolor':'white','alpha':.9,'edgecolor':'none'})
    x=np.array([m['decisions']/1000 for m in metrics])
    ax=axes[1,0]
    rolling=[]
    for i in range(len(metrics)):
        ms=metrics[max(0,i-19):i+1]
        games=sum(m['wins']+m['losses']+m['draws'] for m in ms)
        rolling.append(100*sum(m['wins']+.5*m['draws'] for m in ms)/games if games else np.nan)
    ax.plot(x,rolling,color='#3d566e',linewidth=1.8)
    ax.axhline(50,color='#99a3ad',linestyle='--',linewidth=1)
    ax.set(title='Training score vs sampled frozen league opponents',xlabel='Training decisions (thousands)',ylabel='20-update rolling score (%)',ylim=(0,100))
    ax.grid(alpha=.15)
    note=(('Matrix search on both sides; mixed frozen opponents.' if config.get('matrix_opponents',False)
           else 'Search-assisted learner vs frozen neural policies.') if is_model2 else
          'Training-team games; not a held-out strength test.' if initial_opponent_only else
          'Mixed frozen opponents; no automatic reset to 50%.')
    ax.text(.02,.06,note,transform=ax.transAxes,fontsize=9)
    ax=axes[1,1]
    vals=[m['value_loss'] for m in metrics]
    ax.plot(x,vals,color='#e1aa60',alpha=.3,linewidth=.8)
    ax.plot(x,smooth(vals,20),color='#ac681a',linewidth=1.8,label='20-update average')
    ax.set(title='Value prediction loss',xlabel='Training decisions (thousands)',ylabel=f'{update_label} value loss')
    ax.grid(alpha=.15);ax.legend(frameon=False,fontsize=9)
    sign_rows=winner_sign_series(metrics,args.run/'training/value-outcome-samples.jsonl')
    reward_ax.set(title='Winner prediction from value sign',xlabel='Training decisions (thousands)',ylabel='Rolling winner accuracy (%)',ylim=(0,100))
    reward_ax.axhline(50,color='#99a3ad',linewidth=1,linestyle='--');reward_ax.grid(alpha=.15)
    if any(row['positions'] for row in sign_rows):
        for key,label,color in [('value','Learned value sign','#007f86'),('hp_difference','HP-pool sign','#7654b5'),('value_filtered',f'Value sign, |V| >= {VALUE_SIGN_THRESHOLD:.3f} (0.5 SD)','#b85a25')]:
            reward_ax.plot(x,[row[key]['accuracy_percent'] if row[key]['accuracy_percent'] is not None else np.nan for row in sign_rows],color=color,linewidth=1.8,label=label)
        for key,label,color in [('value_last_sample','Value: last sampled position/game','#007f86'),('hp_difference_last_sample','HP: last sampled position/game','#7654b5')]:
            reward_ax.plot(x,[row[key]['accuracy_percent'] if row[key]['accuracy_percent'] is not None else np.nan for row in sign_rows],color=color,linewidth=1.8,linestyle='--',label=label)
        reward_ax.legend(frameon=False,fontsize=8,loc='lower right')
        reward_ax.text(.02,.34,'Last sampled = approximation to final turn.\nZero sign = 1/2 credit; decisive games only.',transform=reward_ax.transAxes,fontsize=8)
    else:
        reward_ax.text(.5,.5,'No completed value-outcome samples recorded',transform=reward_ax.transAxes,ha='center',va='center')
    summary['winner_sign_accuracy']={**sign_rows[-1], 'method':'Same rolling completed decisive window; filtered value uses only positions with abs(value) >= fixed threshold within that window; last-sample lines select the final recorded prediction per game within the same window, not necessarily the final turn; zero sign earns half credit; draws excluded.'}
    if value_ax is not None:
        for key,label,color in [('all_disagreements','All disagreements','#007f86'),
                ('hp_behind_value_positive','HP behind, value positive','#7654b5'),
                ('hp_ahead_value_negative','HP ahead, value negative','#b85a25'),
                ('material_disagreements_hp10pp_value03','Strong: |HP| ≥ .1, |V| ≥ .3','#3d566e')]:
            values=[((m.get('value_outcome_audit') or {}).get(key) or {}).get('accuracy_percent') for m in metrics]
            value_ax.plot(x,[v if v is not None else np.nan for v in values],label=label,color=color,linewidth=1.6)
        value_ax.axhline(50,color='#999',linestyle='--',linewidth=1)
        value_ax.set(title='Value correct when it disagrees with HP',xlabel='Training decisions (thousands)',ylabel='Rolling winner accuracy (%)',ylim=(0,100))
        value_ax.grid(alpha=.15);value_ax.legend(frameon=False,fontsize=8)
        summary['value_outcome_audit']=last.get('value_outcome_audit')
    # Keep long-run snapshot labels readable while retaining every marker in JSON.
    marker_stride=max(1, (len(snapshots)+8)//9)
    displayed_snapshots=snapshots[marker_stride-1::marker_stride]
    if snapshots and (not displayed_snapshots or displayed_snapshots[-1] != snapshots[-1]):
        displayed_snapshots.append(snapshots[-1])
    for s in displayed_snapshots:
        position=decisions[s['update']]/1000
        for panel in [*axes.flat,reward_ax,*([value_ax] if value_ax is not None else [])]:
            panel.axvline(position,color='#74808b',linestyle=':',alpha=.8,linewidth=1,zorder=0)
        text=f"{s['seconds']/60:.0f}m" + (' final' if s.get('reason')=='final' else '')
        axes[1,0].text(position,.98,text,transform=axes[1,0].get_xaxis_transform(),
                       ha='right',va='top',rotation=90,fontsize=8,color='#596570')
    recovery=args.run/'recovery-ledger.json'
    if recovery.exists():
        restart=json.loads(recovery.read_text())
        for panel in [*axes.flat,reward_ax,*([value_ax] if value_ax is not None else [])]:
            panel.axvline(restart['retained_decisions']/1000,color='#b85a25',linestyle='--',alpha=.8,linewidth=1)
    fig.suptitle(f'{model_label} · {last["seconds"]/60:.1f} minutes of RTX 2060 training',fontsize=19,fontweight='bold',x=.06,ha='left',y=.99)
    fig.text(.06,.945,f'{last["games"]:,} training games · {last["decisions"]:,} learner decisions · {last["update"]} {update_label} updates',fontsize=12,color='#4a5563')
    footer=('Held-out teams; matching seeded setups with sides swapped. Score = wins + 1/2 draws. Bars: 95% paired-cluster bootstrap intervals.\nEqual game counts per checkpoint; see summary.json for counts. One training seed; this is an early learning check, not an Elo estimate.' if points else
            f'Online diagnostics only: no evaluation games. Window: last {(last.get("online_audit") or {}).get("window_capacity",2048):,} sampled learner transitions. Ratios aggregate rolling counts.\n'+('Opponent bank still contains only the frozen initial network. ' if initial_opponent_only else 'Changing self-play opponents. ')+'Not a held-out strength estimate; see summary.json and metrics.jsonl for denominators.')
    if snapshots:
        footer+='\nDotted lines: selected actual league snapshots (all retained in JSON); final save has no subsequent games.'
    if recovery.exists():
        footer+=' Orange dashed line: recovery restart; unsaved tail excluded (see recovery-ledger.json).'
    fig.text(.06,.015,footer,fontsize=9,color='#4a5563')
    fig.tight_layout(rect=[.025,.065,.99,.92])
    fig.savefig(args.out/'progress.png',dpi=170,facecolor='white')
    fig.savefig(args.out/'progress.svg',facecolor='white')
    (args.out/'summary.json').write_text(json.dumps(summary,indent=2))
    print(json.dumps(summary,indent=2))

if __name__=='__main__':main()
