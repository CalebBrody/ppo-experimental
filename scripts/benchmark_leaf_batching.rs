// Included only in the isolated benchmark snapshot by benchmark_leaf_batching.py.
pub(crate) fn benchmark_leaf_batching(
    model: &Model<Inner>, b: &ninjask::Battle, t: &Tracker, bank: &Catalog,
    args: &Args, device: &<Inner as Backend>::Device,
) -> Result<serde_json::Value> {
    let cs = env::choices(b,t.me);
    let seed = args.seed ^ (b.turn as u64 * 7919);
    let run = |batched: bool| -> Result<(Search,f64)> {
        let mut rng = SmallRng::seed_from_u64(seed);
        Inner::sync(device).map_err(|e| anyhow::anyhow!("sync: {e:?}"))?;
        let timer = Instant::now();
        let s = search_position_batch_test(model,b,t,&cs,bank,args,device,&mut rng,false,batched)?
            .context("Expected searched move position")?;
        Inner::sync(device).map_err(|e| anyhow::anyhow!("sync: {e:?}"))?;
        Ok((s,timer.elapsed().as_secs_f64()))
    };
    let (reference,_) = run(false)?;
    // Verify the actual current production implementation, not only the benchmark
    // variant. Its candidates, probability masses and solved output must match.
    let mut production_rng = SmallRng::seed_from_u64(seed);
    let production = search_position_production(model,b,t,&cs,bank,args,device,&mut production_rng,false)?
        .context("Expected production search")?;
    for key in ["candidates","opponent_candidates","outcome_probability_mass",
                "immediate_payoff","payoff","policy","opponent_policy","value","worst_speed_scenario"] {
        ensure!(production.evidence[key]==reference.evidence[key],"Production parity failed: {key}");
    }
    ensure!(production.chosen==reference.chosen,"Production selected action differs");
    // HP-only also exercises the empty leaf queue.
    let mut hp_rng = SmallRng::seed_from_u64(seed);
    let hp = search_position_production(model,b,t,&cs,bank,args,device,&mut hp_rng,true)?
        .context("Expected HP search")?;
    ensure!(hp.evidence["leaf_value_calls"]==0 && hp.evidence["leaf_value_states"]==0,
        "HP-only search unexpectedly evaluated neural leaves");
    run(true)?; // Warm both batching shapes before timed ABBA passes.
    let mut timings = [Vec::new(),Vec::new()];
    let mut leaf_timings = [Vec::new(),Vec::new()];
    let mut calls = [0,0];
    let (mut leaf_diff,mut payoff_diff,mut policy_diff,mut value_diff)=(0f64,0f64,0f64,0f64);
    let mut chosen_identical = true;
    let mut grid_mismatches = 0;
    fn flatten(v:&serde_json::Value,out:&mut Vec<f64>) {
        if let Some(a)=v.as_array() { for x in a {flatten(x,out);} }
        else if let Some(x)=v.as_f64() {out.push(x);}
    }
    let diff = |a:&serde_json::Value,b:&serde_json::Value| -> f64 {
        let (mut x,mut y)=(vec![],vec![]); flatten(a,&mut x);flatten(b,&mut y);
        assert_eq!(x.len(),y.len());
        x.iter().zip(y).map(|(a,b)|(a-b).abs()).fold(0.,f64::max)
    };
    for batched in [false,true,true,false,false,true,true,false] {
        let (s,seconds)=run(batched)?;
        let k=usize::from(batched);
        timings[k].push(seconds);
        leaf_timings[k].push(s.evidence["batch_test"]["leaf_seconds"].as_f64().unwrap());
        calls[k]=s.evidence["batch_test"]["leaf_calls"].as_u64().unwrap();
        for key in ["candidates","opponent_candidates","outcome_probability_mass","immediate_payoff","speed_bound_scenarios"] {
            ensure!(s.evidence[key]==reference.evidence[key],"Different search input {key}");
        }
        ensure!(s.evidence["batch_test"]["input_hash"]==reference.evidence["batch_test"]["input_hash"],"Encoded leaves/weights differ");
        leaf_diff=leaf_diff.max(diff(&s.evidence["batch_test"]["leaf_values"],&reference.evidence["batch_test"]["leaf_values"]));
        payoff_diff=payoff_diff.max(diff(&s.evidence["payoff"],&reference.evidence["payoff"]));
        policy_diff=policy_diff.max(diff(&s.evidence["policy"],&reference.evidence["policy"]));
        policy_diff=policy_diff.max(diff(&s.evidence["opponent_policy"],&reference.evidence["opponent_policy"]));
        value_diff=value_diff.max((s.example.value-reference.example.value).abs() as f64);
        chosen_identical &= s.chosen==reference.chosen;
        let p:Vec<f64>=s.evidence["policy"].as_array().unwrap().iter().map(|x|x.as_f64().unwrap()).collect();
        let q:Vec<f64>=reference.evidence["policy"].as_array().unwrap().iter().map(|x|x.as_f64().unwrap()).collect();
        let choose=|ps:&[f64],u:f64| {let mut sum=0.;for (i,x) in ps.iter().enumerate(){sum+=x;if u<sum{return i;}}ps.len()-1};
        for n in 0..10000 {let u=(n as f64+0.5)/10000.; if choose(&p,u)!=choose(&q,u){grid_mismatches+=1;} }
    }
    let median=|x:&Vec<f64>| {let mut v=x.clone();v.sort_by(f64::total_cmp);(v[1]+v[2])/2.};
    let a=median(&timings[0]);let c=median(&timings[1]);
    let result=json!({"turn":b.turn,"rows":reference.evidence["candidates"].as_array().unwrap().len(),
        "columns":reference.evidence["opponent_candidates"].as_array().unwrap().len(),
        "successors":reference.cells,"leaf_states":reference.evidence["batch_test"]["leaf_values"].as_array().unwrap().len(),
        "per_cell_calls":calls[0],"batched_calls":calls[1],"per_cell_seconds":timings[0],"batched_seconds":timings[1],
        "per_cell_median_seconds":a,"batched_median_seconds":c,"speedup":a/c,"saved_percent":100.*(1.-c/a),
        "per_cell_leaf_median_seconds":median(&leaf_timings[0]),"batched_leaf_median_seconds":median(&leaf_timings[1]),
        "max_leaf_abs_difference":leaf_diff,"max_payoff_abs_difference":payoff_diff,
        "max_policy_abs_difference":policy_diff,"max_value_abs_difference":value_diff,
        "chosen_action_identical":chosen_identical,"sampling_grid_mismatches":grid_mismatches,
        "identical_encoded_inputs_and_probabilities":true,"production_exact_parity":true,
        "production_leaf_value_calls":production.evidence["leaf_value_calls"],
        "hp_only_zero_value_calls":true,
        "numerically_equivalent":leaf_diff<1e-5 && payoff_diff<1e-6 && policy_diff<1e-5 && value_diff<1e-6});
    ensure!(result["numerically_equivalent"]==true && chosen_identical && grid_mismatches==0,"Parity failed: {result}");
    eprintln!("BATCH_RESULT {result}");
    Ok(result)
}
