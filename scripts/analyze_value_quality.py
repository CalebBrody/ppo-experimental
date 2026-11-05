"""Compare fixed-checkpoint neural values to realized returns and game winners."""
import argparse
import json
from pathlib import Path
import numpy as np


def auc(scores, labels):
    pos=np.asarray(scores)[np.asarray(labels)==1]
    neg=np.asarray(scores)[np.asarray(labels)==0]
    if not len(pos) or not len(neg):return None
    return float(((pos[:,None]>neg).sum()+.5*(pos[:,None]==neg).sum())/(len(pos)*len(neg)))


def summarize(items):
    v=np.array([x['v'] for x in items]); y=np.array([x['win'] for x in items])
    phi=np.array([x['phi'] for x in items]); actual=np.array([x['return'] for x in items])
    adjusted=v+phi
    return {'n':len(items),'wins':int(y.sum()),'win_fraction':float(y.mean()),
        'raw_value_auc':auc(v,y),'potential_adjusted_auc':auc(adjusted,y),'hp_advantage_auc':auc(phi,y),
        'raw_zero_threshold_accuracy':float(((v>0)==y).mean()),
        'raw_predict_win_fraction':float((v>0).mean()),
        'adjusted_zero_threshold_accuracy':float(((adjusted>0)==y).mean()),
        'hp_sign_accuracy':float(((phi>0)==y).mean()),
        'mean_value_wins':float(v[y==1].mean()) if y.any() else None,
        'mean_value_losses':float(v[y==0].mean()) if (y==0).any() else None,
        'mean_actual_return':float(actual.mean()),'return_bias':float((v-actual).mean()),
        'return_mae':float(np.abs(v-actual).mean()),'return_rmse':float(np.sqrt(((v-actual)**2).mean())),
        'zero_return_baseline_rmse':float(np.sqrt((actual**2).mean())),
        'value_min':float(v.min()),'value_max':float(v.max())}


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('directory',type=Path)
    args=p.parse_args()
    games=[json.loads(s) for s in (args.directory/'diagnostic-games.jsonl').read_text().splitlines()]
    groups={k:[] for k in ['first_battle_turn','turn5_if_reached','turn10_if_reached','last_battle_decision','all_battle_decisions']}
    turns={}
    for g in games:
        if g['outcome']==2:continue
        win=g['outcome']==0; score=1 if win else -1
        items=[{'game':g['game'],'turn':d['turn'],'v':d['network_value'],'phi':d['hp_potential'],
                'win':int(win),'return':score+g['terminal_hp_potential']-d['hp_potential']}
               for d in g['decisions'] if d['request']=='Move' and 'network_value' in d]
        if not items:continue
        groups['first_battle_turn'].append(items[0]);groups['last_battle_decision'].append(items[-1])
        groups['all_battle_decisions'].extend(items)
        # Exact turn, first Move request only: never substitute a later turn.
        seen=set()
        for item in items:
            if item['turn'] not in seen:
                turns.setdefault(item['turn'],[]).append(item)
                seen.add(item['turn'])
        for turn in [5,10]:
            item=next((x for x in items if x['turn']>=turn),None)
            if item:groups[f'turn{turn}_if_reached'].append(item)
    report={k:summarize(v) for k,v in groups.items() if v}
    rng=np.random.default_rng(481000)
    for key in groups:
        if key=='all_battle_decisions' or not groups[key]:continue
        items=groups[key]; values=[]
        for _ in range(1000):
            sample=[items[i] for i in rng.integers(0,len(items),len(items))]
            a=auc([x['v']+x['phi'] for x in sample],[x['win'] for x in sample])
            if a is not None:values.append(a)
        report[key]['adjusted_auc_bootstrap95']=np.quantile(values,[.025,.975]).tolist()
    result={'provenance':json.loads((args.directory/'diagnostic-provenance.json').read_text()),
            'stages':report,'turns':{str(t):summarize(turns[t]) for t in sorted(turns)},'interpretation':[
                'Per-turn summaries use the first Move request at that exact turn, once per game; sample sizes shrink as games finish.',
                'Value predicts remaining HP-shaped return, not a calibrated win probability.',
                'Realized return = outcome bonus + terminal HP potential - current HP potential; gamma=1.',
                'V+current potential predicts outcome bonus plus terminal potential; used as a winner-ranking score, not a probability.',
                'Stage summaries take one prediction per game; all-decision summaries correlate within games.',
                'Bootstrap intervals resample games within each stage. Later stages exclude games that ended earlier.',
                'Both sides use the same final checkpoint and matrix search; no training or calibration fit performed.']}
    (args.directory/'value-quality.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(result,indent=2))
    import matplotlib
    matplotlib.use('Agg')
    import matplotlib.pyplot as plt
    fig,axes=plt.subplots(1,3,figsize=(15,4.5))
    data=groups['all_battle_decisions']
    for win,color,label in [(0,'#b85a25','Eventually lost'),(1,'#007f86','Eventually won')]:
        chosen=[x for x in data if x['win']==win]
        axes[0].scatter([x['return'] for x in chosen],[x['v'] for x in chosen],alpha=.2,s=9,color=color,label=label)
        axes[1].hist([x['v'] for x in chosen],bins=25,alpha=.5,color=color,label=label,density=True)
    axes[0].plot([-2,2],[-2,2],color='#777',linestyle=':')
    axes[0].set(xlabel='Realized remaining shaped return',ylabel='Raw predicted value',title='Return prediction')
    axes[1].set(xlabel='Raw predicted value',ylabel='Density',title='Do wins and losses separate?')
    axes[1].legend(frameon=False,fontsize=9)
    keys=[k for k in groups if k!='all_battle_decisions' and groups[k]]
    x=np.arange(len(keys))
    axes[2].plot(x,[report[k]['potential_adjusted_auc'] for k in keys],marker='o',label='Value + current HP potential',color='#007f86')
    axes[2].plot(x,[report[k]['hp_advantage_auc'] for k in keys],marker='o',label='HP advantage alone',color='#7654b5')
    axes[2].axhline(.5,color='#777',linestyle=':')
    axes[2].set(xticks=x,xticklabels=['First','Turn 5','Turn 10','Last'][:len(keys)],ylabel='Winner-ranking AUC',ylim=(0,1),title='One position per game')
    axes[2].legend(frameon=False,fontsize=8)
    for ax in axes:ax.grid(alpha=.15);ax.spines[['top','right']].set_visible(False)
    fig.suptitle(f"Model2 value vs actual outcomes · {len(games)} fixed-weight diagnostic games")
    fig.tight_layout();fig.savefig(args.directory/'value-quality.png',dpi=170,facecolor='white')
    fig,axes=plt.subplots(3,1,figsize=(11,10),sharex=True)
    ts=sorted(turns); rows=[result['turns'][str(t)] for t in ts]
    for field,label,color in [('raw_value_auc','Raw value','#007f86'),('hp_advantage_auc','HP advantage alone','#7654b5')]:
        axes[0].plot(ts,[r[field] if r[field] is not None else np.nan for r in rows],marker='o',label=label,color=color)
    axes[0].axhline(.5,color='#777',linestyle=':');axes[0].set(ylabel='Winner-ranking AUC',ylim=(0,1.03));axes[0].legend()
    axes[1].plot(ts,[r['raw_zero_threshold_accuracy'] for r in rows],marker='o',label='Value > 0 predicts win',color='#007f86')
    axes[1].plot(ts,[max(r['win_fraction'],1-r['win_fraction']) for r in rows],linestyle='--',label='Majority-outcome baseline',color='#999999')
    axes[1].set(ylabel='Winner accuracy',ylim=(0,1.03));axes[1].legend()
    axes[2].bar(ts,[r['wins'] for r in rows],label='Eventually won',color='#007f86')
    axes[2].bar(ts,[r['n']-r['wins'] for r in rows],bottom=[r['wins'] for r in rows],label='Eventually lost',color='#b85a25')
    axes[2].set(xlabel='Exact battle turn',ylabel='Games still represented',xticks=ts);axes[2].legend()
    for ax in axes:ax.grid(alpha=.15);ax.spines[['top','right']].set_visible(False)
    fig.suptitle('Model2 winner prediction at every turn · same 256 fixed-weight games\nLater turns contain only games that lasted that long; small samples are noisy')
    fig.tight_layout();fig.savefig(args.directory/'value-quality-all-turns.png',dpi=150,facecolor='white')
    # Empirical winner frequency by raw value, not by battle turn.
    edges=np.arange(-1.5,1.5001,.15)
    calibration=[]
    for lo,hi in zip(edges[:-1],edges[1:]):
        selected=[x for x in data if lo<=x['v']<hi]
        if not selected:continue
        # Cluster bootstrap: decisions within a game are correlated.
        game_ids=sorted({x['game'] for x in selected})
        totals=np.array([[sum(x['win'] for x in selected if x['game']==g),
                          sum(1 for x in selected if x['game']==g)] for g in game_ids])
        draws=rng.integers(0,len(totals),size=(2000,len(totals)))
        boot=totals[draws].sum(axis=1)
        bounds=np.quantile(boot[:,0]/boot[:,1],[.025,.975])
        calibration.append({'low':float(lo),'high':float(hi),'mean_value':float(np.mean([x['v'] for x in selected])),
                            'decisions':len(selected),'games':len(game_ids),'win_rate':float(np.mean([x['win'] for x in selected])),
                            'game_bootstrap95':bounds.tolist()})
    (args.directory/'value-win-curve.json').write_text(json.dumps({'bins':calibration,'method':'All Move decisions; raw value bins width 0.15; 95% intervals resample games within each bin. Positions are decision-weighted, not independent games.'},indent=2)+'\n')
    fig,axes=plt.subplots(2,1,figsize=(10,7),sharex=True,gridspec_kw={'height_ratios':[3,1]})
    xs=np.array([b['mean_value'] for b in calibration]);ys=np.array([b['win_rate'] for b in calibration])
    low=np.array([b['game_bootstrap95'][0] for b in calibration]);high=np.array([b['game_bootstrap95'][1] for b in calibration])
    axes[0].plot(xs,ys,marker='o',color='#007f86',label='Network value')
    axes[0].fill_between(xs,low,high,color='#007f86',alpha=.18,label='95% game-bootstrap interval')
    axes[0].axhline(.5,color='#777',linestyle=':');axes[0].axvline(0,color='#777',linestyle=':')
    axes[0].set(ylabel='Actual eventual win rate',ylim=(-.03,1.03));axes[0].yaxis.set_major_formatter(plt.matplotlib.ticker.PercentFormatter(1));axes[0].legend()
    axes[1].bar(xs,[b['decisions'] for b in calibration],width=.12,color='#007f86',alpha=.5,label='Network value')
    hp_bins=[]
    # phi is half the team-average HP fraction difference; undo the 0.5
    # reward coefficient so HP's displayed scale is the natural [-1, 1].
    hp_edges=np.linspace(-1,1,15)
    for i,(lo,hi) in enumerate(zip(hp_edges[:-1],hp_edges[1:])):
        selected=[x for x in data if lo<=2*x['phi'] and (2*x['phi']<hi or (i==len(hp_edges)-2 and 2*x['phi']<=hi))]
        if not selected:continue
        game_ids=sorted({x['game'] for x in selected})
        totals=np.array([[sum(x['win'] for x in selected if x['game']==g),sum(x['game']==g for x in selected)] for g in game_ids])
        boot=totals[rng.integers(0,len(totals),size=(2000,len(totals)))].sum(axis=1)
        hp_bins.append({'mean_hp_difference':float(np.mean([2*x['phi'] for x in selected])),
                        'decisions':len(selected),'win_rate':float(np.mean([x['win'] for x in selected])),
                        'game_bootstrap95':np.quantile(boot[:,0]/boot[:,1],[.025,.975]).tolist()})
    hx=[b['mean_hp_difference'] for b in hp_bins]
    axes[0].plot(hx,[b['win_rate'] for b in hp_bins],marker='s',color='#7654b5',label='Team HP fraction difference')
    axes[0].fill_between(hx,[b['game_bootstrap95'][0] for b in hp_bins],[b['game_bootstrap95'][1] for b in hp_bins],color='#7654b5',alpha=.13)
    axes[0].legend()
    axes[1].bar(hx,[b['decisions'] for b in hp_bins],width=.1,color='#7654b5',alpha=.4,label='HP difference')
    axes[1].set(xlabel='Raw network value / team HP fraction difference (own − opponent)',ylabel='Decisions');axes[1].legend()
    (args.directory/'value-hp-win-curve.json').write_text(json.dumps({'value_bins':calibration,'hp_bins':hp_bins,'hp_scale':'2 * HP potential = own mean HP fraction minus opponent mean HP fraction; -1 to +1'},indent=2)+'\n')
    for ax in axes:ax.grid(alpha=.15);ax.spines[['top','right']].set_visible(False)
    fig.suptitle(f'Actual win rate by network value and HP advantage · {len(data):,} decisions / {len(games)} games\nHP difference +0.5 means a 50 percentage-point team HP advantage; shading: 95% game-bootstrap intervals')
    fig.tight_layout();fig.savefig(args.directory/'value-win-curve.png',dpi=160,facecolor='white')


if __name__=='__main__':main()
