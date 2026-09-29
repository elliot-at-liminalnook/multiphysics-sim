// Hover previews a step; click/keyboard selection pins it. Follow tracks transitions.
let workflowPinned=null, workflowHover=null, workflowSelected=null, workflowView='prompt';
const workflowNames={director:'Choose a batch',assign:'Write the assignment',worker:'Implement',verify:'Run checks',review:'Review the result'};
function flowDuration(n){n=Math.max(0,n);return n>=3600?Math.floor(n/3600)+'h '+Math.floor(n%3600/60)+'m':n>=60?Math.floor(n/60)+'m '+Math.floor(n%60)+'s':Math.floor(n)+'s'}
function flowText(id,text){const el=document.getElementById(id);if(el.textContent!==text)el.textContent=text}
function flowHTML(id,html){const el=document.getElementById(id);if(el.dataset.rendered!==html){const scroll=el.scrollTop;el.innerHTML=html;el.dataset.rendered=html;el.scrollTop=scroll}}
function renderWorkflow(){
 if(!data?.workflow)return;
 const w=data.workflow,s=data.state,live=data.active,current=workflowNames[w.current]||pretty(w.current);
 const paused=!live&&s.status!=='complete';
 flowText('flow-current',live?`Now: ${current}`:s.status==='complete'?'Work completed':`${s.status==='blocked'?'Needs attention':s.status==='ready'?'Ready':'Paused'} at: ${current}`);
 flowText('flow-state',live?(data.stop_requested?'Stopping':'Live · refreshes every 2s'):'Saved state · not running');
 $('flow-state').className='pill '+(live?'running':'');
 $('flow-alert').hidden=live||!w.message;
 flowText('flow-alert',w.message||'');
 for(const n of w.nodes){
  const b=$('flow-'+n.id);b.classList.toggle('current',n.current);b.classList.toggle('running',n.running);b.classList.toggle('paused',n.current&&paused);
  b.setAttribute('aria-current',n.current?'step':'false');flowText('flow-status-'+n.id,n.status);
 }
 const selected=workflowHover||workflowPinned||w.current;
 const node=w.nodes.find(n=>n.id===selected)||w.nodes[0];workflowSelected=node;
 for(const n of w.nodes){const b=$('flow-'+n.id);b.classList.toggle('inspected',n.id===node.id);b.setAttribute('aria-pressed',String(workflowPinned===n.id))}
 flowText('flow-title',node.owner+' · '+node.title);
 flowText('flow-purpose',node.purpose);
 flowText('flow-mode',workflowHover?'Hover preview':workflowPinned?'Pinned step':'Following current step');
 $('flow-follow').hidden=!workflowPinned&&!workflowHover;
 const call=node.call, now=data.now;
 const elapsed=call?flowDuration(call.elapsed_seconds):'—';
 flowText('flow-time',elapsed);flowText('flow-time-label',call?(call.live?'This turn · elapsed':call.timing_complete?'Last turn · duration':'Through last log update'):'No turn yet');
 flowText('flow-call',call?'#'+call.number:'—');
 flowText('flow-activity-age',call?flowDuration(Math.max(0,now-call.last_activity_at))+' ago':'No output yet');
 flowText('flow-model',call?.model|| (call?'Claude Code':node.id==='verify'?'Local checks':'Not started'));
 const isCheck=node.id==='verify';
 flowText('flow-prompt-label',isCheck?'Verification plan':call?'Exact prompt · '+(call.live?'current turn':'last recorded turn'):'What this step will do');
 const checks= [...new Set(['diff',...(s.plan?.checks||[])])];
 const prompt=isCheck?'Required checks: '+checks.join(', ')+'.\n\nThe coordinator runs these independently. The orchestrator then reviews the results.':call?.prompt||node.purpose+'\n\nNo prompt has been issued for this step yet.';
 flowText('flow-prompt',prompt);
 $('flow-full').disabled=!call&&!isCheck;
 flowText('flow-feed-label',isCheck?(node.live_output?.length?'Current check output · no final verdict yet':'Last completed check receipts'):call?.live?'Live activity · newest first':'Saved activity · newest first');
 let feed='';
 if(isCheck&&node.live_output?.length){feed=[...node.live_output].reverse().map(c=>`<div class="flow-event tool"><strong>${escapeHTML(c.name)} · captured output</strong><br>${escapeHTML(c.command.join(' '))}<br>${escapeHTML(c.text.slice(-3000)||'Command started; no output yet.')}</div>`).join('')}
 else if(isCheck){feed=(node.checks||[]).map(c=>`<div class="flow-event"><strong>${c.exit_code?'Failed':'Passed'} · ${escapeHTML(c.name)}</strong><br>${escapeHTML(c.command?.join(' ')||'')}<br>${escapeHTML((c.stderr_text||c.stdout_text||'No output').slice(-1800))}</div>`).join('')||'<div class="empty">No completed check receipts yet. An unfinished check is not counted as passed.</div>'}
 else {feed=call?.activity?.length?[...call.activity].reverse().slice(0,12).map(e=>`<div class="flow-event ${escapeHTML(e.kind)}">${escapeHTML(e.text)}</div>`).join(''):'<div class="empty">'+(call?.live?'Waiting for the next activity event. The agent may be thinking or waiting on a command.':'No recorded activity for this step yet.')+'</div>'}
 flowHTML('flow-feed',feed);
 const items=s.plan?.checklist||[];
 flowText('flow-progress',`${items.filter(i=>i.status==='verified').length}/${items.length} acceptance items verified`);
 flowText('flow-totals',`${w.completed_batches} batches accepted · ${s.rounds} worker turns · $${data.estimated_spent.toFixed(2)} reported estimate`);
 flowText('flow-caption',call?.live?'Activity comes from public messages and tool events; silence does not prove a hang.':node.current&&paused?'This step is paused. Its saved prompt and activity remain available.':'Showing the most recent recorded turn for this step.');
}
function initWorkflow(){
 const track=$('flow-track');
 track.addEventListener('pointerover',e=>{const step=e.target.closest('[data-step]');if(step){workflowHover=step.dataset.step;renderWorkflow()}});
 track.addEventListener('pointerleave',()=>{workflowHover=null;renderWorkflow()});
 track.addEventListener('focusin',e=>{const step=e.target.closest('[data-step]');if(step){workflowHover=step.dataset.step;renderWorkflow()}});
 track.addEventListener('focusout',()=>{workflowHover=null;renderWorkflow()});
 track.addEventListener('click',e=>{const step=e.target.closest('[data-step]');if(step){workflowPinned=step.dataset.step;workflowHover=null;renderWorkflow()}});
 $('flow-follow').onclick=()=>{workflowPinned=null;workflowHover=null;renderWorkflow()};
 $('flow-full').onclick=()=>{if(workflowSelected)modal(workflowSelected.owner+' · '+workflowSelected.title,$('flow-prompt').textContent)};
}
