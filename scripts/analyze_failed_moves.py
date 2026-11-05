"""Analyze full fixed-weight diagnostic traces, including consecutive Protect."""
import argparse
import collections
import json
from pathlib import Path

PROTECT={'Protect','Detect','Spiky Shield',"King's Shield",'Baneful Bunker','Obstruct','Silk Trap','Burning Bulwark','Endure'}


def main():
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('directory',type=Path)
    args=ap.parse_args()
    games=[json.loads(s) for s in (args.directory/'diagnostic-games.jsonl').read_text().splitlines()]
    attempts=collections.Counter();failed=collections.Counter();buckets=collections.defaultdict(collections.Counter)
    examples=[];mismatches=[];double_turns=0;selected_protects=0;repeated_selected=0
    for game in games:
        own='p'+str(game['learner_side']+1)
        for d in game['decisions']:
            selected=d['selected'] if isinstance(d['selected'],list) else []
            protect=[a for a in selected if a.get('move') in PROTECT]
            double_turns+=len(protect)==2
            histories={h['slot']:h for h in d['protect_history']}
            for h in histories.values():
                if h['stall']!=h['actual_stall']:
                    mismatches.append({'game':game['game'],'turn':d['turn'],**h})
            for a in protect:
                selected_protects+=1
                repeated_selected+=histories[a['actor_slot']]['stall']>0
            last=None
            for line in d['events']:
                p=line.split('|')
                if len(p)<3:continue
                if p[1]=='move':
                    last=p
                    if p[2].startswith(own):
                        attempts[p[3]]+=1
                        if p[3] in PROTECT:
                            slot=ord(p[2][2])-ord('a'); h=histories.get(slot,{})
                            buckets[h.get('stall',-1)]['attempts']+=1
                elif p[1]=='cant':
                    last=None
                elif p[1]=='-fail' and last and last[2].startswith(own) and p[2]==last[2] and (len(p)==3 or p[3].startswith('move:')):
                    move=last[3];failed[move]+=1
                    slot=ord(p[2][2])-ord('a');h=histories.get(slot,{})
                    if move in PROTECT:buckets[h.get('stall',-1)]['failed']+=1
                    chosen_probability=None
                    if d.get('search'):
                        s=d['search'];chosen_probability=s['policy'][s['chosen_candidate']]
                    examples.append({'game':game['game'],'outcome':game['outcome'],'turn':d['turn'],
                        'actor':p[2],'move':move,'history':h,'both_allies_protect':len(protect)==2,
                        'chosen_joint_probability':chosen_probability,'events':d['events'],
                        'selected':selected,'state':d['state']})
    total=sum(attempts.values());nf=sum(failed.values())
    report={'provenance':json.loads((args.directory/'diagnostic-provenance.json').read_text()),
        'attempts':total,'failed_moves':nf,'failure_rate_percent':100*nf/total if total else None,
        'attempts_by_move':dict(attempts),'failures_by_move':dict(failed),
        'protect_buckets_by_prior_stall_counter':dict(buckets),
        'selected_protects':selected_protects,'selected_repeat_protects':repeated_selected,
        'both_allies_protect_decisions':double_turns,'history_mismatches':mismatches,
        'examples':examples,
        'limitations':['New fixed-weight games against the same final checkpoint, not original training failures.',
                       'Protect failures may be rational gambles; failure alone does not establish a mistake.',
                       'Stall-roll probability excludes other conditions that can prevent a move succeeding.']}
    (args.directory/'failed-move-analysis.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({k:v for k,v in report.items() if k not in ['examples','attempts_by_move']},indent=2))


if __name__=='__main__':main()
