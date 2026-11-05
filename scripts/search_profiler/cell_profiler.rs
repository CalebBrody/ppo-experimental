use super::*;
use std::io::Write;

pub(crate) fn profile(model:&Model<Inner>,b:&ninjask::Battle,t:&Tracker,bank:&Catalog,args:&Args,
    device:&<Inner as Backend>::Device,cfg:&serde_json::Value,verified:usize)->Result<()> {
    let side=t.me;let cs=env::choices(b,side);
    let seed=cfg["search_seed"].as_u64().unwrap_or(args.seed ^ ((b.turn as u64)<<32) ^ 0xA137);
    let mut rng=SmallRng::seed_from_u64(seed);
    let mut world=assumed_world(t,bank,&mut rng)?;restore_public_history(&mut world,b,side);
    let legal:HashSet<_>=env::choices(&world,side).iter().map(|c|format!("{c:?}")).collect();
    let eligible:Vec<_>=cs.iter().enumerate().filter(|(_,c)|legal.contains(&format!("{c:?}"))).map(|(i,_)|i).collect();
    let own_ids=shortlist(&scores(model,b,t,&cs,device),&eligible,&mut rng);
    let own:Vec<_>=own_ids.iter().map(|&i|cs[i].clone()).collect();
    let opp_view=opponent_view(t,&world);let opp_all=env::choices(&world,1-side);
    let opp_ids=shortlist(&scores(model,&world,&opp_view,&opp_all,device),&(0..opp_all.len()).collect::<Vec<_>>(),&mut rng);
    let opp:Vec<_>=opp_ids.iter().map(|&i|opp_all[i].clone()).collect();
    let worlds=speed_bound_worlds(&world,t);ensure!(worlds.len()==1,"Profiler expects one adverse-speed scenario");
    let mut origin=worlds[0].clone();origin.log_enabled=true;origin.log.clear();
    let mode_name=cfg["mode"].as_str().unwrap_or("PreciceButSame");
    let mode=match mode_name {"PreciceButSame"=>ninjask::EnumerationMode::PreciceButSame,
        "AlmostPrecise"=>ninjask::EnumerationMode::AlmostPrecise,"Precise"=>ninjask::EnumerationMode::Precise,
        other=>anyhow::bail!("Unknown mode {other}")};
    let selector=cfg["cell"].as_str().unwrap_or("all");
    let chosen=if selector=="all" {None} else {
        let parts:Vec<_>=selector.split(',').collect();ensure!(parts.len()==2,"Cell must be row,column or all");
        let row:usize=parts[0].parse()?;let col:usize=parts[1].parse()?;
        ensure!(row>=1 && row<=own.len() && col>=1 && col<=opp.len(),"Cell outside {}x{} matrix",own.len(),opp.len());
        Some((row-1,col-1))
    };
    // Warm the ordinary 64-state value batch outside all per-cell timers.
    let _=model.state_value(Tensor::from_data(TensorData::new(vec![0f32;64*STATE],[64,STATE]),device)).into_data();
    Inner::sync(device).unwrap();
    let mut warmed_sizes=HashSet::from([64usize]);
    let before=env::hp_advantage(&origin,side);
    let mut results=Vec::new();let mut stream=std::fs::File::create(args.out.join("cells.jsonl"))?;
    let timeout=cfg["timeout_seconds"].as_f64().unwrap_or(30.);
    let matrix=json!({"rows":own.len(),"columns":opp.len(),"own":own.iter().map(|a|crate::diagnosis::choice(b,t,a,&cs)).collect::<Vec<_>>(),
        "opponent":opp.iter().map(|a|crate::diagnosis::choice(&world,&opp_view,a,&opp_all)).collect::<Vec<_>>()});
    std::fs::write(args.out.join("matrix.json"),serde_json::to_vec_pretty(&matrix)?)?;
    for (i,a) in own.iter().enumerate(){for(j,foe)in opp.iter().enumerate(){
        if chosen.is_some_and(|cell|cell!=(i,j)){continue;}
        let(Choice::Turn(ours),Choice::Turn(theirs))=(a,foe)else{anyhow::bail!("Expected turn choices")};
        let(p1,p2)=if side==0{(ours,theirs)}else{(theirs,ours)};
        eprintln!("Profiling cell {},{}",i+1,j+1);
        ninjask::PROFILE_PATHS.store(0,std::sync::atomic::Ordering::Relaxed);
        let start=Instant::now();
        *ninjask::PROFILE_DEADLINE.lock().unwrap()=if timeout>0.{Some(start+std::time::Duration::from_secs_f64(timeout))}else{None};
        let outcome_result=origin.enumerate_outcomes(p1,p2,mode);
        let engine_seconds=start.elapsed().as_secs_f64();
        *ninjask::PROFILE_DEADLINE.lock().unwrap()=None;
        let mut row=json!({"row":i+1,"column":j+1,"paths":ninjask::PROFILE_PATHS.load(std::sync::atomic::Ordering::Relaxed),
            "engine_seconds":engine_seconds,"own":matrix["own"][i],"opponent":matrix["opponent"][j]});
        match outcome_result {
            Err(err)=>{row["status"]=json!("incomplete");row["error"]=json!(err.to_string());},
            Ok(outcomes)=>{
                let mass:f64=outcomes.iter().map(|o|o.probability).sum();ensure!((mass-1.).abs()<1e-6,"Invalid probability mass {mass}");
                let mut nn=0.;let mut prep=0.;let mut warmup=0.;let mut states_count=0;let mut batches=0;let mut value=0.;
                for chunk in outcomes.chunks(64){
                    let pstart=Instant::now();let mut states=Vec::new();let mut weights=Vec::new();
                    for o in chunk{
                        value+=o.probability*env::battle_reward(&o.battle,side,o.battle.ended,before)as f64;
                        if !o.battle.ended && o.battle.turn<args.max_turns{
                            let mut next=t.clone();for line in o.battle.log_lines(Viewer::Side(side),0){next.feed(&line);}next.feed_engine(&o.battle);
                            states.extend(env::encode_state(&next));weights.push(o.probability);
                        }
                    }
                    prep+=pstart.elapsed().as_secs_f64();
                    if !weights.is_empty(){
                        if warmed_sizes.insert(weights.len()) {
                            let warm_start=Instant::now();
                            let _=model.state_value(Tensor::from_data(TensorData::new(vec![0f32;weights.len()*STATE],[weights.len(),STATE]),device)).into_data();
                            Inner::sync(device).unwrap();warmup+=warm_start.elapsed().as_secs_f64();
                        }
                        Inner::sync(device).unwrap();let nstart=Instant::now();
                        let values=model.state_value(Tensor::from_data(TensorData::new(states,[weights.len(),STATE]),device)).into_data().to_vec::<f32>().unwrap();
                        Inner::sync(device).unwrap();nn+=nstart.elapsed().as_secs_f64();
                        states_count+=weights.len();batches+=1;value+=expected_leaf_value(&weights,&values);
                    }
                }
                row["status"]=json!("completed");row["outcomes"]=json!(outcomes.len());row["nn_states"]=json!(states_count);
                row["nn_batches"]=json!(batches);row["nn_seconds"]=json!(nn);row["preparation_seconds"]=json!(prep);
                row["shape_warmup_seconds"]=json!(warmup);
                row["probability_mass"]=json!(mass);row["expected_value"]=json!(value);row["measured_total_seconds"]=json!(engine_seconds+prep+nn);
            }
        }
        writeln!(stream,"{}",row)?;stream.flush()?;println!("CELL_RESULT {}",row);results.push(row);
    }}
    let report=json!({"game":cfg["game"],"turn":b.turn,"mode":mode_name,"search_seed":seed,"verified_replay_decisions":verified,
        "matrix":matrix,"cells":results,"scope":"one run per cell; one adverse-speed world; NN upload, forward and readback included; replay, policy shortlisting and warmup excluded"});
    std::fs::write(args.out.join("profile.json"),serde_json::to_vec_pretty(&report)?)?;Ok(())
}
