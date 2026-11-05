"""Make an evidence-based post-run infographic from matched traced diagnostics."""
import argparse
import json
from pathlib import Path
import textwrap
import numpy as np
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt


def interval(values, rng):
    pairs=np.asarray(values,dtype=float).reshape(-1,2).mean(axis=1)
    return np.quantile(pairs[rng.integers(0,len(pairs),(5000,len(pairs)))].mean(axis=1),[.025,.975])


def main():
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('run',type=Path)
    args=ap.parse_args()
    run=args.run.resolve()
    audit=json.loads((run/'issue-check.json').read_text())
    by_name={Path(x['directory']).name:x for x in audit['evaluations']}
    reports={name:by_name[name] for name in ['start-sampled','final-sampled','start-greedy','final-greedy']}
    for name,r in reports.items():
        if not r['trace_complete']:
            raise ValueError('Incomplete traces: '+name)
    metrics=[json.loads(x) for x in (run/'training/metrics.jsonl').read_text().splitlines()]
    last=metrics[-1]
    out=run/'reports/issue-infographic'
    out.mkdir(parents=True,exist_ok=True)
    rng=np.random.default_rng(37)
    navy='#16293b';muted='#52687a';teal='#087f8c';orange='#d16732';pale='#e9f1f4'
    plt.rcParams.update({'font.family':'DejaVu Sans','font.size':11,'axes.spines.top':False,'axes.spines.right':False,'axes.labelcolor':navy,'text.color':navy,'xtick.color':muted,'ytick.color':muted})
    fig=plt.figure(figsize=(13,14.8),facecolor='#fafcfd')
    fig.text(.065,.966,'HP PROGRESS REWARD',fontsize=13,color=teal,fontweight='bold')
    fig.text(.065,.933,'Did the known mistakes improve?',fontsize=27,fontweight='bold')
    fig.text(.065,.903,f'{last["seconds"]/60:.1f} min continuation  |  {last["games"]:,} training games  |  {last["update"]} PPO updates',fontsize=12,color=muted)
    fig.text(.065,.881,'Same experimental architecture; additional training with delta-HP rewards. No unshaped control.',fontsize=10,color=muted)
    grid=fig.add_gridspec(3,2,left=.08,right=.95,bottom=.19,top=.835,hspace=.62,wspace=.35)
    summary={'training':last,'comparisons':{},'audit':str(run/'issue-check.json')}
    for col,mode in enumerate(['sampled','greedy']):
        ax=fig.add_subplot(grid[0,col]); before=reports['start-'+mode];after=reports['final-'+mode]
        raw=[]
        for r in [before,after]:
            raw.append(json.loads((Path(r['directory'])/'evaluation.json').read_text()))
        a,b=raw
        keys=lambda r:[(x['match_seed'],x['learner_side'],x['team_a'],x['team_b']) for x in r['results']]
        assert keys(a)==keys(b),'Evaluation setups differ'
        vals=[np.array([x['score'] for x in r['results']]) for r in raw]
        means=np.array([x.mean()*100 for x in vals]);cis=np.array([interval(x,rng)*100 for x in vals])
        ax.bar([0,1],means,color=[muted,teal],width=.55)
        ax.errorbar([0,1],means,yerr=np.array([means-cis[:,0],cis[:,1]-means]),fmt='none',ecolor=navy,capsize=4)
        for i,r in enumerate([before,after]):
            ax.text(i,means[i]+4,f'{means[i]:.1f}%',ha='center',fontsize=18,fontweight='bold')
            ax.text(i,7,f'{r["wins"]}W / {r["losses"]}L\n{r["draws"]} draws',ha='center',color='white',fontsize=10)
        delta=vals[1]-vals[0];ci=interval(delta,rng)*100
        ax.set(xticks=[0,1],xticklabels=['Starting checkpoint','After continuation'],ylim=(0,116),ylabel='Score against random (%)')
        ax.set_yticks([0,25,50,75,100]);ax.set_title(('Sampled policy' if mode=='sampled' else 'Greedy policy')+f' · {before["games"]:,} games each',loc='left',fontweight='bold',pad=30)
        ax.text(0,1.035,f'Change {delta.mean()*100:+.1f} points · 95% CI {ci[0]:+.1f} to {ci[1]:+.1f}',transform=ax.transAxes,fontsize=10,color=muted)
        summary['comparisons'][mode]={'starting_score':means[0],'final_score':means[1],'change_points':delta.mean()*100,'change_ci95':ci.tolist()}
    start,end=reports['start-greedy'],reports['final-greedy']
    ax=fig.add_subplot(grid[1,0])
    shares=[]
    for i,r in enumerate([start,end]):
        c=r['counts'];n=c.get('dual_target_opportunities',0);left=100*c.get('dual_target_1',0)/max(1,n)
        shares.append((left,100-left))
        ax.barh(i,left,color=teal,height=.48)
        ax.barh(i,100-left,left=left,color=orange,height=.48)
        ax.text(left/2,i,f'{left:.1f}%',ha='center',va='center',color='white',fontweight='bold')
        ax.text(left+(100-left)/2,i,f'{100-left:.1f}%',ha='center',va='center',color='white',fontweight='bold')
        ax.text(101,i,f'n={n:,}',va='center',fontsize=9,color=muted)
    ax.set(yticks=[0,1],yticklabels=['Start','Final'],xlim=(0,122),xticks=[0,50,100],xlabel='Eligible target selections (%)',ylim=(1.65,-.7))
    ax.set_title('Target diversity · greedy',loc='left',fontweight='bold',pad=16)
    ax.text(0,-.35,'Teal: slot 1   Orange: slot 2\nBoth foes alive; both targets legal. Collapse screen: ≥95% one slot.',transform=ax.transAxes,fontsize=9,color=muted)
    ax=fig.add_subplot(grid[1,1]);rates=[r['immunity_events_per_100_attempts'] for r in [start,end]]
    ax.bar([0,1],rates,color=[muted,orange],width=.55)
    for i,r in enumerate([start,end]):
        c=r['counts'];ax.text(i,rates[i]+.25,f'{rates[i]:.2f}',ha='center',fontsize=18,fontweight='bold')
        ax.text(i,0.2,f'{c.get("observed_immunity_events",0)} events\n{c.get("executed_move_attempts",0):,} attempts',ha='center',va='bottom',color='white',fontsize=9)
    ax.set(xticks=[0,1],xticklabels=['Start','Final'],ylabel='Immunity events / 100 move attempts',ylim=(0,max(rates)*1.4+0.2))
    ax.set_title('Immunity attacks · greedy',loc='left',fontweight='bold',pad=16)
    known=[r['counts'].get('known_type_immunity_events',0) for r in [start,end]]
    ax.text(0,-.29,f'Known pre-action type immunity, same target: {known[0]} → {known[1]} events.\nOther events can involve switches or exceptional mechanics.',transform=ax.transAxes,fontsize=9,color=muted)
    ax=fig.add_subplot(grid[2,0]);ax.axis('off')
    ax.set_title('Known prerequisite problems · greedy',loc='left',fontweight='bold',pad=12)
    labels=[('Double Shock without Electric','double_shock_without_electric'),('Steel Roller without terrain','steel_roller_without_terrain'),('First-turn move used late','first_turn_move_after_first_action')]
    risk_keys={'double_shock_without_electric':'double_shock_risk_states','steel_roller_without_terrain':'steel_roller_risk_states'}
    def entry(r,key):
        count=r['counts'].get(key,0)
        return f'{count} / {r["counts"].get(risk_keys[key],0)}' if key in risk_keys else str(count)
    rows=[[label,entry(start,key),entry(end,key)] for label,key in labels]
    table=ax.table(cellText=rows,colLabels=['Selection screen','Start','Final'],colWidths=[.64,.18,.18],cellLoc='left',loc='upper center',bbox=[0,.3,1,.67]);table.auto_set_font_size(False);table.set_fontsize(10)
    for (row,col),cell in table.get_celld().items():
        cell.set_edgecolor('#d8e3e9');cell.set_facecolor(pale if row==0 else '#fafcfd');cell.set_text_props(color=navy,weight='bold' if row==0 else 'normal')
    failed=lambda r,move:r['counts'].get('failed_move:'+move,0)
    ax.text(0,.18,f'Actual failed attempts: Double Shock {failed(start,"Double Shock")} → {failed(end,"Double Shock")}; Steel Roller {failed(start,"Steel Roller")} → {failed(end,"Steel Roller")}.',fontsize=9,color=orange,transform=ax.transAxes)
    ax.text(0,.04,'First two rows: flags / relevant active states with move PP.\nLegality may vary; opponent terrain changes can rescue Steel Roller.',fontsize=9,color=muted,transform=ax.transAxes)
    ax=fig.add_subplot(grid[2,1]);pcts=[100*r['top_two_preview_share'] for r in [start,end]]
    ax.bar([0,1],pcts,color=[muted,teal],width=.55)
    for i,p in enumerate(pcts):ax.text(i,p+2,f'{p:.1f}%',ha='center',fontsize=17,fontweight='bold')
    ax.axhline(98,color=orange,linestyle='--',linewidth=1)
    ax.text(.02,1.02,'Screen: top two index orders cover ≥98% of previews',transform=ax.transAxes,fontsize=9,color=muted)
    ax.set(xticks=[0,1],xticklabels=['Start','Final'],ylabel='Top two preview index orders (%)',ylim=(0,112),yticks=[0,25,50,75,100])
    ax.set_title('Preview index concentration · greedy',loc='left',fontweight='bold',pad=30)
    states=end['issues'];open_issues=[k.replace('_',' ') for k,v in states.items() if v=='observed']
    footer='Still observed: '+(', '.join(open_issues) if open_issues else 'none of the screened behavioral patterns in this sample')+'.'
    if audit['tracked_type_encoding_gap']:footer+=' Current-type observation gap remains open.'
    fig.text(.065,.135,'\n'.join(textwrap.wrap(footer,112)),fontsize=12,fontweight='bold',linespacing=1.5)
    fig.text(.065,.067,'Held-out teams; identical seeded setups with swapped sides. Random legal opponent. Score = wins + ½ draws.\nScore intervals bootstrap matchup pairs. Behavioral rates are descriptive. No finite sample proves a problem resolved.\nSee issue-check.json for flags, denominators, per-team losses and replay examples.',fontsize=9,color=muted,linespacing=1.5)
    for suffix in ['png','svg']:fig.savefig(out/f'infographic.{suffix}',dpi=170,facecolor=fig.get_facecolor())
    summary['greedy_issues']=states
    summary['tracked_type_encoding_gap']=audit['tracked_type_encoding_gap']
    summary['source_checks']=audit.get('source_checks',{})
    summary['behavior']={}
    for mode in ['sampled','greedy']:
        summary['behavior'][mode]={}
        for stage in ['start','final']:
            r=reports[stage+'-'+mode]
            summary['behavior'][mode][stage]={key:r[key] for key in ['games','wins','losses','draws','dominant_target_share','top_two_preview_share','immunity_events_per_100_attempts','issues','counts']}
    (out/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
    print(json.dumps(summary,indent=2))


if __name__=='__main__':main()
