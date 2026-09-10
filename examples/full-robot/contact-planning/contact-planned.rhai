// Robot policy only. Shared Rust supplies trajectories, contact phases and lease validation.
fn pause_remaining(phase, direction, p) {
    for window in p.pause_windows_s {
        let q=phase;
        if q<window[0] {q+=p.period_s;}
        if q>=window[0] && q<=window[1] {
            return if direction>0.0 {window[1]-q}else{q-window[0]};
        }
    }
    -1.0
}
fn control(t,sensors,commands,state) {
    let p=parameters_except(["trajectory","static_feedforward","dynamic_feedforward","velocity_feedforward"]);
    if !state.contains("phase") {
        state.phase=p.initial_phase_s;state.rate=0.0;state.last_t=t;
        state.braking=false;state.hold_until=0.0;state.yaw_offsets=[];
        for index in p.hip_motor_indices {state.yaw_offsets.push(0.0);}
        state.lease=#{sequence:-1.0,age_s:p.command_lease.timeout_s};
    }
    state.lease=command_lease_update(state.lease.sequence,state.lease.age_s,
        sensors["command.packet_sequence"],p.command_lease);
    let dt=t-state.last_t;state.last_t=t;
    let advance=dt*state.rate;
    if state.braking {
        let remaining=pause_remaining(state.phase,state.rate,p);
        if remaining<0.0 {throw "braking clock left its declared pause window";}
        if advance.abs()>=remaining {
            advance=advance.sign()*(remaining-0.000000001).max(0.0);
            state.rate=0.0;
        }
    }
    state.phase=(state.phase+advance)%p.period_s;
    if state.phase<0.0 {state.phase+=p.period_s;}
    let contact=contact_phase_sample(p.motion,state.phase);
    let all_stance=true;
    for foot in contact.feet {if !foot.in_contact {all_stance=false;}}
    let in_pause=all_stance && pause_remaining(state.phase,state.rate,p)>=0.0;
    let requested=if state.lease.fresh {sensors["command.forward_speed"]/p.nominal_speed_m_s}else{0.0};
    if state.braking || (in_pause && state.rate!=0.0 && requested*state.rate<=0.0) {
        state.braking=true;
        let change=dt*p.deceleration_m_s2/p.nominal_speed_m_s;
        state.rate=state.rate.sign()*(state.rate.abs()-change).max(0.0);
        if state.rate==0.0 {state.braking=false;state.hold_until=t+p.reversal_settle_s;}
    } else if t<state.hold_until {state.rate=0.0;}
    else if requested!=0.0 && requested*state.rate>=0.0 {
        let change=dt*p.acceleration_m_s2/p.nominal_speed_m_s;
        state.rate+=(requested-state.rate).max(-change).min(change);
    } else if in_pause {state.rate=0.0;}
    let yaw=if state.lease.fresh && requested!=0.0 {sensors["command.yaw_rate"]}else{0.0};
    for leg in 0..p.hip_motor_indices.len() {
        if !contact.feet[leg].in_contact {state.yaw_offsets[leg]*=0.65;}
        else {state.yaw_offsets[leg]=(state.yaw_offsets[leg]-p.yaw_jacobian_ratios[leg]*yaw*dt)
            .max(-p.maximum_yaw_offset_rad).min(p.maximum_yaw_offset_rad);}
    }
    let curve_time=(state.phase-p.phase_offset_s+p.period_s)%p.period_s;
    let reference=trajectory_parameter_sample("trajectory",curve_time);
    let static_load=trajectory_parameter_sample("static_feedforward",curve_time);
    let dynamic_load=trajectory_parameter_sample("dynamic_feedforward",curve_time);
    let odd_load=if parameter_exists("velocity_feedforward") {
        trajectory_parameter_sample("velocity_feedforward",curve_time).values
    } else { [] };
    let dynamic_scale=if yaw.abs()<0.000000001 {state.rate*state.rate}else{0.0};
    for name in commands.keys() {
        let j=p.motor_indices[name];let target=reference.values[j];
        for leg in 0..p.hip_motor_indices.len() {
            if j==p.hip_motor_indices[leg] {target+=state.yaw_offsets[leg];}
        }
        let joint=name.sub_string(0,name.len()-7);
        let velocity_load=if odd_load.len()>0 {state.rate*odd_load[j]} else {0.0};
        let feedforward=static_load.values[j]+velocity_load+dynamic_scale*dynamic_load.values[j];
        commands[name]=target+sensors["command.tracking_gain"]*(target-sensors[joint+".angle"])
            +p.velocity_lead_s[j]*state.rate*reference.rates[j]
            +feedforward.max(-p.maximum_feedforward_offset_rad).min(p.maximum_feedforward_offset_rad);
        if p.contains("point_feedback_gain") {
            commands[name]+=p.point_feedback_gain*sensors[joint+".point_correction"];
        }
    }
    #{commands:commands,state:state}
}
