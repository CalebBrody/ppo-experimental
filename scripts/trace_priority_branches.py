"""Trace priority-ordered paths for game 124 turn 1's archived 20-100-path cells.

python scripts/trace_priority_branches.py --build --out reports/priority-trace-16
Subsequent runs omit --build. Each cell is fully enumerated once; Python compares
the first --limit completed paths (including the initial path) with the full run.
Instrumentation and engine changes are confined to an isolated source snapshot.
"""
from pathlib import Path
import argparse
import json
import os
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]
BASE = ROOT / 'reports/search-profiler-builds/20261105-092822-758035'
WORK = ROOT / 'reports/priority-trace-worker'
SUITE = ROOT / 'Model2/runs/opponent-suite-insample-heldout-20261104'
OLD = ROOT / 'reports/game124-turn1-cell-profile-final-20261105/profile.json'


def replace(path, old, new):
    text = path.read_text()
    assert old in text, (path, old)
    path.write_text(text.replace(old, new, 1))


def build():
    WORK.mkdir(parents=True, exist_ok=False)
    shutil.copytree(BASE / 'src', WORK / 'src')
    shutil.copytree(ROOT.parent / 'Ninjask/src', WORK / 'engine/src')
    for name in ['Cargo.toml', 'Cargo.lock']:
        shutil.copy2(BASE / name, WORK / name)
    shutil.copy2(ROOT.parent / 'Ninjask/Cargo.toml', WORK / 'engine/Cargo.toml')
    replace(WORK / 'engine/Cargo.toml', 'version = "0.2.0"', 'version = "0.2.2026110516"')
    lib = WORK / 'engine/src/lib.rs'
    lib.write_text(lib.read_text() + '\npub static PROFILE_PATHS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);\npub static PROFILE_DEADLINE: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);\n')
    rng = WORK / 'engine/src/rng.rs'
    replace(rng, '    pub(crate) fn advance(&mut self) -> bool {', '''    pub(crate) fn diagnostic_path(&self) -> serde_json::Value {
        serde_json::json!({"effective_priority": self.priority_ceiling,
            "pending": self.pending.len(), "decisions": self.path.iter().enumerate().map(|(i,d)|
                serde_json::json!({"position":i+1,"kind":format!("{:?}",d.kind),
                    "priority":d.priority,"rank":d.selected,"alternatives":d.alternatives,
                    "selected":d.order.as_ref().map_or(d.selected,|o|o[d.selected as usize])})
                ).collect::<Vec<_>>()})
    }
    pub(crate) fn advance(&mut self) -> bool {''')
    en = WORK / 'engine/src/battle/enumerate.rs'
    replace(en, '        loop {', '''        use std::io::Write;
        let trace_file = std::env::var("PRIORITY_TRACE_FILE").unwrap();
        let mut trace = std::fs::File::create(trace_file).unwrap();
        let mut count = 0u64;
        loop {''')
    replace(en, '            let key = battle.state_key();', '''            let key = battle.state_key();
            let outcome_id = indices.get(&key).copied().unwrap_or(outcomes.len());
            count += 1;
            let mut record = path.diagnostic_path();
            record["path"] = serde_json::json!(count);
            record["probability"] = serde_json::json!(probability);
            record["outcome_id"] = serde_json::json!(outcome_id);
            writeln!(trace, "{}", record).unwrap();''')
    replace(en, '        Ok(outcomes)', '        crate::PROFILE_PATHS.store(count,std::sync::atomic::Ordering::Relaxed);\n        Ok(outcomes)')
    cp = WORK / 'src/model2/cell_profiler.rs'
    replace(cp, '        eprintln!("Profiling cell {},{}",i+1,j+1);', '''        if !cfg["cells"].as_array().unwrap().iter().any(|v|v[0].as_u64()==Some((i+1)as u64)&&v[1].as_u64()==Some((j+1)as u64)){continue;}
        std::env::set_var("PRIORITY_TRACE_FILE",args.out.join(format!("paths-{}-{}.jsonl",i+1,j+1)));
        eprintln!("Profiling cell {},{}",i+1,j+1);''')
    replace(cp, '                for chunk in outcomes.chunks(64){', '''                let rewards:Vec<f64>=outcomes.iter().map(|o|env::battle_reward(&o.battle,side,o.battle.ended,before)as f64).collect();
                let mut all_values=Vec::<f64>::new();
                for chunk in outcomes.chunks(64){''')
    replace(cp, '                        states_count+=weights.len();', '                        all_values.extend(values.iter().map(|v|v.clamp(-2.,2.)as f64));\n                        states_count+=weights.len();')
    replace(cp, '                row["status"]=json!("completed");', '''                assert_eq!(all_values.len(),outcomes.len(),"This turn-1 diagnostic requires nonterminal leaves");
                row["leaf_values"]=json!(all_values.iter().zip(&rewards).map(|(v,r)|v+r).collect::<Vec<_>>());
                row["status"]=json!("completed");''')
    with (WORK / 'build.log').open('w') as log:
        subprocess.run(['cargo','build','--release','--manifest-path',str(WORK/'Cargo.toml'),
                        '--target-dir',str(ROOT/'target')],cwd=ROOT,stdout=log,stderr=subprocess.STDOUT,check=True)
    shutil.copy2(ROOT/'target/release/model1.exe',WORK/'trace.exe')


def main():
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--build',action='store_true')
    ap.add_argument('--limit',type=int,default=16)
    ap.add_argument('--out',type=Path,required=True)
    a=ap.parse_args()
    if a.limit<1: ap.error('--limit must be positive')
    if a.build: build()
    out=a.out.resolve();out.mkdir(parents=True,exist_ok=False)
    old=json.loads(OLD.read_text())
    cells=[[c['row'],c['column']]for c in old['cells']if 20<=c['paths']<=100]
    game=SUITE/'training-hp/traces/game-0124.json'
    config={'game':str(game),'turn':1,'cell':'all','cells':cells,'mode':'PreciceButSame','timeout_seconds':0}
    (out/'config.json').write_text(json.dumps(config,indent=2))
    env=os.environ.copy();env['MODEL2_PROFILE_CONFIG']=str(out/'config.json')
    cmd=[str(WORK/'trace.exe'),'--eval-matrix','--weights',str(SUITE/'final'),'--dex',str(SUITE/'dex.json'),
         '--teams',str(SUITE/'training-teams.txt'),'--eval-search-teams',str(SUITE/'training-teams.txt'),
         '--seed',str(json.loads(game.read_text())['match_seed']),'--out',str(out)]
    with (out/'worker.log').open('w') as log:
        subprocess.run(cmd,cwd=ROOT,env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
    report=json.loads((out/'profile.json').read_text());rows=[]
    for c in report['cells']:
        paths=[json.loads(s)for s in (out/f"paths-{c['row']}-{c['column']}.jsonl").read_text().splitlines()]
        selected=paths[:a.limit];mass=sum(p['probability']for p in selected)
        raw=sum(p['probability']*c['leaf_values'][p['outcome_id']]for p in selected)
        full=sum(p['probability']*c['leaf_values'][p['outcome_id']]for p in paths)
        assert abs(full-c['expected_value'])<1e-6
        assert abs(sum(p['probability']for p in paths)-1)<1e-6
        row={'cell':[c['row'],c['column']],'full_paths':len(paths),'kept_paths':len(selected),
             'covered_probability':mass,'conditional_value':raw/mass,'full_value':full,
             'difference':raw/mass-full,'first_paths':selected}
        rows.append(row)
        print(f"{c['row']},{c['column']} paths={len(paths)} covered={mass:.4%} truncated={raw/mass:.6f} full={full:.6f} delta={raw/mass-full:+.6f}")
    (out/'comparison.json').write_text(json.dumps({'limit':a.limit,'initial_path_counts':True,'cells':rows},indent=2))


if __name__=='__main__':main()
