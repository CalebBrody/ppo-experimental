"""Report phase wall times. Parallel evaluation time must not be added to trainer time."""
import argparse
import json
from pathlib import Path


def summarize(path):
    path=Path(path)
    training=path/'training' if (path/'training').is_dir() else path
    metrics_path=training/'metrics.jsonl'
    rows=[json.loads(line) for line in metrics_path.read_text().splitlines()] if metrics_path.exists() else []
    phase_path=training/'timing.json'
    phase=json.loads(phase_path.read_text()) if phase_path.exists() else {}
    last=rows[-1] if rows else {}
    train_games=phase.get('training_games_seconds',last.get('training_games_seconds'))
    model=phase.get('model_training_seconds',last.get('model_training_seconds'))
    combined=phase.get('training_wall_seconds',last.get('seconds'))
    other=max(0,combined-train_games-model) if all(v is not None for v in (combined,train_games,model)) else None
    paths=sorted(path.rglob('evaluation.json'))
    total=0.;timed=0;games=0
    for file in paths:
        if 'source' in file.relative_to(path).parts:
            continue
        evaluation=json.loads(file.read_text())
        games+=evaluation['games']
        seconds=evaluation.get('evaluation_games_seconds')
        if seconds is None and (file.parent/'timing.json').exists():
            seconds=json.loads((file.parent/'timing.json').read_text()).get('evaluation_games_seconds')
        if seconds is not None:
            total+=seconds;timed+=1
    evaluations=sum('source' not in file.relative_to(path).parts for file in paths)
    state_path=path/'status.json'
    state=json.loads(state_path.read_text()) if state_path.exists() else {}
    wall=state.get('finished_unix',0)-state.get('started_unix',0) if state.get('finished_unix') and state.get('started_unix') else None
    return {'training_games_seconds':train_games,'model_training_seconds':model,
            'training_other_seconds':other,'combined_training_seconds':combined,
            'evaluation_games_seconds':total if timed==evaluations else None,
            'measured_online_audit_seconds':phase.get('measured_audit_seconds',(last.get('online_audit') or {}).get('measured_audit_seconds')),
            'measured_evaluation_games_seconds':total,'evaluations_timed':timed,'evaluations_total':evaluations,
            'training_games':last.get('games',phase.get('games')),'evaluation_games':games,
            'supervised_wall_seconds':wall,
            'definitions':{'training_games':'Rollout collection: battle simulation, both policies inference, observations, resets and GAE.',
                           'model_training':'PPO optimization: policy/value forward passes, backpropagation and optimizer updates.',
                           'evaluation_games':'Inference and battle simulation during evaluation, including optional trace generation/writes.',
                           'training_other':'Trainer logging, checkpointing and other work outside collection/optimization.'},
            'limitations':['Timers measure wall-clock phase duration, not GPU utilization or CPU/GPU kernel time.',
                           'Training-game and model-training phases are sequential within one trainer.',
                           'Evaluation processes may overlap training and each other; summed process times are not end-to-end elapsed time.',
                           'Missing historical timers remain null; no phase split is inferred from losses or game counts.']}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('path',type=Path)
    args=parser.parse_args()
    report=summarize(args.path)
    (args.path/'timing-summary.json').write_text(json.dumps(report,indent=2)+'\n')
    for label,key in [('Training games','training_games_seconds'),('Model training','model_training_seconds'),('Evaluation games','evaluation_games_seconds'),('Training other','training_other_seconds'),('Combined trainer','combined_training_seconds')]:
        value=report[key]
        print(f'{label}: '+(f'{value:.3f}s ({value/60:.2f} min)' if value is not None else 'not recorded'))
    print(f'Evaluation timing coverage: {report["evaluations_timed"]}/{report["evaluations_total"]}; process times can overlap.')


if __name__=='__main__':main()
