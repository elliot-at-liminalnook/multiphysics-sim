import {test} from 'node:test';import assert from 'node:assert/strict';
import {correctionInteraction} from './feedback_contributions.mjs';
test('angular contribution cancellation distinguishes opposing, aligned and orthogonal vectors',()=>{
  let v=correctionInteraction([1,2],[-1,-2]);assert.equal(v.sumNorm,0);assert.equal(v.cancelledNorm,2*Math.sqrt(5));assert(v.opposed);
  v=correctionInteraction([1,0],[2,0]);assert.equal(v.cancelledNorm,0);assert(!v.opposed);
  v=correctionInteraction([1,0],[0,1]);assert(Math.abs(v.cancelledNorm-(2-Math.sqrt(2)))<1e-15);assert(!v.opposed);
  assert.equal(correctionInteraction([0],[0]).cancelledNorm,0);
  assert.throws(()=>correctionInteraction([NaN],[0]));assert.throws(()=>correctionInteraction([1],[]));
});
