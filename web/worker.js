// Transport only. Physics, controllers, recording and validation execute in Rust.
import init, { Simulation, EmbeddedSimulation, EnvironmentSimulation, MotionEvaluation, bind_motion_experiment, inspect_robot_contract, compare_environment_fidelity, materialize_motion } from './sim_web.js';
const ready = init();
let simulation;
let evaluation;
let embedded = false, environment = false, chunk = 8, loaded;
let queue = Promise.resolve();
self.onmessage = ({ data }) => {
  // Opt-in transport diagnostics stay outside Rust frames and recordings.
  const received = data.profile_timing ? performance.now() : undefined;
  queue = queue.then(async () => {
    try {
      await ready;
      const encoding = data.response_encoding ?? 'object';
      if (!['object','json'].includes(encoding) || (encoding === 'json' && data.type !== 'step'))
        throw new Error('response encoding must be object, or json for a step reply');
      const started = received === undefined ? undefined : performance.now();
      let wasmCallMs = 0, parseMs = 0;
      const stepResult = call => {
        if (started === undefined) { const json = call(); return encoding === 'json' ? json : JSON.parse(json); }
        const before = performance.now(), json = call(), parsedAt = performance.now();
        const value = encoding === 'json' ? json : JSON.parse(json);
        wasmCallMs += parsedAt - before; parseMs += performance.now() - parsedAt;
        return value;
      };
      let result;
      switch (data.type) {
        case 'experiment_bind': {
          result = JSON.parse(bind_motion_experiment(JSON.stringify(data.spec)));
          break;
        }
        case 'experiment_start':
        case 'experiment_resume': {
          const next = data.type === 'experiment_start'
            ? new MotionEvaluation(JSON.stringify(data.experiment), JSON.stringify(data.proposal))
            : MotionEvaluation.resume(JSON.stringify(data.experiment), JSON.stringify(data.checkpoint));
          evaluation?.free(); evaluation = next;
          result = JSON.parse(evaluation.frame());
          break;
        }
        case 'experiment_advance': {
          if (!evaluation) throw new Error('no active motion evaluation');
          result = JSON.parse(evaluation.advance(data.maximum_actions ?? 1));
          break;
        }
        case 'experiment_checkpoint':
        case 'experiment_frame':
        case 'experiment_metadata': {
          if (!evaluation) throw new Error('no active motion evaluation');
          const method = data.type.slice('experiment_'.length);
          result = JSON.parse(evaluation[method]());
          break;
        }
        case 'inspect_robot': {
          result = JSON.parse(inspect_robot_contract(JSON.stringify(data.document)));
          break;
        }
        case 'materialize_motion': {
          result = JSON.parse(materialize_motion(JSON.stringify(data.scene), JSON.stringify(data.actions), JSON.stringify(data.recipe), JSON.stringify(data.values)));
          break;
        }
        case 'compare_environment_fidelity': {
          result = JSON.parse(compare_environment_fidelity(JSON.stringify(data.reference), JSON.stringify(data.candidate), JSON.stringify(data.plan)));
          break;
        }
        case 'load': {
          const next = data.task ? new EnvironmentSimulation(JSON.stringify(data.scene), JSON.stringify(data.config), JSON.stringify(data.task), data.seed ?? 0) : data.config ? new EmbeddedSimulation(JSON.stringify(data.scene), JSON.stringify(data.config), data.seed ?? 0) : new Simulation(JSON.stringify(data.scene), data.seed ?? 0);
          simulation?.free();
          simulation = next;
          embedded = Boolean(data.config); environment = Boolean(data.task); loaded = data;
          const metadata = embedded ? JSON.parse(simulation.metadata()) : undefined;
          chunk = Math.max(1, Math.min(40, metadata?.report_every ?? 8));
          result = { frame: JSON.parse(simulation.frame()), inputs: JSON.parse(simulation.inputs()), metadata };
          break;
        }
        case 'step': {
          if (environment) {
            if (data.steps !== undefined && data.steps !== 1) throw new Error('environment step is exactly one action interval');
            result = stepResult(() => simulation.step(new Float64Array(data.action)));
            break;
          }
          const count = data.steps ?? chunk;
          if (embedded && (!Number.isInteger(count) || count < 1 || count > 1000)) throw new Error('browser work chunk must be an integer in 1..1000');
          if (embedded && data.action !== undefined) simulation.set_inputs(new Float64Array(data.action));
          result = stepResult(() => embedded ? simulation.advance(count) : simulation.step(new Float64Array(data.action))); break;
        }
        case 'frame': result = JSON.parse(simulation.frame()); break;
        case 'predict_controller_trajectory': {
          if (!environment) throw new Error('controller trajectory prediction requires a loaded environment');
          result = JSON.parse(simulation.predict_controller_trajectory(JSON.stringify(data.model), JSON.stringify(data.previous), JSON.stringify(data.actions)));
          break;
        }
        case 'set_attempt_audit_limit': {
          if (!Number.isInteger(data.limit) || data.limit < 0 || data.limit > 10000) throw new Error('attempt limit must be 0..10000');
          simulation.set_attempt_audit_limit(data.limit); result = null; break;
        }
        case 'contact_impulse_report': result = JSON.parse(simulation.contact_impulse_report(data.start, data.end)); break;
        case 'implicit_attempt_report': result = JSON.parse(simulation.implicit_attempt_report()); break;
        case 'reset': {
          if (environment) result = JSON.parse(simulation.reset(data.seed ?? 0));
          else if (embedded) {
            const next = new EmbeddedSimulation(JSON.stringify(loaded.scene), JSON.stringify(loaded.config), data.seed ?? 0);
            simulation.free(); simulation = next; result = JSON.parse(simulation.frame());
          } else result = JSON.parse(simulation.reset(data.seed ?? 0));
          break;
        }
        case 'recording': result = JSON.parse(simulation.recording()); break;
        case 'replay': {
          if (environment) {
            const total=simulation.prepare_replay(JSON.stringify(data.recording));
            result=JSON.parse(simulation.frame());
            for (let i=0;i<total;i++) {
              result=JSON.parse(simulation.advance_replay());
              self.postMessage({id:data.id,progress:{completed_steps:i+1,total_steps:total}});
              await new Promise(resolve=>setTimeout(resolve,0));
            }
          } else if (embedded) {
            const steps = simulation.prepare_replay(JSON.stringify(data.recording));
            result = JSON.parse(simulation.frame());
            for (let completed = 0; completed < steps;) {
              const count = Math.min(chunk, steps - completed);
              result = JSON.parse(simulation.advance(count));
              if (result.error) break;
              completed += count;
              self.postMessage({id:data.id, progress:{completed_steps:completed,total_steps:steps}});
              await new Promise(resolve => setTimeout(resolve, 0));
            }
          } else result = JSON.parse(simulation.replay(JSON.stringify(data.recording)));
          break;
        }
        default: throw new Error(`Unknown simulation request: ${data.type}`);
      }
      const timing = started === undefined ? undefined : {
        queue_s: (started - received) / 1000,
        worker_s: (performance.now() - started) / 1000,
        wasm_call_s: wasmCallMs / 1000,
        json_parse_s: parseMs / 1000,
      };
      self.postMessage({ id: data.id, ...(encoding === 'json' ? {result_json:result} : {result}), ...(timing ? { timing } : {}) });
    } catch (error) {
      self.postMessage({ id: data.id, error: String(error) });
    }
  });
};
