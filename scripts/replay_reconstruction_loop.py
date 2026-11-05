"""Replay the complete matched HP suite with a frozen control or fixed executable."""
import argparse,json,pathlib,subprocess,time
p=argparse.ArgumentParser();p.add_argument('--iteration',type=int,default=1);a=p.parse_args();a.phase=f'iteration-{a.iteration}'
root=pathlib.Path(__file__).resolve().parents[1]; suite=root/'Model2/runs/opponent-suite-insample-heldout-20261104'; out=root/'Model2/runs/reconstruction-fix-loop-20261105'
for pool in ['training','heldout']:
 dest=out/f'{a.phase}-{pool}'; assert not dest.exists(), f'Refusing reuse: {dest}'
 cmd=[str(root/f'target/release/model1-reconstruction-loop-{a.iteration}.exe'),'--weights',str(suite/'final'),'--eval-matrix','--eval-hp-opponent','--eval-search-teams',str(suite/'training-teams.txt'),'--dex',str(suite/'dex.json'),'--matrix-samples','2','--regret-iterations','256','--eval-traces','--teams',str(suite/f'{pool}-teams.txt'),'--evaluate','200','--seed','781000','--out',str(dest)]
 start=time.time(); print(f'Starting {a.phase} {pool}',flush=True)
 with (out/f'{a.phase}-{pool}.log').open('w') as log: subprocess.run(cmd,cwd=root,stdout=log,stderr=subprocess.STDOUT,check=True)
 r=json.loads((dest/'evaluation.json').read_text()); assert all(n <= positions*8*8*2 for n,positions in zip(r['search_simulations'],r['search_positions'])), 'Search budget differs from frozen one-world control'; print(json.dumps({'phase':a.phase,'pool':pool,'wins':r['wins'],'losses':r['losses'],'seconds':time.time()-start}),flush=True)
