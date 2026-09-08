import {test} from 'node:test';
import assert from 'node:assert/strict';
import {decodeWorkerResult} from '../worker-message.mjs';
test('object and JSON envelopes deliver identical frames and decode once',()=>{
  const frame={time_s:.02,done:false,error:null,contacts:[{force_n:[-1.2e-9,0,25.4]}],observations:[1e20,-.005],policy:{phase:'raise'}};
  const object={id:17,result:structuredClone(frame)},json={id:17,result_json:JSON.stringify(frame),timing:{worker_s:.004}};
  assert.deepEqual(decodeWorkerResult(object),decodeWorkerResult(json));
  const first=json.result;assert.equal(decodeWorkerResult(json),first);
  assert.equal(json.id,17);assert(!Object.hasOwn(json,'result_json'));
  assert(json.timing.receive_json_parse_s>=0);
  assert.equal(json.timing.worker_s,.004);
});
test('malformed or ambiguous JSON replies do not install a result',()=>{
  for(const value of [null,42,'{bad']){
    const reply={id:23,result_json:value};assert.throws(()=>decodeWorkerResult(reply));
    assert(!Object.hasOwn(reply,'result'));assert.equal(reply.id,23);
  }
  assert.throws(()=>decodeWorkerResult({result_json:'{}',result:{}}),/envelope/);
  const error={id:4,error:'bad action'},progress={id:4,progress:{completed_steps:5}};
  assert.equal(decodeWorkerResult(error),undefined);assert.deepEqual(error,{id:4,error:'bad action'});
  assert.equal(decodeWorkerResult(progress),undefined);assert.deepEqual(progress,{id:4,progress:{completed_steps:5}});
});
