//! Diagnostic-only frozen sampled search, preserving the archived replay policy.
use super::*;

pub(super) fn sampled(
    model: &Model<Inner>,
    b: &ninjask::Battle,
    t: &Tracker,
    cs: &[Choice],
    bank: &Catalog,
    args: &Args,
    device: &<Inner as Backend>::Device,
    rng: &mut SmallRng,
    hp_only: bool,
) -> Result<Option<Search>> {
    if b.request != RequestState::Move {
        return Ok(None);
    }
    let side = t.me;
    let mut world = assumed_world(t, bank, rng)?;
    restore_public_history(&mut world, b, side);
    let legal_world: HashSet<_> = env::choices(&world, side)
        .iter()
        .map(|c| format!("{c:?}"))
        .collect();
    let eligible: Vec<_> = cs
        .iter()
        .enumerate()
        .filter(|(_, c)| legal_world.contains(&format!("{c:?}")))
        .map(|(i, _)| i)
        .collect();
    if eligible.is_empty() {
        return Ok(None);
    }
    let own_ids = if hp_only {
        let mut ids = eligible.clone(); ids.shuffle(rng); ids.truncate(N); ids
    } else { shortlist(&scores(model, b, t, cs, device), &eligible, rng) };
    let own: Vec<_> = own_ids.iter().map(|&i| cs[i].clone()).collect();
    let opp_view = opponent_view(t, &world);
    let opp_all = env::choices(&world, 1 - side);
    if opp_all.is_empty() {
        return Ok(None);
    }
    let opp_ids = if hp_only {
        let mut ids: Vec<_> = (0..opp_all.len()).collect(); ids.shuffle(rng); ids.truncate(N); ids
    } else { shortlist(
        &scores(model, &world, &opp_view, &opp_all, device),
        &(0..opp_all.len()).collect::<Vec<_>>(),
        rng,
    )
    };
    let opp: Vec<_> = opp_ids.iter().map(|&i| opp_all[i].clone()).collect();
    let speed_worlds = speed_bound_worlds(&world, t);
    if args.benchmark_chance_turn == b.turn && std::env::var_os("MODEL2_ENGINE_ONLY_BENCHMARK").is_some() {
        let mut benchmark_worlds = speed_worlds.clone();
        if std::env::var_os("NINJASK_COUNT_ENUM_PATHS").is_some() {
            let mut low = benchmark_worlds[0].clone();
            for (k, m) in t.sides[1-side].mons.iter().enumerate() {
                if let Some(bounds) = t.speed_bounds(1-side,k) {
                    if let Some(mon) = low.sides[1-side].mons.iter_mut().find(|p| p.name.as_ref() == m.name) {
                        mon.stats[5] = bounds.min; mon.speed = bounds.min as i32;
                    }
                }
            }
            benchmark_worlds.push(low);
            eprintln!("COUNT_SCENARIOS 0=current_upper_bounds 1=historical_lower_bounds");
        }
        engine_only(&benchmark_worlds, &own, &opp, side, rng, args)?;
    }
    let mut scenario_payoffs = vec![vec![vec![0.; opp.len()]; own.len()]; speed_worlds.len()];
    let mut leaf_states = vec![];
    let mut leaves = vec![];
    let before = env::hp_advantage(&world, side);
    let detailed = (args.search_trace_turn > 0 && b.turn == args.search_trace_turn)
        || (args.inspect_decision > 0 && b.turn == 16
            && b.sides[side].mons.iter().filter(|m| !m.fainted).count() == 1);
    let mut cell_traces = vec![];
    for _ in 0..args.matrix_samples {
        let seed = rng.random::<u64>(); // Common random numbers across the matrix.
        for (scenario, speed_world) in speed_worlds.iter().enumerate() {
        for (i, a) in own.iter().enumerate() {
            for (j, b) in opp.iter().enumerate() {
                let mut w = speed_world.clone();
                w.rng = ninjask::rng::Rng::new(seed);
                w.log_enabled = true;
                w.log.clear();
                env::apply(&mut w, side, a.clone());
                env::apply(&mut w, 1 - side, b.clone());
                if detailed {
                    cell_traces.push(json!({"sample_seed":seed,"speed_scenario":scenario,"row":i,"column":j,
                        "reward":env::battle_reward(&w,side,w.ended,before),
                        "before_hp_potential":before,"after_hp_potential":env::hp_advantage(&w,side),
                        "ended":w.ended,"request":format!("{:?}",w.request),
                        "events":w.log_lines(Viewer::Side(side),0)}));
                }
                scenario_payoffs[scenario][i][j] += if hp_only { 2. * env::hp_advantage(&w, side) as f64 }
                    else { env::battle_reward(&w, side, w.ended, before) as f64 }
                    / args.matrix_samples as f64;
                if !hp_only && !w.ended && w.turn < args.max_turns {
                    let mut next = t.clone();
                    for line in w.log_lines(Viewer::Side(side), 0) {
                        next.feed(&line);
                    }
                    next.feed_engine(&w);
                    leaf_states.extend(env::encode_state(&next));
                    leaves.push((scenario, i, j));
                }
            }
        }
    }
    }
    let scenario_immediate = scenario_payoffs.clone();
    if !leaves.is_empty() {
        let values = model
            .state_value(Tensor::from_data(
                TensorData::new(leaf_states, [leaves.len(), STATE]),
                device,
            ))
            .into_data()
            .to_vec::<f32>()
            .unwrap();
        for ((scenario, i, j), v) in leaves.into_iter().zip(values) {
            scenario_payoffs[scenario][i][j] += v.clamp(-2., 2.) as f64 / args.matrix_samples as f64;
        }
    }
    let (mut payoff, immediate_payoff, worst_speed_scenario) =
        worst_speed_cells(&scenario_payoffs, &scenario_immediate);
    let bootstrap_payoff: Vec<Vec<f64>> = payoff
        .iter()
        .zip(&immediate_payoff)
        .map(|(row, immediate)| {
            row.iter()
                .zip(immediate)
                .map(|(total, reward)| total - reward)
                .collect()
        })
        .collect();
    for row in &mut payoff {
        for x in row {
            *x = x.clamp(-2., 2.);
        }
    }
    let (p, q, value, gap) = solve(&payoff, args.regret_iterations);
    let selected = draw(&p, rng);
    let mut policy = vec![0.; N];
    let mut mask = vec![-1e9; N];
    for i in 0..own.len() {
        policy[i] = p[i] as f32;
        mask[i] = 0.;
    }
    let mut evidence = json!({"turn":b.turn,"side":side,"state":crate::diagnosis::state(t),
        "candidates":own.iter().map(|a|crate::diagnosis::choice(b,t,a,cs)).collect::<Vec<_>>(),
        "payoff":payoff,"policy":p,"opponent_policy":q,"value":value,"duality_gap":gap,
        "immediate_payoff":immediate_payoff,"bootstrap_payoff":bootstrap_payoff,
        "opponent_candidates":opp.iter().map(|a|crate::diagnosis::choice(&world,&opp_view,a,&opp_all)).collect::<Vec<_>>(),
        "cell_traces":cell_traces,
        "speed_bound_scenarios":speed_worlds.iter().map(|w| w.sides[1-side].mons.iter()
            .map(|m| json!({"name":m.name.as_ref(),"speed_stat":m.stats[5]})).collect::<Vec<_>>()).collect::<Vec<_>>(),
        "worst_speed_scenario":worst_speed_scenario,
        "speed_aggregation":"single adverse-speed scenario: opponent lower bounds in Trick Room, upper bounds otherwise",
        "chosen_candidate":selected,"world_source":"Tracker plus catalog assumptions; no actual private opponent set"});
    if detailed {
        let summary = |battle: &ninjask::Battle| battle.sides.iter().map(|s| s.mons.iter().map(|m|
            json!({"name":m.name.as_ref(),"hp":m.hp,"stats":m.stats,"speed":m.speed,
                "active":m.is_active,"fainted":m.fainted,"disable":m.vol.disable,
                "stall_counter":m.vol.stall_counter})).collect::<Vec<_>>()).collect::<Vec<_>>();
        // Diagnostic output only: actual private stats never enter search/inference.
        evidence["diagnostic_reconstructed"] = json!(summary(&world));
        evidence["diagnostic_actual"] = json!(summary(b));
    }

    Ok(Some(Search {
        example: Example {
            obs: env::observe_padded(b, t, &own, N),
            policy,
            value: value as f32,
            mask,
        },
        chosen: own_ids[selected],
        gap,
        cells: own.len() * opp.len() * args.matrix_samples * speed_worlds.len(),
        evidence,
    }))
}

// Diagnostic only: fixed shortlists/worlds; no model calls or encoding in timed loops.
fn engine_only(worlds: &[ninjask::Battle], own: &[Choice], opp: &[Choice], side: usize,
    rng: &SmallRng, args: &Args) -> Result<()> {
    let mut times = Vec::new();
    for _ in 0..3 {
        let mut repeat_rng = rng.clone();
        let start = Instant::now();
        for _ in 0..args.matrix_samples {
            let seed = repeat_rng.random::<u64>();
            for origin in worlds {
                for a in own { for b in opp {
                    let mut w = origin.clone();
                    w.rng = ninjask::rng::Rng::new(seed);
                    w.log_enabled = true;
                    w.log.clear();
                    env::apply(&mut w, side, a.clone());
                    env::apply(&mut w, 1-side, b.clone());
                    std::hint::black_box(&w);
                }}
            }
        }
        times.push(start.elapsed().as_secs_f64());
    }
    let base = json!({"sampled_engine_seconds":times,"sampling_turns":own.len()*opp.len()*worlds.len()*args.matrix_samples,
        "enumeration_calls_planned":own.len()*opp.len()*worlds.len(),"speed_scenarios":worlds.len(),
        "scope":"engine cloning, RNG, simulation and engine event logging only; no neural calls, tracker encoding, reward or solve; shortlist setup excluded"});
    std::fs::write(args.out.join("engine-only.json"),serde_json::to_vec_pretty(&base)?)?;
    eprintln!("ENGINE_ONLY {}", base);
    for (s, world) in worlds.iter().enumerate() {
        let mut origin = world.clone(); origin.log_enabled = true; origin.log.clear();
        for (i,a) in own.iter().enumerate() { for (j,b) in opp.iter().enumerate() {
            if std::env::var_os("NINJASK_COUNT_ENUM_PATHS").is_some() && (i != 0 || j != 0) { continue; }
            let (Choice::Turn(ours),Choice::Turn(theirs)) = (a,b) else { anyhow::bail!("Turn choices required"); };
            let (p1,p2) = if side == 0 {(ours,theirs)} else {(theirs,ours)};
            eprintln!("ENGINE_ENUM_START {s} {i} {j}");
            let start = Instant::now();
            let outcomes = origin.enumerate_outcomes(p1,p2,ninjask::EnumerationMode::AlmostPrecise)?;
            let seconds = start.elapsed().as_secs_f64();
            eprintln!("ENGINE_ENUM_DONE {}",json!({"scenario":s,"row":i,"column":j,"seconds":seconds,"outcomes":outcomes.len(),"nonterminal":outcomes.iter().filter(|o|!o.battle.ended).count()}));
        }}
    }
    std::process::exit(0);
}
