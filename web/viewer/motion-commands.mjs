// UI command mapping only. Controllers and motion clocks remain in Rust/Rhai.
export function boundedInputValue(channel, value) {
  if(!Number.isFinite(value)||!Number.isFinite(channel.lower)||!Number.isFinite(channel.upper)||channel.lower>channel.upper)
    throw new Error('Input values and ordered limits must be finite.');
  return Math.max(channel.lower,Math.min(channel.upper,value));
}

export function motionCommandConfig(current, channels) {
  const existing=current?.data?.policy_contract?.step_reference?.config;
  if(existing)return existing;
  const names=current?.motion_commands;
  if(names===undefined)return null;
  const kinds=['LinearVelocity','LinearVelocity','AngularVelocity'];
  if(!Array.isArray(names)||names.length!==3||new Set(names).size!==3||names.some((name,i)=>{
    const matches=channels.filter(c=>c.name===name);
    return matches.length!==1||matches[0].kind!==kinds[i]||matches[0].lower>0||matches[0].upper<0;
  }))throw new Error('Motion controls require three unique, typed velocity inputs containing zero.');
  return {command_channels:names,sequence:{update_command_before_lift:false}};
}

export function motionHeartbeatIndex(current, channels) {
  const name=current?.motion_heartbeat;
  if(name===undefined)return -1;
  const matches=channels.flatMap((c,i)=>c.name===name?[i]:[]),i=matches[0],c=channels[i];
  if(matches.length!==1||c.kind!=='Dimensionless'||c.lower!==0||!Number.isSafeInteger(c.upper)||c.upper<1||!Number.isSafeInteger(c.initial)||c.initial<0||c.initial>=c.upper||current.motion_commands?.includes(name))throw Error('Motion heartbeat requires a unique bounded integer sequence channel.');
  return i;
}

// A transport packet is fresh even when the held keys have not changed. The
// shared Rust controller detects loss by observing an unchanged sequence.
export function nextMotionAction(current, channels, values) {
  const action=[...values],i=motionHeartbeatIndex(current,channels);
  if(i>=0){if(!Number.isSafeInteger(action[i])||action[i]<0||action[i]>=channels[i].upper)throw Error('Motion packet sequence exhausted or invalid; reset the session.');action[i]++;}
  return action;
}

// Preset-specific velocity requests preserve tuned forward/yaw combinations
// without changing the controller's authored input bounds or doing physics here.
export function driveMotionValues(current, channels, keys) {
  const drive=motionCommandConfig(current,channels);
  if(!drive)return null;
  const cs=drive.command_channels.map(name=>channels.find(c=>c.name===name));
  const vectors=current.motion_key_vectors;
  if(vectors===undefined){
    const directions=[Number(keys.has('w'))-Number(keys.has('s')),0,Number(keys.has('a'))-Number(keys.has('d'))];
    return cs.map((c,i)=>directions[i]>0?c.upper:directions[i]<0?c.lower:0);
  }
  if(Object.keys(vectors).sort().join('')!=='adsw'||Object.values(vectors).some(v=>
    !Array.isArray(v)||v.length!==3||v.some((x,i)=>!Number.isFinite(x)||x<cs[i].lower||x>cs[i].upper)))
    throw Error('WASD vectors require four finite, typed, in-range motion requests.');
  return cs.map((c,i)=>boundedInputValue(c,['w','a','s','d'].reduce((v,key)=>v+(keys.has(key)?vectors[key][i]:0),0)));
}
