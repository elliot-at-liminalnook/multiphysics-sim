// Explicit hypothetical robustness cases; author through CAD before simulation.
import fs from 'node:fs';
import crypto from 'node:crypto';
const base='examples/full-robot/measured-actuator-integration/gait-generation';
const source=`${base}/full-authority/pilot.spec.json`;
const sha=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const spec=JSON.parse(fs.readFileSync(source));
const name=process.argv[2];
if (!['battery-chain','motor-spread'].includes(name)) throw Error('Choose battery-chain or motor-spread');
const dir=`${base}/robustness/${name}`;
if (fs.existsSync(dir)) throw Error(`Preserve existing ${dir}`);
fs.mkdirSync(dir,{recursive:true});
const manifest={
  name,calibrated:false,parent:{path:source,sha256:sha(source)},
  generator_sha256:sha(new URL(import.meta.url)),
  scope:'Hypothetical sensitivity case, not a measured parameter set, physical rating, confidence bound or identified motor assignment.',
  assumptions:name==='battery-chain' ? {
    battery:{cells:3,nominal_voltage_v:11.1,capacity_ah:2,initial_soc:0.8,internal_resistance_ohm:0.1},
    wiring:'Common feed, then four separate three-motor chains in Hip/Worm/Foot order. Each edge includes supply and return resistance.',
    common_feed_resistance_ohm:0.05,each_chain_segment_resistance_ohm:0.05,
    model_envelope:{minimum_pack_voltage_v:9,maximum_pack_voltage_v:12.6,minimum_soc:0.1,maximum_soc:1},
    limitations:['Battery choice has not been supplied; 3-cell 2 Ah is an explicit trial assumption.',
      'No calibrated current measurements identify internal or wiring resistance.',
      'Chain order/topology is a proposed scenario, not a verified quadruped wiring harness.',
      'Registered battery discharge curve is generic, not measured for a selected pack.',
      'Voltage/SOC envelope stops invalid experiments; it is not a simulated physical BMS.']
  } : {
    factors:[0.8,1,1.2],parameters:['rotor_inertia','gear_friction'],
    assignment:'Sort stable CAD motor IDs and cycle factors; deterministic synthetic heterogeneity, not physical-unit attribution.',
    limitations:['20 percent spread is an unmeasured probe, not an observed population distribution.',
      'Electrical torque/back-EMF constants remain coupled and unchanged.',
      'Other parameters and correlations need separate scenarios.']
  }
};
const manifestPath=`${dir}/scenario.json`;
fs.writeFileSync(manifestPath,JSON.stringify(manifest,null,2)+'\n');
const evidence={path:manifestPath,sha256:sha(manifestPath),scope:manifest.scope};
const parameter=(value,unit)=>({value,unit,provenance:'estimated',uncertainty:null,evidence:'robustness_probe'});
const profiles=structuredClone(spec.scene.robot.actuator_profiles);
if (name==='battery-chain') {
  const branches=[{id:'common-feed',parent:null,resistance:parameter(0.05,'Ω'),motors:[]}];
  const assignments=[];
  for (const leg of ['-Y','+X','+Y','-X']) {
    let parent='common-feed';
    for (const role of ['Hip','Worm','Foot']) {
      const matches=spec.scene.robot.motors.filter(m=>m.joint===`${leg} | ${role} servo output`);
      if(matches.length!==1)throw Error(`Missing unique motor ${leg}/${role}`);
      const id=`${leg}-${role.toLowerCase()}`;
      branches.push({id,parent,resistance:parameter(0.05,'Ω'),motors:[matches[0].id]});
      assignments.push(matches[0].id);parent=id;
    }
  }
  if(assignments.length!==Object.keys(profiles.bindings).length || new Set(assignments).size!==assignments.length)
    throw Error('Incomplete power assignment');
  profiles.power={version:1,description:'Hypothetical shared 3-cell battery and four three-motor power chains',
    limitations:manifest.assumptions.limitations,evidence:{robustness_probe:evidence},
    battery:Object.fromEntries([
      ['cells',3,'1'],['nominal_voltage',11.1,'V'],['capacity_ah',2,'A·h'],
      ['initial_soc',0.8,'1'],['internal_resistance',0.1,'Ω']
    ].map(([k,v,u])=>[k,parameter(v,u)])),branches,
    operating_limits:Object.fromEntries([
      ['minimum_pack_voltage',9,'V'],['maximum_pack_voltage',12.6,'V'],
      ['minimum_soc',0.1,'1'],['maximum_soc',1,'1']
    ].map(([k,v,u])=>[k,parameter(v,u)]))};
  fs.writeFileSync(`${dir}/power-selection.json`,JSON.stringify({residual_scales:[1,1,1,1]})+'\n');
} else {
  const families={};
  Object.keys(profiles.bindings).sort().forEach((id,i)=>{
    const binding=profiles.bindings[id], old=profiles.families[binding.family];
    if(binding.physical_unit!==null || Object.keys(binding.deviations).length)throw Error('Preserve physical-unit profiles');
    const factor=manifest.assumptions.factors[i%3], key=`${binding.family}-synthetic-${Math.round(factor*100)}`;
    if(!families[key]){
      const f=structuredClone(old);f.version=1;
      f.description=`Synthetic ${factor}x inertia/friction variant; not an identified physical unit`;
      f.evidence.robustness_probe=evidence;
      f.limitations.push(...manifest.assumptions.limitations);
      for(const k of manifest.assumptions.parameters)f.motor[k]=parameter(old.motor[k].value*factor,old.motor[k].unit);
      families[key]=f;
    }
    binding.family=key;binding.version=1;
  });
  profiles.families=families;
}
fs.writeFileSync(`${dir}/profiles.json`,JSON.stringify(profiles,null,2)+'\n');
console.log(JSON.stringify({name,profile_count:Object.keys(profiles.families).length,motor_count:Object.keys(profiles.bindings).length}));
