"""Isolated, replay-verified ABBA benchmark of capped-search leaf batching.

Creates a new source snapshot; never patches the training executable. Uses turns
1, 3, 5 and 8 of archived game 124, verifying replay events before benchmarking.
GPU timings include upload/readback and may include contention from live training.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess


def prepare(root, out):
    build = out / 'build'
    frozen = root / 'Model2/runs/fresh-capped16-5h-20261105/build'
    for folder in ['src', 'vendor']:
        shutil.copytree(frozen / folder, build / folder)
    for name in ['Cargo.toml', 'Cargo.lock']:
        shutil.copy2(frozen / name, build / name)
    old = root / 'Model2/runs/turn1-expensive-cell-sampling-20261105/src'
    shutil.copy2(old / 'legacy_search.rs', build / 'src/legacy_search.rs')
    main = (build / 'src/main.rs').read_text()
    main = main.replace('mod model2;', 'mod model2;\nmod legacy_search;\nmod turn_bench;')
    main = main.replace('    let evaluation_start = Instant::now();',
                        '    return turn_bench::run(args,dex,teams,model,device);\n    let evaluation_start = Instant::now();', 1)
    (build / 'src/main.rs').write_text(main)
    driver = (old / 'turn_bench.rs').read_text()
    driver = driver.replace('positions.into_iter().take(1)',
                            'positions.into_iter().filter(|(b,_)| [1,3,5,8].contains(&b.turn))')
    driver = driver.replace('benchmark_archived_turn', 'benchmark_leaf_batching')
    driver = driver.replace('"chance_method":"AlmostPrecise"',
                            '"chance_method":"PreciceButSame, max 16 paths"')
    driver = driver.replace('learner full matrices, shared candidates and one adverse-speed scenario; sampled median of 3 after warmup; enumeration one run; replay excluded',
                            'Identical capped search, per-cell vs cross-cell leaf batches of 64; ABBA twice after warmup; replay excluded; concurrent training may contend for GPU')
    (build / 'src/turn_bench.rs').write_text(driver)
    path = build / 'src/model2.rs'
    source = path.read_text()
    start = source.index('fn search_position_enumerated(')
    end = source.index('\npub fn diagnose(', start)
    fn = source[start:end]
    fn = fn.replace('fn search_position_enumerated(', 'fn search_position_batch_test(')
    fn = fn.replace('    hp_only: bool,', '    hp_only: bool,\n    cross_cell: bool,')
    fn = fn.replace('    let mut benchmark_log_seconds = 0.;', '''
    let mut benchmark_log_seconds = 0.;
    let mut leaf_calls = 0usize;
    let mut leaf_seconds = 0.;
    let mut leaf_values: Vec<f32> = vec![];
    let mut input_hash = std::collections::hash_map::DefaultHasher::new();
    let mut pending_states: Vec<f32> = vec![];
    let mut pending_cells: Vec<(usize,usize,usize,Vec<f64>)> = vec![];
    use std::hash::{Hash, Hasher};
''')
    begin = fn.index('                    if !weights.is_empty() {')
    finish = fn.index('\n                }\n            }\n        }\n    }', begin)
    fn = fn[:begin] + '''                    if !weights.is_empty() {
                        for x in &states { x.to_bits().hash(&mut input_hash); }
                        for x in &weights { x.to_bits().hash(&mut input_hash); }
                        (scenario,i,j).hash(&mut input_hash);
                        if cross_cell {
                            pending_states.extend(states);
                            pending_cells.push((scenario,i,j,weights));
                        } else {
                            let timer = Instant::now();
                            let values = model.state_value(Tensor::from_data(
                                TensorData::new(states, [weights.len(), STATE]), device,
                            )).into_data().to_vec::<f32>().unwrap();
                            leaf_seconds += timer.elapsed().as_secs_f64();
                            leaf_calls += 1;
                            scenario_payoffs[scenario][i][j] += expected_leaf_value(&weights, &values);
                            leaf_values.extend(values);
                        }
                    }''' + fn[finish:]
    marker = '    let (mut payoff, immediate_payoff, worst_speed_scenario) ='
    fn = fn.replace(marker, '''    if cross_cell {
        for chunk in pending_states.chunks(64 * STATE) {
            let timer = Instant::now();
            let values = model.state_value(Tensor::from_data(
                TensorData::new(chunk.to_vec(), [chunk.len()/STATE, STATE]), device,
            )).into_data().to_vec::<f32>().unwrap();
            leaf_seconds += timer.elapsed().as_secs_f64();
            leaf_calls += 1;
            leaf_values.extend(values);
        }
        let mut cursor = 0;
        for (s,i,j,weights) in pending_cells {
            let end = cursor + weights.len();
            scenario_payoffs[s][i][j] += expected_leaf_value(&weights, &leaf_values[cursor..end]);
            cursor = end;
        }
        assert_eq!(cursor,leaf_values.len());
    }
''' + marker)
    fn = fn.replace('    if detailed {\n        let summary', '''    evidence["batch_test"] = json!({"leaf_calls":leaf_calls,"leaf_seconds":leaf_seconds,
        "leaf_values":leaf_values,"input_hash":input_hash.finish()});
    if detailed {
        let summary''')
    production = (root / 'src/model2.rs').read_text()
    prod_start = production.index('fn search_position_enumerated(')
    prod_end = production.index('\npub fn diagnose(', prod_start)
    production_fn = production[prod_start:prod_end].replace(
        'fn search_position_enumerated(', 'fn search_position_production(')
    path.write_text(source + '\n' + fn + '\n' + production_fn + '\n'
                    + (root / 'scripts/benchmark_leaf_batching.rs').read_text())
    return build


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--out', type=Path, required=True)
    ap.add_argument('--weights', type=Path, required=True, help='Checkpoint without .bin')
    args = ap.parse_args()
    root = Path(__file__).resolve().parents[1]
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    build = prepare(root, out)
    suite = root / 'Model2/runs/opponent-suite-insample-heldout-20261104'
    archive = suite / 'training-hp/traces/game-0124.json'
    shutil.copy2(args.weights.with_suffix('.bin'), out / 'weights.bin')
    provenance = {'weights_source':str(args.weights.resolve()),
                  'weights_sha256':hashlib.sha256((out/'weights.bin').read_bytes()).hexdigest(),
                  'archive':str(archive),'archive_sha256':hashlib.sha256(archive.read_bytes()).hexdigest(),
                  'source_hashes':{str(p.relative_to(build)):hashlib.sha256(p.read_bytes()).hexdigest()
                                   for p in build.rglob('*') if p.is_file()}}
    (out/'provenance.json').write_text(json.dumps(provenance,indent=2))
    env = dict(os.environ)
    # Experimental build cache only. Snapshot and executable remain independent.
    env['CARGO_TARGET_DIR'] = str(root / 'target')
    cached_binary = root/'target/release/model1.exe'
    backup = out/'previous-model1.exe'
    if cached_binary.exists():
        shutil.copy2(cached_binary, backup)
    try:
        with (out/'build.log').open('w') as log:
            subprocess.run(['cargo','build','--release','--locked'],cwd=build,env=env,
                           stdout=log,stderr=subprocess.STDOUT,check=True)
        shutil.copy2(cached_binary,out/'benchmark.exe')
    finally:
        if backup.exists():
            shutil.copy2(backup,cached_binary)
    env['ARCHIVED_GAME'] = str(archive)
    cmd = [str(out/'benchmark.exe'),'--weights',str(out/'weights'),'--evaluate','2','--eval-matrix',
           '--eval-search-teams',str(suite/'training-teams.txt'),'--teams',str(suite/'training-teams.txt'),
           '--dex',str(suite/'dex.json'),'--seed','781062','--matrix-samples','2',
           '--out',str(out/'results')]
    with (out/'benchmark.log').open('w') as log:
        subprocess.run(cmd,cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
    report=json.loads((out/'results/comparison.json').read_text())
    rows=report['rows']
    before=sum(r['per_cell_median_seconds'] for r in rows)
    after=sum(r['batched_median_seconds'] for r in rows)
    summary={'positions':len(rows),'per_cell_total_median_seconds':before,
             'batched_total_median_seconds':after,'saved_seconds':before-after,
             'saved_percent':100*(1-after/before),'speedup':before/after,
             'all_outputs_exactly_equal':all(
                 all(r[k]==0 for k in ['max_leaf_abs_difference','max_payoff_abs_difference',
                     'max_policy_abs_difference','max_value_abs_difference','sampling_grid_mismatches'])
                 and r['chosen_action_identical'] for r in rows),
             'scope':report['scope']}
    (out/'results/summary.json').write_text(json.dumps(summary,indent=2))
    print(json.dumps(report,indent=2))


if __name__ == '__main__':
    main()
