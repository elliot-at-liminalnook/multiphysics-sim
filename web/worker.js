// Transport only. Physics, controllers, recording and validation execute in Rust.
import init, { Simulation, EmbeddedSimulation, EnvironmentSimulation } from './sim_web.js';
const ready = init();
let simulation;
let embedded = false, environment = false, chunk = 8, loaded;
let queue = Promise.resolve();
self.onmessage = ({ data }) => {
  queue = queue.then(async () => {
    try {
      await ready;
      let result;
      switch (data.type) {
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
            result = JSON.parse(simulation.step(new Float64Array(data.action)));
            break;
          }
          const count = data.steps ?? chunk;
          if (embedded && (!Number.isInteger(count) || count < 1 || count > 1000)) throw new Error('browser work chunk must be an integer in 1..1000');
          if (embedded && data.action !== undefined) simulation.set_inputs(new Float64Array(data.action));
          result = JSON.parse(embedded ? simulation.advance(count) : simulation.step(new Float64Array(data.action))); break;
        }
        case 'frame': result = JSON.parse(simulation.frame()); break;
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
      self.postMessage({ id: data.id, result });
    } catch (error) {
      self.postMessage({ id: data.id, error: String(error) });
    }
  });
};
