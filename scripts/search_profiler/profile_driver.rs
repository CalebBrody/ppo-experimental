use crate::*;
use ninjask::{Battle, tracker::Tracker, battle::log::Viewer, state::RequestState};
use rand::{RngExt, SeedableRng, rngs::SmallRng};

pub(crate) fn run(args:&Args, dex:Arc<Dex>, teams:Arc<Vec<Team>>, model:Model<Inner>,
    device:&<Inner as Backend>::Device, config:&serde_json::Value)->Result<()> {
    let archive:serde_json::Value=serde_json::from_slice(&std::fs::read(config["game"].as_str().context("game path")?)?)?;
    let target=config["turn"].as_u64().context("turn")? as u32;
    let side=archive["learner_side"].as_u64().context("learner_side")? as usize;
    ensure!(side<2,"Invalid learner side");
    let seed=archive["match_seed"].as_u64().context("match_seed")?;
    let bank=model2::catalog_path(args.eval_search_teams.as_ref().context("catalog teams")?,&dex)?;
    let mut rng=SmallRng::seed_from_u64(seed);
    let a=rng.random_range(0..teams.len()); let c=rng.random_range(0..teams.len());
    ensure!(a==archive["team_a"].as_u64().unwrap() as usize && c==archive["team_b"].as_u64().unwrap() as usize,
        "Team indices differ: use the original trace's team file");
    let mut b=Battle::from_teams(dex.clone(),[&teams[a],&teams[c]],["p1","p2"],rng.random());
    let mut ts=[Tracker::new(dex.clone(),0),Tracker::new(dex.clone(),1)];let mut cursor=0;
    let mut foe_rng=SmallRng::seed_from_u64(seed ^ 0xB237);let mut verified=0;
    for d in archive["decisions"].as_array().context("decisions")? {
        env::sync(&b,&mut ts,&mut cursor);
        ensure!(b.turn==d["turn"].as_u64().unwrap() as u32,"Replay turn mismatch");
        if b.turn==target && b.request==RequestState::Move {
            eprintln!("Verified {} archived decisions; profiling turn {}",verified,target);
            return model2::cell_profiler::profile(&model,&b,&ts[side],&bank,args,device,config,verified);
        }
        let log_start=b.log.len();let mut selected=Vec::new();
        for s in 0..2 {
            if !b.side_needs_choice(s){continue;}
            let cs=env::choices(&b,s);
            let i=if s==side {d["selected_index"].as_u64().context("learner selection missing")? as usize}
                else {match legacy_search::search_position_mode(&model,&b,&ts[s],&cs,&bank,args,device,&mut foe_rng,true)? {
                    Some(result)=>result.chosen,None=>foe_rng.random_range(0..cs.len())
                }};
            ensure!(i<cs.len(),"Archived choice out of range");selected.push((s,cs[i].clone()));
        }
        for (s,choice) in selected {env::apply(&mut b,s,choice);}
        let events=b.log_lines(Viewer::Side(side),log_start);
        if serde_json::json!(events)!=d["events"] {
            std::fs::write(args.out.join("replay-mismatch.json"),serde_json::to_vec_pretty(&serde_json::json!({"decision":d["decision"],"expected":d["events"],"actual":events}))?)?;
            anyhow::bail!("Replay diverged at decision {}. No timings are reported for a different position; see replay-mismatch.json. This replay adapter supports the original sampled HP-only suite.",d["decision"]);
        }
        verified+=1;
    }
    anyhow::bail!("No move request for turn {target} in the archived game")
}
