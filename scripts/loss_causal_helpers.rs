// Diagnostic-only counterfactuals. Never used to choose the played action.
fn repair_world(world:&ninjask::Battle, actual:&ninjask::Battle, side:usize, mode:&str)->Option<ninjask::Battle> {
    let mut w=world.clone(); let mut changed=false;
    macro_rules! copy { ($x:expr,$y:expr) => { if $x!=$y { $x=$y;changed=true; } }; }
    for s in 0..2 { for m in &mut w.sides[s].mons {
        let Some(a)=actual.sides[s].mons.iter().find(|a|a.name==m.name) else {continue};
        match mode {
            "own_set" if s==side => {
                copy!(m.points,a.points);copy!(m.nature_plus,a.nature_plus);copy!(m.nature_minus,a.nature_minus);
                m.nature_name=a.nature_name;
            },
            "protean_libero" if m.is_active && m.ability==a.ability => {
                copy!(m.ability_state.protean,a.ability_state.protean);
                copy!(m.ability_state.libero,a.ability_state.libero);
            },
            "unnerve" if m.is_active && m.ability==a.ability => {copy!(m.ability_state.unnerved,a.ability_state.unnerved);},
            "charge" if m.is_active && m.vol.two_turn.is_some() => {
                copy!(m.vol.two_turn_duration,a.vol.two_turn_duration);
                copy!(m.vol.charging_target_loc,a.vol.charging_target_loc);
                copy!(m.vol.charging_duration,a.vol.charging_duration);
            },
            "encore" if m.is_active => {copy!(m.vol.encore,a.vol.encore);copy!(m.vol.encore_duration,a.vol.encore_duration);},
            "stockpile" if m.is_active => {copy!(m.vol.stockpile_def,a.vol.stockpile_def);copy!(m.vol.stockpile_spd,a.vol.stockpile_spd);},
            "move_result" if m.is_active && m.moves.iter().any(|mv|["stompingtantrum","temperflare"].contains(&world.dex.mv(mv.id).id.as_str())) => {copy!(m.move_last_turn_result,a.move_last_turn_result);},
            "rage_fist" if m.is_active && m.moves.iter().any(|mv|world.dex.mv(mv.id).id=="ragefist") => {copy!(m.times_attacked,a.times_attacked);},
            "last_item" if m.is_active && m.moves.iter().any(|mv|["recycle","belch"].contains(&world.dex.mv(mv.id).id.as_str())) => {copy!(m.last_item,a.last_item);copy!(m.ate_berry,a.ate_berry);},
            "action_count" if m.is_active => {copy!(m.active_move_actions,a.active_move_actions);},
            _=>{}
        }
    }}
    if mode=="field_timers" {
        copy!(w.field.weather_turns,actual.field.weather_turns);
        copy!(w.field.terrain_turns,actual.field.terrain_turns);
        copy!(w.field.trick_room,actual.field.trick_room);
    }
    changed.then_some(w)
}
fn causal_leaf(b:&ninjask::Battle,t:&Tracker,side:usize,max:u32)->Option<Vec<f32>> {
    if b.ended || b.turn>=max {return None}
    let mut next=t.clone();for line in b.log_lines(Viewer::Side(side),0){next.feed(&line);}next.feed_engine(b);
    Some(env::encode_state(&next))
}
type CausalCell=(u64,usize,usize,Vec<String>,Option<Vec<f32>>,f32);
fn causal_audit(model:&Model<Inner>,world:&ninjask::Battle,actual:&ninjask::Battle,t:&Tracker,
    own:&[Choice],opp:&[Choice],args:&Args,device:&<Inner as Backend>::Device,
    baseline:&[CausalCell],base_value:f64,base_policy:&[f64])->serde_json::Value {
    let side=t.me;let mut reports=vec![];
    for mode in ["own_set","protean_libero","unnerve","charge","encore","stockpile","move_result","rage_fist","last_item","action_count","field_timers"] {
        let Some(repaired)=repair_world(world,actual,side,mode) else {continue};
        let mut matrix=vec![vec![0.;opp.len()];own.len()];let mut states=vec![];let mut leaves=vec![];
        let mut events_changed=0;let mut features_changed=0;let mut rewards_changed=0;let mut example=serde_json::Value::Null;
        let before=env::hp_advantage(&repaired,side);
        for (seed,i,j,events,features,reward) in baseline {
            let mut w=repaired.clone();w.rng=ninjask::rng::Rng::new(*seed);w.log_enabled=true;w.log.clear();
            env::apply(&mut w,side,own[*i].clone());env::apply(&mut w,1-side,opp[*j].clone());
            let ev=w.log_lines(Viewer::Side(side),0);let ft=causal_leaf(&w,t,side,args.max_turns);
            let rew=env::battle_reward(&w,side,w.ended,before);
            events_changed+=(ev!=*events) as usize;features_changed+=(ft!=*features) as usize;rewards_changed+=(rew!=*reward) as usize;
            if example.is_null() && (ft!=*features || rew!=*reward) {
                example=json!({"seed":seed,"row":i,"column":j,"before_events":events,"after_events":ev,
                    "before_reward":reward,"after_reward":rew,"after_state":full_audit(&w)});
            }
            matrix[*i][*j]+=rew as f64/args.matrix_samples as f64;
            if let Some(f)=ft {states.extend(f);leaves.push((*i,*j));}
        }
        if !leaves.is_empty() {
            let values=model.state_value(Tensor::from_data(TensorData::new(states,[leaves.len(),STATE]),device)).into_data().to_vec::<f32>().unwrap();
            for ((i,j),v) in leaves.into_iter().zip(values){matrix[i][j]+=v.clamp(-2.,2.) as f64/args.matrix_samples as f64;}
        }
        for row in &mut matrix {for v in row{*v=v.clamp(-2.,2.);}}
        let (p,q,value,gap)=solve(&matrix,args.regret_iterations);
        reports.push(json!({"mode":mode,"simulations":baseline.len(),"events_changed":events_changed,"features_changed":features_changed,
            "rewards_changed":rewards_changed,"before_value":base_value,"after_value":value,"value_delta":value-base_value,
            "before_policy":base_policy,"after_policy":p,"opponent_policy":q,"duality_gap":gap,"payoff":matrix,"example":example}));
    }
    json!(reports)
}
