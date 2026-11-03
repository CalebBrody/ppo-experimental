"""Render real hourly evaluation results and the complete measured matchup grid."""
import json
from pathlib import Path
import numpy as np
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt

def atomic_json(path,data):
    temp=path.with_suffix('.tmp')
    temp.write_text(json.dumps(data,indent=2),encoding='utf-8')
    temp.replace(path)

def interval(report):
    values=np.array([r['score'] for r in report['results']]).reshape(-1,2).mean(axis=1)
    rng=np.random.default_rng(37)
    means=values[rng.integers(0,len(values),(3000,len(values)))].mean(axis=1)
    return np.mean(values)*100,*list(np.quantile(means,[.025,.975])*100)

def render(run,hour,latest=False,output=None):
    run=Path(run)
    points=json.loads((run/'evaluations.json').read_text())
    points=[p for p in points if p['hour']<=hour]
    meta=json.loads((run/'training'/f'hour-{hour:03}.json').read_text())
    metrics=[]
    for line in (run/'training/metrics.jsonl').read_text().splitlines():
        try:m=json.loads(line)
        except json.JSONDecodeError:continue
        if latest or m['update']<=meta['update']:metrics.append(m)
    if latest and metrics:meta=metrics[-1]
    dest=Path(output) if output else run/'reports'/f'hour-{hour:03}'
    dest.mkdir(parents=True,exist_ok=True)
    plt.rcParams.update({'font.family':'DejaVu Sans','font.size':10,'axes.spines.top':False,'axes.spines.right':False})
    fig,axs=plt.subplots(2,2,figsize=(13,8))
    for ax,kind,label,color in zip(axs[0],['random','hour-000'],['Against random legal play','Against the model at the start of this run'],['#007f86','#7654b5']):
        ps=[p for p in points if p['opponent']==kind]
        vals=[interval(json.loads(Path(p['report']).read_text())) for p in ps]
        if vals:
            y,lo,hi=np.array(vals).T
            ax.errorbar([p['hour'] for p in ps],y,yerr=[y-lo,hi-y],marker='o',color=color,capsize=3)
        ax.axhline(50,color='gray',linestyle='--',alpha=.4)
        ax.set(title=label,xlabel='Training hour',ylabel='Evaluation score (%)',ylim=(0,100))
        ax.grid(alpha=.15)
    x=[m['seconds']/3600 for m in metrics]
    rolling=[];loss=[]
    for i in range(len(metrics)):
        chunk=metrics[max(0,i-19):i+1]
        n=sum(m['wins']+m['losses']+m['draws'] for m in chunk)
        rolling.append(100*sum(m['wins']+.5*m['draws'] for m in chunk)/n if n else np.nan)
        loss.append(np.mean([m['value_loss'] for m in chunk]))
    axs[1,0].plot(x,rolling,color='#3d566e')
    axs[1,0].set(title='Self-play score (changing opponent league)',xlabel='Training hour',ylabel='20-update score (%)',ylim=(0,100))
    axs[1,1].plot(x,loss,color='#ac681a')
    axs[1,1].set(title='Value loss (20-update average)',xlabel='Training hour',ylabel='PPO value loss')
    for ax in axs[1]:ax.grid(alpha=.15)
    label=f'stopped at {meta["seconds"]/3600:.2f} hours; evaluations through hour {hour}' if latest else f'hour {hour} of 24'
    fig.suptitle(f'Model1 · {label}\n{meta["games"]:,} games · {meta["decisions"]:,} decisions',fontsize=14,fontweight='bold')
    fig.text(.05,.015,'Held-out teams; matching seeded setups and swapped sides. Score = wins + half of draws. Bars: 95% matchup-pair bootstrap intervals.\nThese are fixed-opponent tests, not ladder Elo. Training diagnostics alone do not measure playing strength.',fontsize=9)
    fig.tight_layout(rect=[0,.07,1,.94]);fig.savefig(dest/'progress.png',dpi=150);plt.close(fig)
    fig,axs=plt.subplots(1,2,figsize=(15,7),gridspec_kw={'width_ratios':[1,1.25]})
    prior=[p for p in points if p['hour']==hour and p['opponent'].startswith('hour-') and (hour==0 or int(p['opponent'][5:])<hour)]
    prior.sort(key=lambda p:p['opponent'])
    if prior:
        vals=[interval(json.loads(Path(p['report']).read_text())) for p in prior]
        y,lo,hi=np.array(vals).T
        idx=np.arange(len(prior))
        axs[0].barh(idx,y,color='#007f86',alpha=.8,xerr=[y-lo,hi-y],capsize=2)
        axs[0].set_yticks(idx,[f'Hour {int(p["opponent"][5:])}' for p in prior])
        for i,v in enumerate(y):axs[0].text(min(v+2,94),i,f'{v:.0f}%',va='center',fontsize=8)
    axs[0].axvline(50,color='gray',linestyle='--');axs[0].set(xlim=(0,100),xlabel='Latest model score (%)',title=f'Hour {hour} against every earlier hour')
    grid=np.full((hour+1,hour+1),np.nan)
    for p in points:
        if p['opponent'].startswith('hour-'):
            grid[p['hour'],int(p['opponent'][5:])]=p['score']*100
    cmap=plt.get_cmap('RdYlGn').copy();cmap.set_bad('#eeeeee')
    im=axs[1].imshow(grid,vmin=0,vmax=100,cmap=cmap,origin='lower',aspect='equal')
    axs[1].set(xticks=np.arange(hour+1),yticks=np.arange(hour+1),xlabel='Opponent checkpoint hour',ylabel='Evaluated checkpoint hour',title='All measured hourly matchups')
    if hour<=12:
        for i in range(hour+1):
            for j in range(hour+1):
                if np.isfinite(grid[i,j]):axs[1].text(j,i,f'{grid[i,j]:.0f}',ha='center',va='center',fontsize=8)
    fig.colorbar(im,ax=axs[1],fraction=.046,label='Score (%)')
    fig.suptitle(f'Model1 · historical opponents through hour {hour}',fontsize=16,fontweight='bold')
    n_prior=prior[0]['games'] if prior else 0
    n_random=next((p['games'] for p in points if p['opponent']=='random'),0)
    fig.text(.05,.02,f'{n_prior} games per historical opponent; {n_random} against random play. Gray cells have not been evaluated; reverse results are not inferred.\nError bars resample matched, side-swapped game pairs. Small changes can be sampling noise.',fontsize=9)
    fig.tight_layout(rect=[0,.08,1,.94]);fig.savefig(dest/'matchups.png',dpi=160);plt.close(fig)
    current=[p for p in points if p['hour']==hour]
    summary={'hour':hour,'games':meta['games'],'decisions':meta['decisions'],'update':meta['update'],
             'progress_image':str((dest/'progress.png').resolve()),'matchups_image':str((dest/'matchups.png').resolve()),'evaluations':current}
    atomic_json(dest/'summary.json',summary)
    if not output:atomic_json(run/'latest-report.json',summary)
    return summary

if __name__=='__main__':
    import sys
    render(Path(sys.argv[1]),int(sys.argv[2]))
