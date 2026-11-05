"""Replay only the seeded pairs needed for the 21 unchanged positions."""
from pathlib import Path
import json,subprocess,time
root=Path(__file__).resolve().parents[1]
base=root/'Model2/runs/state-fixes-replay-20261104'
out=base/'unchanged-position-audit'
suite=root/'Model2/runs/opponent-suite-insample-heldout-20261104'
targets=json.loads((out/'targets.json').read_text())
pairs=sorted({(r['pool'],r['game']//2) for r in targets})
exe=root/'target/release/model1-unchanged-audit.exe'
for pool,pair in pairs:
 dest=out/f'{pool}-pair-{pair:03}'
 assert not dest.exists(),dest
 cmd=[str(exe),'--weights',str(suite/'final'),'--eval-matrix','--eval-hp-opponent',
 '--eval-search-teams',str(suite/'training-teams.txt'),'--dex',str(suite/'dex.json'),
 '--matrix-samples','2','--regret-iterations','256','--eval-traces',
 '--teams',str(suite/f'{pool}-teams.txt'),'--evaluate','2','--seed',str(781000+pair),
 '--search-trace-turn','4294967295','--out',str(dest)]
 start=time.time()
 with (out/f'{pool}-pair-{pair:03}.log').open('w') as log:
  subprocess.run(cmd,cwd=root,stdout=log,stderr=subprocess.STDOUT,check=True)
 print(pool,pair,'seconds',round(time.time()-start,2),flush=True)
print('Done',len(pairs)*2,'games',flush=True)
