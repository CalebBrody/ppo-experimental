"""Compare logged scalar search estimates only while public battle histories match."""
import json, pathlib, numpy as np
root=pathlib.Path(__file__).resolve().parents[1]
run=root/'Model2/runs/state-fixes-replay-20261104'
out=run/'value-difference-analysis'
out.mkdir(exist_ok=True)
screen=json.loads((root/'Model2/runs/hp-loss-state-audit-20261104/loss-screen.json').read_text())
exposure={}
for k in ['unburden_own_positions','toxic_history_positions']:
 for x in screen[k]:
  exposure.setdefault((x['pool'],x['game'],x['decision']),[]).append(k)
rows=[];excluded=[];candidates=0
def weighted(s,key):
 return float(np.asarray(s['policy']) @ np.asarray(s[key]) @ np.asarray(s['opponent_policy']))
for pool in ['training','heldout']:
 for game in range(200):
  name=f'game-{game:04}.json'
  a=json.loads((run/f'control-{pool}/traces'/name).read_text())
  b=json.loads((run/f'fixed-{pool}/traces'/name).read_text())
  same_history=True
  for x,y in zip(a['decisions'],b['decisions']):
   key=(pool,game,x['decision'])
   matched=(same_history and x['decision']==y['decision'] and x['turn']==y['turn']
       and x['request']==y['request'] and x['state']==y['state'])
   if matched and x.get('search') and y.get('search'):
    s,t=x['search'],y['search']
    rows.append(dict(pool=pool,game=game,decision=x['decision'],turn=x['turn'],
      control_loss=a['outcome']==1,confirmed_exposure=exposure.get(key,[]),
      before=s['value'],after=t['value'],delta=t['value']-s['value'],
      next_value_before=weighted(s,'bootstrap_payoff'),next_value_after=weighted(t,'bootstrap_payoff'),
      next_value_delta=weighted(t,'bootstrap_payoff')-weighted(s,'bootstrap_payoff'),
      same_candidates=s['candidates']==t['candidates'] and s['opponent_candidates']==t['opponent_candidates']))
   # Include the decision immediately BEFORE its events diverge; exclude everything after.
   same_history=matched and x.get('events')==y.get('events')
eligible={(r['pool'],r['game'],r['decision']) for r in rows}
for key,kinds in exposure.items():
 if key not in eligible: excluded.append(dict(pool=key[0],game=key[1],decision=key[2],exposures=kinds))
def stats(rs,key):
 v=np.array([r[key] for r in rs]);n=len(v)
 changed=v[np.abs(v)>1e-6]
 return dict(n=n,exact_changed=int(np.count_nonzero(v)),changed_over_1e6=int(len(changed)),
  percent_changed_over_1e6=100*len(changed)/n if n else None,
  percent_exact_changed=100*np.count_nonzero(v)/n if n else None,
  unchanged_within_1e6=int(n-len(changed)),mean_absolute_delta=float(np.abs(v).mean()) if n else None,
  median_absolute_delta=float(np.median(np.abs(v))) if n else None,
  max_absolute_delta=float(np.abs(v).max()) if n else None,
  signed_range=[float(v.min()),float(v.max())] if n else None)
groups={'confirmed_exposed_positions':[r for r in rows if r['confirmed_exposure']],
'all_matched_loss_positions':[r for r in rows if r['control_loss']],
'all_matched_positions':rows}
summary={k:{'search_value':stats(v,'delta'),'expected_next_state_value':stats(v,'next_value_delta'),
'games':len(set((r['pool'],r['game']) for r in v))} for k,v in groups.items()}
report={'definitions':{'search_value':'Solved matrix expected shaped return, including immediate reward; not win probability.',
'expected_next_state_value':'policy @ bootstrap_payoff @ opponent_policy: searched expectation of next-state neural value, terminal cells contribute zero; can change when strategies change.',
'delta':'fixed minus control','match':'Identical prior public event sequence, same turn/request and logged pre-decision observation. Include last position before event divergence.',
'limitations':'Not fixed-hidden-world/RNG ablation. Search RNG consumption may diverge before public battle events do. Confirmed exposure list covers own Unburden and Toxic in original losses; per-position Choice exposure was not logged.'},
'confirmed_exposure_positions_original':len(exposure),'confirmed_exposure_positions_excluded':len(excluded),
'summary':summary,'excluded_exposed_positions':excluded,'positions':rows}
(out/'value-differences.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({k:v for k,v in report.items() if k not in ['positions','excluded_exposed_positions']},indent=2))

# Static, exportable histogram: include unchanged positions explicitly.
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
from matplotlib.ticker import MaxNLocator
fig, axes=plt.subplots(1,2,figsize=(12,5.8),gridspec_kw={'width_ratios':[1,1.15]})
fig.patch.set_facecolor('#f5f7fa')
for ax in axes:
 ax.set_facecolor('white')
 ax.spines[['top','right']].set_visible(False)
 ax.grid(axis='y',alpha=.2)
 ax.set_axisbelow(True)
 ax.yaxis.set_major_locator(MaxNLocator(integer=True))
v=np.array([r['next_value_delta'] for r in groups['confirmed_exposed_positions']])
zero=v[np.abs(v)<=1e-6];changed=v[np.abs(v)>1e-6]
bins=np.arange(-.125,.25001,.025)
axes[0].hist([zero,changed],bins=bins,stacked=True,color=['#aab3c0','#087f8c'],
 label=[f'Unchanged: {len(zero)}',f'Changed: {len(changed)}'],edgecolor='white')
axes[0].axvline(0,color='#536172',lw=1,ls='--')
axes[0].set(title='Known affected positions\n17 / 38 changed (44.7%)',xlabel='Fixed − control expected next-state value',ylabel='Search positions')
axes[0].legend(frameon=False,loc='upper right')
allv=np.array([r['next_value_delta'] for r in groups['all_matched_loss_positions']])
nz=allv[np.abs(allv)>1e-6]
axes[1].hist(nz,bins=np.arange(-1,1.0001,.1),color='#6556a6',edgecolor='white')
axes[1].axvline(0,color='#536172',lw=1,ls='--')
axes[1].set(title='All comparable positions in 136 losses\n69 / 1,005 changed (6.9%)',xlabel='Fixed − control expected next-state value',ylabel='Changed search positions')
axes[1].text(.98,.96,'936 unchanged positions omitted\nfrom this panel for visibility',transform=axes[1].transAxes,ha='right',va='top',fontsize=10,color='#536172')
fig.suptitle('How much did the fixes change the next-state estimate?',fontsize=17,fontweight='bold',x=.06,ha='left')
fig.text(.06,.85,'Paired replay positions with identical public history before the decision',fontsize=11,color='#536172')
fig.text(.06,.045,'Left: confirmed own-Unburden / Toxic exposure; 23 of 61 original positions excluded after divergence.\nMetric: strategy-weighted next-state neural value (bootstrap term), excluding immediate reward. Change threshold: 10⁻⁶.\nBoth players receive all three fixes. Search sampling may differ; this is not a fixed-RNG, single-field ablation.',
 fontsize=9,color='#536172',linespacing=1.5)
fig.subplots_adjust(left=.065,right=.97,top=.72,bottom=.24,wspace=.3)
fig.savefig(out/'next-state-value-differences.png',dpi=180,facecolor=fig.get_facecolor())
fig.savefig(out/'next-state-value-differences.svg',facecolor=fig.get_facecolor())
# Histogram counts make the graphic independently auditable.
(out/'histogram-bins.json').write_text(json.dumps({
 'known_affected':{'edges':bins.tolist(),'counts':np.histogram(v,bins)[0].tolist(),'n':len(v),'zeros':len(zero)},
 'all_matched_losses_changed_only':{'edges':np.arange(-1,1.0001,.1).tolist(),'counts':np.histogram(nz,np.arange(-1,1.0001,.1))[0].tolist(),'n':len(nz),'zeros_omitted':int(len(allv)-len(nz))}},indent=2)+'\n')
print('Saved',out/'next-state-value-differences.png')
