"""Profile archived HP-only games with a prebuilt Rust/GPU worker.

Examples (from Model1-experimental):
  python scripts/profile_search.py --game 124 --turn 1 --cell all
  python scripts/profile_search.py --game 124 --turn 1 --cell 7,4
  python scripts/profile_search.py --build  # only when refreshing engine/model code

Cells are 1-based row,column. A normal invocation never invokes Cargo.
Replay is checked against the archive; unsupported/divergent replays fail explicitly.
"""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
SUPPORT = ROOT / "scripts/search_profiler"
ENGINE = ROOT.parent / "Ninjask"
SUITE = ROOT / "Model2/runs/opponent-suite-insample-heldout-20261104"
BIN = SUPPORT / "bin/profile_search.exe"
META = SUPPORT / "bin/build.json"


def source_hash(directory: Path) -> str:
    h = hashlib.sha256()
    for path in sorted(directory.rglob("*.rs")):
        h.update(path.relative_to(directory).as_posix().encode())
        h.update(path.read_bytes())
    return h.hexdigest()


def build() -> None:
    stamp = dt.datetime.now().strftime("%Y%m%d-%H%M%S-%f")
    work = ROOT / "reports/search-profiler-builds" / stamp
    work.mkdir(parents=True)
    shutil.copytree(ROOT / "src", work / "src")
    shutil.copytree(ENGINE / "src", work / "engine/src")
    engine_hash = source_hash(work / "engine/src")
    engine_manifest = (ENGINE / "Cargo.toml").read_text()
    engine_manifest = engine_manifest.replace('version = "0.2.0"', 'version = "0.2.20261105"', 1)
    (work / "engine/Cargo.toml").write_text(engine_manifest)
    manifest = (ROOT / "Cargo.toml").read_text()
    for old in ['path = "../Ninjask"', 'path = "vendor/Ninjask"']:
        manifest = manifest.replace(old, 'path = "engine"')
    manifest = manifest.replace('path = "vendor/rl4burn"', f'path = "{(ROOT / "vendor/rl4burn").as_posix()}"')
    (work / "Cargo.toml").write_text(manifest)
    shutil.copyfile(ROOT / "Cargo.lock", work / "Cargo.lock")
    for name in ["legacy_search.rs", "profile_driver.rs"]:
        shutil.copyfile(SUPPORT / name, work / "src" / name)
    shutil.copyfile(SUPPORT / "cell_profiler.rs", work / "src/model2/cell_profiler.rs")
    main = work / "src/main.rs"
    text = main.read_text().replace("mod model2;", "mod model2;\nmod legacy_search;\nmod profile_driver;", 1)
    anchor = '    println!("Parameters: {}", model.num_params());'
    if anchor not in text:
        raise RuntimeError("main.rs changed: cannot locate profiler dispatch point")
    text = text.replace(anchor, anchor + '''
    if let Ok(path) = std::env::var("MODEL2_PROFILE_CONFIG") {
        let config: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
        return profile_driver::run(&args, dex, teams, model.valid(), &device, &config);
    }
''', 1)
    main.write_text(text)
    model2 = work / "src/model2.rs"
    model2.write_text(model2.read_text() + "\npub(crate) mod cell_profiler;\n")
    lib = work / "engine/src/lib.rs"
    lib.write_text(lib.read_text() + '''
// Isolated profiler instrumentation; never copied into the live engine.
pub static PROFILE_PATHS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static PROFILE_DEADLINE: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
''')
    enum = work / "engine/src/battle/enumerate.rs"
    text = enum.read_text()
    anchor = "        loop {\n            let mut battle = self.clone();"
    if anchor not in text:
        raise RuntimeError("Enumerator changed: cannot instrument path counts")
    text = text.replace(anchor, '''        let mut profile_paths = 0u64;
        loop {
            if profile_paths % 256 == 0 {
                crate::PROFILE_PATHS.store(profile_paths, std::sync::atomic::Ordering::Relaxed);
                if crate::PROFILE_DEADLINE.lock().unwrap().is_some_and(|t| std::time::Instant::now()>t) {
                    return Err(EnumerationError("Per-cell profiling timeout; partial path count".into()));
                }
            }
            let mut battle = self.clone();''', 1)
    text = text.replace("            apply(&mut battle)?;", "            apply(&mut battle)?;\n            profile_paths += 1;", 1)
    text = text.replace("        Ok(outcomes)", "        crate::PROFILE_PATHS.store(profile_paths, std::sync::atomic::Ordering::Relaxed);\n        Ok(outcomes)", 1)
    enum.write_text(text)
    print(f"Building profiler once; build log: {work / 'build.log'}", flush=True)
    with (work / "build.log").open("w") as log:
        result = subprocess.run(["cargo", "build", "--release", "--manifest-path", str(work / "Cargo.toml"),
                                 "--target-dir", str(ROOT / "target")], cwd=ROOT, stdout=log, stderr=subprocess.STDOUT)
    if result.returncode:
        raise RuntimeError(f"Build failed. See {work / 'build.log'}")
    BIN.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(ROOT / "target/release/model1.exe", BIN)
    META.write_text(json.dumps({"built_at":dt.datetime.now().isoformat(),"engine_source_hash":engine_hash,
                               "binary_sha256":hashlib.sha256(BIN.read_bytes()).hexdigest(),"source_snapshot":str(work)}, indent=2))
    print(f"Ready: {BIN}")


def cell_selector(value: str) -> str:
    if value.lower() == "all":
        return "all"
    try:
        row, column = map(int, value.split(","))
        if row > 0 and column > 0:
            return f"{row},{column}"
    except ValueError:
        pass
    raise argparse.ArgumentTypeError("Use all or a 1-based row,column, e.g. 7,4")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--build", action="store_true", help="Explicitly refresh the worker; normal runs never build")
    parser.add_argument("--game", help="Original HP-suite game number, or absolute path to a game trace JSON")
    parser.add_argument("--pool", choices=["training", "heldout"], default="training")
    parser.add_argument("--turn", type=int, default=1)
    parser.add_argument("--cell", type=cell_selector, default="all")
    parser.add_argument("--mode", choices=["PreciceButSame", "AlmostPrecise", "Precise"], default="PreciceButSame")
    parser.add_argument("--timeout-seconds", type=float, default=30, help="Engine limit per cell; 0 disables")
    parser.add_argument("--search-seed", type=int, help="Override deterministic shortlist/world-assumption seed")
    parser.add_argument("--teams", type=Path)
    parser.add_argument("--catalog", type=Path, default=SUITE / "training-teams.txt")
    parser.add_argument("--dex", type=Path, default=SUITE / "dex.json")
    parser.add_argument("--weights", type=Path, default=SUITE / "final.bin")
    parser.add_argument("--out", type=Path, help="New output directory; existing directories are rejected")
    args = parser.parse_args()
    if args.turn < 1 or args.timeout_seconds < 0:
        parser.error("turn must be positive and timeout nonnegative")
    if args.build:
        build()
    if args.game is None:
        if args.build:
            return
        parser.error("--game is required")
    if not BIN.is_file():
        parser.error("Profiler is not built. Run this script once with --build")
    game = SUITE / f"{args.pool}-hp/traces/game-{int(args.game):04}.json" if args.game.isdecimal() else Path(args.game).resolve()
    archive = json.loads(game.read_text())
    teams = args.teams or SUITE / f"{args.pool}-teams.txt"
    weights = args.weights.resolve()
    if weights.suffix == ".bin":
        weights = weights.with_suffix("")
    stamp = dt.datetime.now().strftime("%Y%m%d-%H%M%S-%f")
    out = (args.out or ROOT / "reports/search-profiles" / f"{game.stem}-turn{args.turn}-{stamp}").resolve()
    out.mkdir(parents=True, exist_ok=False)
    meta = json.loads(META.read_text())
    if meta["engine_source_hash"] != source_hash(ENGINE / "src"):
        print("Note: local engine changed since this worker was built. --build refreshes it; this run uses the saved worker.")
    config = {"game":str(game.resolve()),"turn":args.turn,"cell":args.cell,"mode":args.mode,"timeout_seconds":args.timeout_seconds}
    if args.search_seed is not None:
        config["search_seed"] = args.search_seed
    config_path = out / "config.json"
    config_path.write_text(json.dumps(config, indent=2))
    (out / "build.json").write_text(json.dumps(meta, indent=2))
    command = [str(BIN),"--eval-matrix","--weights",str(weights),"--dex",str(args.dex.resolve()),"--teams",str(teams.resolve()),
               "--eval-search-teams",str(args.catalog.resolve()),"--seed",str(archive["match_seed"]),"--out",str(out)]
    env = os.environ.copy();env["MODEL2_PROFILE_CONFIG"] = str(config_path)
    print(f"{args.mode}: {game.name}, turn {args.turn}, cell {args.cell}. One run per cell.\nOutput: {out}", flush=True)
    with (out / "worker.log").open("w") as log:
        proc = subprocess.Popen(command,cwd=ROOT,env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True,encoding="utf-8",errors="replace")
        try:
            for line in proc.stdout:
                log.write(line);log.flush()
                if line.startswith("CELL_RESULT "):
                    row=json.loads(line[len("CELL_RESULT "):])
                    nn=f"{row['nn_seconds']:.6f}s" if "nn_seconds" in row else "not reached"
                    print(f"Cell {row['row']},{row['column']}: paths={row['paths']:,}, engine={row['engine_seconds']:.6f}s, NN={nn}, {row['status']}",flush=True)
                elif line.startswith(("Verified", "Profiling")):
                    print(line.rstrip(),flush=True)
            code=proc.wait()
        except KeyboardInterrupt:
            proc.terminate();proc.wait()
            raise
    if code:
        raise RuntimeError(f"Worker failed ({code}); see {out / 'worker.log'}")
    report=json.loads((out / "profile.json").read_text())
    ranked=sorted(report["cells"],key=lambda r:r.get("measured_total_seconds",r["engine_seconds"]),reverse=True)
    print("\nCell       Paths       States    Engine(s)     NN(s)    Prep(s)  Status")
    for r in ranked:
        nn_text = f"{r['nn_seconds']:.6f}" if "nn_seconds" in r else "-"
        prep_text = f"{r['preparation_seconds']:.6f}" if "preparation_seconds" in r else "-"
        print(f"{r['row']},{r['column']:<5} {r['paths']:>10,} {str(r.get('outcomes','-')):>10} {r['engine_seconds']:>11.6f} "
              f"{nn_text:>9} {prep_text:>10}  {r['status']}")
    print(f"\nFull actions, counts and timing: {out / 'profile.json'}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, RuntimeError, ValueError, subprocess.SubprocessError) as exc:
        sys.exit(str(exc))
