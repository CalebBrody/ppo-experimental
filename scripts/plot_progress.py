"""Plot fixed-opponent evaluation and PPO diagnostics; no strength claim from loss."""
import argparse
import json
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

def main():
    ap=argparse.ArgumentParser()
    ap.add_argument('run',type=Path)
    ap.add_argument('--out',type=Path,default=Path('reports/progress'))
    args=ap.parse_args();args.out.mkdir(parents=True,exist_ok=True)
    metrics=[json.loads(x) for x in (args.run/'training/metrics.jsonl').read_text().splitlines()]
    points=json.loads((args.run/'evaluation-index.json').read_text())
    rng=np.random.default_rng(37)
    last=metrics[-1]
    decisions={0:0,**{m['update']:m['decisions'] for m in metrics}}
    summary={'run':str(args.run),'updates':last['update'],'decisions':last['decisions'],
             'training_games':last['games'],'training_seconds':last['seconds'],'evaluations':{},
             'self_play_wins':sum(m['wins'] for m in metrics),
             'self_play_losses':sum(m['losses'] for m in metrics),
             'self_play_draws':sum(m['draws'] for m in metrics),
             'method':'95% bootstrap intervals, resampling same-team side-swapped game pairs; draws count as half a win'}
    plt.rcParams.update({'font.family':'DejaVu Sans','font.size':11,'axes.spines.top':False,'axes.spines.right':False})
    fig,axes=plt.subplots(2,2,figsize=(13,8.8),gridspec_kw={'height_ratios':[1.25,1]})
    colors={'random':'#007f86','initial':'#7654b5'}
    for ax,name,title in zip(axes[0],['random','initial'],['Against random legal play','Against the frozen starting model']):
        ps=sorted([p for p in points if p['opponent']==name],key=lambda p:p['update'])
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
    for update in sorted({p['update'] for p in points if p['update'] not in (0,last['update'])}):
        ax.axvline(decisions[update]/1000,color='#99a3ad',linestyle=':',alpha=.5,linewidth=1)
    ax.axhline(50,color='#99a3ad',linestyle='--',linewidth=1)
    ax.set(title='Self-play score: opponents change',xlabel='Training decisions (thousands)',ylabel='20-update rolling score (%)',ylim=(0,100))
    ax.grid(alpha=.15)
    ax.text(.02,.06,'Diagnostic only; a stronger league can keep this near 50%.',transform=ax.transAxes,fontsize=9)
    ax=axes[1,1]
    vals=[m['value_loss'] for m in metrics]
    ax.plot(x,vals,color='#e1aa60',alpha=.3,linewidth=.8)
    ax.plot(x,smooth(vals,20),color='#ac681a',linewidth=1.8,label='20-update average')
    ax.set(title='Value prediction loss',xlabel='Training decisions (thousands)',ylabel='PPO value loss')
    ax.grid(alpha=.15);ax.legend(frameon=False,fontsize=9)
    fig.suptitle(f'Model1 experimental · {last["seconds"]/60:.1f} minutes of RTX 2060 training',fontsize=19,fontweight='bold',x=.06,ha='left',y=.99)
    fig.text(.06,.945,f'{last["games"]:,} training games · {last["decisions"]:,} learner decisions · {last["update"]} PPO updates',fontsize=12,color='#4a5563')
    fig.text(.06,.015,'Held-out teams; matching seeded setups with sides swapped. Score = wins + ½ draws. Bars: 95% paired-cluster bootstrap intervals.\n400 games per opponent at the start/end; 100 at intermediate checkpoints. One training seed; this is an early learning check, not an Elo estimate.',fontsize=9,color='#4a5563')
    fig.tight_layout(rect=[.025,.065,.99,.92])
    fig.savefig(args.out/'progress.png',dpi=170,facecolor='white')
    fig.savefig(args.out/'progress.svg',facecolor='white')
    (args.out/'summary.json').write_text(json.dumps(summary,indent=2))
    print(json.dumps(summary,indent=2))

if __name__=='__main__':main()
