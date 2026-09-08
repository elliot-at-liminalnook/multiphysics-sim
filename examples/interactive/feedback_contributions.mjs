import assert from 'node:assert/strict';
export function correctionInteraction(body,point){
  assert(body.length>0&&body.length===point.length&&[...body,...point].every(Number.isFinite));
  const bodyNorm=Math.hypot(...body),pointNorm=Math.hypot(...point);
  const dot=body.reduce((s,v,i)=>s+v*point[i],0);
  const sumNorm=Math.hypot(...body.map((v,i)=>v+point[i]));
  return {bodyNorm,pointNorm,dot,sumNorm,
    cancelledNorm:Math.max(0,bodyNorm+pointNorm-sumNorm),opposed:dot<0};
}
