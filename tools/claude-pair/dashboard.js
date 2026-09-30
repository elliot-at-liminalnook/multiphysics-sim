// Claude Pair dashboard. Polls /api/state every 2 s and renders it; controls POST with the page token.
'use strict';
const TOKEN = window.PAIR_TOKEN;
const $ = id => document.getElementById(id);
const esc = s => String(s ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
const ROLES = {
  director: { name: 'Director', letter: 'D', color: 'var(--director)' },
  orchestrator: { name: 'Orchestrator', letter: 'O', color: 'var(--orchestrator)' },
  worker: { name: 'Worker', letter: 'W', color: 'var(--worker)' },
  checks: { name: 'Checks', letter: '✓', color: 'var(--coordinator)' },
  coordinator: { name: 'Coordinator', letter: 'C', color: 'var(--coordinator)' },
  user: { name: 'You', letter: 'U', color: 'var(--user)' },
};
const STAGES = [
  { id: 'director', label: 'Choose batch', owner: 'Director', role: 'director' },
  { id: 'assign', label: 'Assign', owner: 'Orchestrator', role: 'orchestrator' },
  { id: 'worker', label: 'Implement', owner: 'Worker', role: 'worker' },
  { id: 'verify', label: 'Reruns', owner: 'If requested', role: 'coordinator' },
  { id: 'review', label: 'Review', owner: 'Orchestrator', role: 'orchestrator' },
];
const STAGE_DONE = { assign: 'Wrote the assignment', review: 'Reviewed the result', worker: 'Implemented', director: 'Chose the next batch' };
const LIMIT_KEYS = ['max_rounds', 'max_hours', 'budget_usd', 'call_budget_usd', 'turn_minutes', 'max_turns'];

let data = null, view = 'overview', pinnedRole = null, liveTab = 'activity', busy = false;
const dirty = { guidance: false, limits: false, outer: false };
const rendered = {};

/* ---------- formatting ---------- */
function dur(sec) {
  sec = Math.max(0, Math.floor(sec || 0));
  if (sec >= 3600) return `${Math.floor(sec / 3600)}h ${String(Math.floor(sec % 3600 / 60)).padStart(2, '0')}m`;
  if (sec >= 60) return `${Math.floor(sec / 60)}m ${String(sec % 60).padStart(2, '0')}s`;
  return `${sec}s`;
}
function ago(ts) { return data && ts ? dur(data.now - ts) + ' ago' : '—'; }
const money = n => '$' + (n || 0).toFixed(2);
function clock(ts) {
  const d = new Date(ts * 1000), today = new Date();
  const time = d.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
  return d.toDateString() === today.toDateString() ? time : d.toLocaleDateString([], { weekday: 'short' }) + ' ' + time;
}
const pct = (a, b) => b ? Math.max(0, Math.min(100, a / b * 100)) : 0;
const plural = (n, w) => `${n} ${w}${n === 1 ? '' : 's'}`;

/* ---------- DOM helpers ---------- */
function html(id, markup, keepScroll = true) {
  if (rendered[id] === markup) return false;
  const el = $(id), top = el.scrollTop;
  el.innerHTML = markup;
  rendered[id] = markup;
  if (keepScroll) el.scrollTop = top;
  return true;
}
function text(id, value) { const el = $(id); if (el.textContent !== value) el.textContent = value; }
function toast(message) { $('toast').textContent = message; $('toast').hidden = false; clearTimeout(toast.t); toast.t = setTimeout(() => $('toast').hidden = true, 5000); }
let lastFocus = null;
function modal(title, body) {
  lastFocus = document.activeElement;
  $('modal-title').textContent = title;
  $('modal-body').innerHTML = body;
  $('modal').hidden = false;
  $('modal-close').focus();
}
const pre = t => `<pre class="codeblock">${esc(t)}</pre>`;
function closeModal() { $('modal').hidden = true; lastFocus?.focus(); }

/* ---------- derived state ---------- */
const latest = role => data.calls.filter(c => c.role === role).at(-1);
function followRole() {
  const s = data.state;
  if (data.active && s.inflight?.role) return s.inflight.role;
  if (['verify', 'precheck'].includes(s.phase) && data.active) return 'checks';
  return data.calls.at(-1)?.role || 'orchestrator';
}
const liveRole = () => pinnedRole || followRole();

function status() {
  const s = data.state;
  if (data.active && data.stop_requested) return { label: 'Stopping', cls: 'warn' };
  if (s.status === 'waiting') return { label: s.wait_kind === 'retry' ? 'Retrying' : 'Waiting for usage limit', cls: 'warn' };
  if (data.active) return { label: 'Running', cls: 'running' };
  return ({ ready: { label: 'Ready', cls: '' }, paused: { label: 'Paused', cls: 'paused' }, blocked: { label: 'Needs attention', cls: 'blocked' },
    complete: { label: 'Complete', cls: 'complete' }, running: { label: 'Stopped', cls: 'paused' } })[s.status] || { label: s.status, cls: '' };
}

function now() {
  const s = data.state, live = data.active, phase = s.phase, batch = s.outer?.current_batch;
  const doing = {
    director: ['director', 'Director is choosing the next batch', 'the Director chooses the next batch'],
    precheck: ['checks', 'Running checks before the worker starts', 'checks run before the worker starts'],
    worker: s.verification ? ['worker', 'Worker is running a verification pass', 'the worker runs a verification pass (build, bug hunt, fixes)']
                           : ['worker', 'Worker is implementing', 'the worker implements the assignment'],
    verify: ['checks', 'Running independent checks', 'independent checks run'],
  };
  let [role, active, next] = doing[phase] || (s.report
    ? ['orchestrator', 'Orchestrator is reviewing the result', 'the orchestrator reviews the result']
    : ['orchestrator', 'Orchestrator is writing the next assignment', 'the orchestrator writes the next assignment']);
  let eyebrow = live ? 'Now' : 'Next', headline = live ? active : 'Paused · ' + next[0].toUpperCase() + next.slice(1);
  let subline = s.plan?.summary || (batch ? batch.title + ' — ' + batch.objective : '');
  if (s.status === 'waiting' && s.resume_at && s.wait_kind === 'retry') {
    eyebrow = 'Retrying'; headline = 'Retrying after a failed call';
    subline = `Continues the same session at ${clock(s.resume_at)} (in ${dur(s.resume_at - data.now)}).`;
  } else if (s.status === 'waiting' && s.resume_at) {
    eyebrow = 'Usage limit'; headline = `Waiting for Claude usage to reset`;
    subline = `Resumes automatically at ${clock(s.resume_at)} (in ${dur(s.resume_at - data.now)}), continuing the interrupted ${ROLES[role]?.name.toLowerCase() || 'agent'} session where it left off.`;
  } else if (!data.calls.length && !live) {
    eyebrow = 'Ready'; headline = 'Ready to start';
    subline = 'Press Start. The orchestrator reads the mission and writes the first assignment, the worker implements it in this folder, checks run, and the orchestrator reviews.';
  } else if (s.status === 'complete') {
    eyebrow = 'Done'; headline = 'Mission complete';
  } else if (s.status === 'blocked') {
    headline = 'Needs attention';
  }
  return { role, eyebrow, headline, subline };
}

/* ---------- top-level render ---------- */
function render() {
  const s = data.state, st = status();
  const name = data.workspace.split('/').filter(Boolean).at(-1) || data.workspace;
  text('project', name); $('project').title = data.workspace;
  $('branch').hidden = !data.branch; text('branch', data.branch || '');
  text('commits-branch', data.branch || 'this branch');
  $('conn').classList.remove('lost'); text('conn', 'Live');
  $('status').className = 'chip ' + st.cls; text('status-text', st.label);
  $('start').disabled = data.active || busy || (s.status === 'complete' && !data.outer_settings.enabled);
  $('stop').disabled = !data.active || data.stop_requested || busy;
  text('start', data.calls.length ? 'Continue' : 'Start');
  if (!data.active) document.title = `${st.label} · Claude Pair`;
  renderBanner(); renderNow(); renderTabs();
  if (view === 'overview') renderOverview();
  if (view === 'timeline') renderTimeline();
  if (view === 'notebook') renderNotebook();
  if (view === 'decisions') renderDecisions();
  if (view === 'director') renderDirector();
  if (view === 'settings') renderSettings();
}

function renderBanner() {
  const s = data.state, b = $('banner'), action = $('banner-action');
  let kind = '', message = '', act = '';
  bannerAction = 'resume';
  if (s.status === 'waiting' && s.resume_at && s.wait_kind === 'retry') {
    kind = 'warn'; act = 'Retry now'; bannerAction = 'retry-now';
    message = `<strong>Retrying after a failed call</strong>${esc(s.message)} (in ${esc(dur(s.resume_at - data.now))}).`;
  } else if (s.status === 'waiting' && s.resume_at) {
    kind = 'warn'; act = 'Try now'; bannerAction = 'retry-now';
    message = `<strong>Claude usage limit reached</strong>Resuming automatically at ${esc(clock(s.resume_at))} · in ${esc(dur(s.resume_at - data.now))}. Nothing is lost; the interrupted session continues where it stopped.`;
  } else if (!data.active && s.status === 'paused' && s.weekly_reset_at) {
    kind = 'warn';
    message = `<strong>Weekly Claude limit used up</strong>It resets ${esc(clock(s.weekly_reset_at))}${s.weekly_reset_at > data.now ? ' · in ' + esc(dur(s.weekly_reset_at - data.now)) : ''}. Press Continue after that; the interrupted session picks up where it stopped.`;
  } else if (!data.active && s.status === 'blocked') {
    kind = 'bad'; message = `<strong>The run stopped with an error</strong>${esc(s.message)}`;
    if (s.inflight) act = 'Resume interrupted turn';
  } else if (!data.active && s.status === 'paused' && s.message) {
    kind = 'warn'; message = `<strong>Paused</strong>${esc(s.message)}`;
    if (s.inflight) act = 'Resume interrupted turn';
  }
  b.hidden = !kind; b.className = 'banner ' + kind;
  html('banner-text', message);
  action.hidden = !act; text('banner-action', act);
}

function renderNow() {
  const s = data.state, info = now(), live = data.active;
  $('now-dot').style.background = ROLES[info.role]?.color || 'var(--accent)';
  $('now-dot').className = 'role-dot' + (live ? ' pulse' : '');
  text('now-eyebrow', info.eyebrow); text('headline', info.headline); text('subline', info.subline);
  const call = data.active && s.inflight ? data.calls.find(c => c.live) : null;
  const meta = [];
  if (call) meta.push(`<span>${esc(ROLES[call.role].name)} · turn <b>#${call.number}</b></span>`, `<span>running <b>${dur(call.elapsed_seconds)}</b></span>`, `<span>last output <b>${esc(ago(call.last_activity_at))}</b></span>`);
  if (call?.subagents?.length) meta.push(`<span><b>${esc(subagentSummary(call))}</b></span>`);
  if (s.outer?.current_batch) meta.push(`<span>epic <b>${esc(s.outer.current_batch.id)}</b></span>`);
  if (s.assignment) meta.push(`<span>assignment <b>#${s.assignment}</b></span>`);
  html('now-meta', meta.join(''));
  renderRecent();

  const current = data.workflow.current, order = STAGES.map(x => x.id), at = order.indexOf(current);
  const directorOff = !data.outer_settings.enabled && !s.outer;
  html('pipeline', STAGES.map((st, i) => {
    const off = st.id === 'director' && directorOff;
    const cls = ['stage', i < at && !off ? 'done' : '', st.id === current ? 'current' : '', st.id === current && live ? 'live' : '', st.id === 'director' && directorOff ? 'off' : ''].join(' ');
    const mark = i < at && !off ? '✓' : String(i + 1);
    return `<div class="${cls}" style="--stage-color:${ROLES[st.role].color}" title="${esc(st.owner)}"><span class="node">${mark}</span><span class="stage-label">${esc(st.label)}</span><span class="stage-owner">${esc(st.id === 'director' && directorOff ? 'off' : st.owner)}</span></div>`;
  }).join(''), false);

  const items = s.plan?.checklist || [], verified = items.filter(i => i.status === 'verified').length;
  const L = data.limits, spent = data.estimated_spent, g = data.git || {};
  const rl = data.rate_limits, five = rl?.unifiedWindows?.five_hour, week = rl?.unifiedWindows?.seven_day;
  const meter = (label, value, small, width, cls, hint = '') =>
    `<div title="${esc(hint)}"><div class="meter-label"><span>${label}</span></div><div class="meter-value">${value}${small ? ` <small>${small}</small>` : ''}</div><div class="bar ${cls}"><span style="width:${width}%"></span></div></div>`;
  const tone = p => p >= 90 ? 'bad' : p >= 70 ? 'warn' : '';
  const fiveP = five ? five.utilization * 100 : null, weekP = week ? week.utilization * 100 : null;
  const capped = (value, cap) => cap ? [pct(value, cap), tone(pct(value, cap))] : [0, 'none'];
  const [turnW, turnT] = capped(s.rounds, L.max_rounds), [costW, costT] = capped(spent, L.budget_usd);
  html('meters', [
    meter('Checklist', items.length ? `${verified}` : '—', items.length ? `of ${items.length} verified` : 'no checklist yet', pct(verified, items.length), 'ok'),
    meter('Worker turns', `${s.rounds}`, L.max_rounds ? `of ${L.max_rounds}` : `${dur(data.elapsed_seconds)} active`, turnW, turnT,
      L.max_hours ? `Active time limit: ${L.max_hours}h` : 'No turn or time limit'),
    meter('Usage estimate', money(spent), L.budget_usd ? `of ${money(L.budget_usd)}` : 'no ceiling', costW, costT, 'API-price estimate of the subscription usage, not a bill'),
    meter('Claude 5-hour', fiveP === null ? '—' : `${Math.round(fiveP)}%`, five ? `resets ${clock(five.resetsAt)}` : 'no reading yet', fiveP ?? 0, tone(fiveP ?? 0),
      'The run pauses when this is used up and resumes after the reset'),
    meter('Claude weekly', weekP === null ? '—' : `${Math.round(weekP)}%`, week ? `resets ${clock(week.resetsAt)}` : 'no reading yet', weekP ?? 0, tone(weekP ?? 0),
      'The run stops when this is used up: it is the only limit'),
    meter('Changes', `${(g.commits || []).length}`, `${plural((g.commits || []).length, 'commit').replace(/^\d+ /, '')} · <span class="plusminus"><span class="plus">+${g.insertions || 0}</span> <span class="minus">−${g.deletions || 0}</span></span>`,
      data.verify_every ? pct(data.unverified_commits, data.verify_every) : 0, data.verify_every ? '' : 'none',
      data.verify_every ? `${data.unverified_commits} of ${data.verify_every} commits until the next verification pass` : ''),
  ].join(''), false);
}

function renderTabs() {
  text('badge-timeline', String(data.calls.length));
  text('badge-notebook', String(data.notebook.total));
  text('badge-decisions', String((data.state.decisions || []).length));
}

/* The active (or latest) agent's last few actions, on every tab. */
function renderRecent() {
  const call = data.calls.find(c => c.live) || data.calls.at(-1);
  if (call?.subagents?.length) return renderTeam(call);
  const acts = (call?.activity || []).filter(e => e.kind !== 'session').slice(-4).reverse();
  $('recent').hidden = !acts.length;
  if (!acts.length) return;
  const running = acts.find(e => e.status === 'running');
  if (call?.live) document.title = `${summarize(acts[0]).slice(0, 60)} · ${ROLES[call.role]?.name || ''}`;
  html('recent', `<div class="recent-head">${esc(ROLES[call.role]?.name || '')} · latest actions${running ? ` · <b>running ${esc(dur(data.now - running.at))}</b>` : ''}</div>` +
    acts.map((e, i) => `<button class="recent-row ${e.status || e.kind}" data-recent="${i}"><span class="ev-icon">${esc(e.kind === 'message' ? ROLES[call.role]?.letter : (TOOL_ICON[e.name] || '•'))}</span><span class="recent-text">${esc(summarize(e))}</span><span class="ev-meta">${statusHTML(e)}${e.at ? `<time class="ev-time">${esc(ago(e.at))}</time>` : ''}</span></button>`).join(''), false);
  renderRecent.acts = acts; renderRecent.call = call;
}

/* With subagents, the header shows one row per agent and what it's doing now. */
function renderTeam(call) {
  const all = lanes(call), shown = all.slice(0, 9);
  $('recent').hidden = false;
  const latest = l => l.items.at(-1);
  html('recent', `<div class="recent-head">${esc(subagentSummary(call))} · what each agent is doing</div>` +
    shown.map((l, i) => {
      const e = latest(l);
      return `<button class="recent-row lane" data-lane="${i}"><span class="ev-icon">${l.id ? '⇉' : esc(ROLES[call.role]?.letter || '·')}</span>` +
        `<span class="recent-text"><b class="lane-name">${esc(l.label)}</b> <span class="faint">${e ? esc(summarize(e)) : 'starting…'}</span></span>` +
        `<span class="ev-meta"><span>${l.total ?? l.items.length} actions</span>${l.id ? laneStatus(l) : (e ? statusHTML(e) : '')}${e?.at ? `<time class="ev-time">${esc(ago(e.at))}</time>` : ''}</span></button>`;
    }).join('') + (all.length > shown.length ? `<div class="recent-head">…and ${all.length - shown.length} more in the Agents tab</div>` : ''), false);
  renderRecent.lanes = all; renderRecent.call = call;
  if (call.live) document.title = `${subagentSummary(call)} · ${ROLES[call.role]?.name || ''}`;
}
function openLane(call, lane) {
  const brief = lane.prompt ? `<details><summary class="faint" style="cursor:pointer;font-size:12px">The brief it was given</summary>${pre(lane.prompt)}</details>` : '';
  const report = lane.output ? `<div class="section-label">Its report</div>${pre(lane.output)}` : '';
  modal(lane.label + (lane.type ? ` · ${lane.type}` : ''), `<div class="now-meta" style="margin:0 0 10px">${lane.id ? laneStatus(lane) : ''}<span>${lane.total ?? lane.items.length} actions</span></div>` +
    brief + report + `<div class="section-label">Actions, oldest first</div>` + (lane.items.map((e, i) => eventHTML(e, call.role, `lane-${i}`)).join('') || '<div class="empty">No actions yet.</div>'));
}

/* ---------- overview ---------- */
function renderOverview() {
  renderLive(); renderAssignment(); renderChecklist(); renderChecks(); renderShots(); renderCommits(); renderGuide();
}

const TOOL_ICON = { Bash: '$', Read: '◧', Edit: '✎', Write: '✎', MultiEdit: '✎', NotebookEdit: '✎', Grep: '⌕', Glob: '⌕', Task: '⇉', Agent: '⇉', WebFetch: '↗', WebSearch: '⌕', TodoWrite: '☐', Skill: '★' };
const base = p => String(p || '').split('/').filter(Boolean).at(-1) || p || '';
/* One lane per agent in a turn: the lead first, then each subagent in spawn order. */
function lanes(call) {
  if (!call) return [];
  const acts = call.activity || [];
  const lead = { id: null, label: `${ROLES[call.role]?.name || 'Agent'} (lead)`, role: call.role, status: call.live ? 'running' : 'done',
                 items: acts.filter(a => !a.parent && a.kind !== 'session'), total: null };
  return [lead, ...(call.subagents || []).map((s, i) => ({ id: s.id, index: i, label: s.description || s.type, type: s.type, status: s.status,
    at: s.at, ended_at: s.ended_at, prompt: s.prompt, output: s.output, total: s.actions,
    items: acts.filter(a => a.parent === s.id) }))];
}
function laneStatus(l) {
  if (l.status === 'running') return `<span class="ev-status running"><span class="spin"></span>${l.at ? dur(data.now - l.at) : 'running'}</span>`;
  const took = l.at && l.ended_at ? dur(l.ended_at - l.at) : '';
  return l.status === 'error' ? `<span class="ev-status bad">✕ ${took}</span>` : `<span class="ev-status ok">✓ ${took}</span>`;
}
let subagentNames = {};  // Agent call id -> "pair-implementer: jobs module", for labelling a subagent's own actions
function subagentSummary(call) {
  const subs = call?.subagents || [];
  if (!subs.length) return '';
  const running = subs.filter(s => s.status === 'running').length;
  return `⇉ ${plural(subs.length, 'subagent')}${running ? ` · ${running} running` : ''}`;
}
const openEvents = new Set();
/* One plain-language line per action: "Edited jobs.rs", "Ran cargo check", ... */
function summarize(e) {
  if (e.kind === 'message') return 'Said: ' + e.text.replace(/\s+/g, ' ').slice(0, 110);
  if (e.kind === 'session') return e.text;
  const d = e.details || {}, name = e.name || e.text.split(':')[0];
  switch (name) {
    case 'Read': return 'Read ' + base(d.file_path || d.path) + (d.offset ? ` from line ${d.offset}` : '');
    case 'Edit': case 'MultiEdit': return 'Edited ' + base(d.file);
    case 'Write': return `Wrote ${base(d.file)}${d.lines ? ` (${d.lines} lines)` : ''}`;
    case 'Bash': return d.description ? d.description : 'Ran ' + String(d.command || '').replace(/\s+/g, ' ').replace(/^(cd \S+ *(&&|;) *)+/, '').slice(0, 100);
    case 'Grep': return 'Searched for ' + (d.pattern || '') + (d.path ? ' in ' + base(d.path) : '');
    case 'Glob': return 'Listed ' + (d.pattern || '');
    case 'Task': case 'Agent': return `Started ${d.subagent_type || 'a subagent'}: ` + (d.description || d.prompt || '').slice(0, 90);
    case 'WebFetch': return 'Fetched ' + (d.url || '');
    case 'WebSearch': return 'Searched the web: ' + (d.query || '');
    default: return e.text;
  }
}
function statusHTML(e) {
  if (e.kind !== 'tool') return '';
  if (e.status === 'running') return `<span class="ev-status running"><span class="spin"></span>${e.at ? dur(data.now - e.at) : 'running'}</span>`;
  const took = e.at && e.ended_at ? dur(Math.max(0, e.ended_at - e.at)) : '';
  return e.status === 'error' ? `<span class="ev-status bad">✕ ${took}</span>` : `<span class="ev-status ok">✓ ${took}</span>`;
}
function detailsHTML(e) {
  const d = e.details || {};
  let out = '';
  if (d.edits) out = d.edits.map(x => `<pre class="diff">${esc(x.old).split('\n').map(l => `<span class="del">- ${l}</span>`).join('\n')}\n${esc(x.new).split('\n').map(l => `<span class="add">+ ${l}</span>`).join('\n')}</pre>`).join('');
  else if (d.content !== undefined) out = pre(d.content);
  else if (d.command !== undefined) out = pre('$ ' + d.command);
  else if (Object.keys(d).length) out = pre(JSON.stringify(d, null, 2));
  if (d.file) out = `<div class="mono faint" style="margin:4px 0">${esc(d.file)}</div>` + out;
  if (e.output) out += `<div class="section-label">${e.status === 'error' ? 'Error' : 'Output'}</div>${pre(e.output)}`;
  return out;
}
function eventHTML(e, role, key) {
  const when = e.at ? `<time class="ev-time" title="${esc(new Date(e.at * 1000).toLocaleTimeString())}">${esc(ago(e.at))}</time>` : '';
  const who = e.parent && subagentNames[e.parent] ? `<span class="ev-who">${esc(subagentNames[e.parent])}</span>` : '';
  if (e.kind === 'tool') {
    const id = e.id || key, open = openEvents.has(id) ? ' open' : '';
    return `<details class="ev tool ${e.status || ''} ${e.parent ? 'sub' : ''}"${open}><summary data-ev="${esc(id)}"><span class="ev-icon">${esc(TOOL_ICON[e.name] || '•')}</span><span class="ev-text">${who}<b>${esc(e.name || '')}</b> ${esc(summarize(e).replace(/^(Read|Edited|Wrote|Ran|Searched for|Listed) ?/, m => ''))}</span><span class="ev-meta">${statusHTML(e)}${when}</span></summary><div class="ev-body">${detailsHTML(e)}</div></details>`;
  }
  if (e.kind === 'session') return `<div class="ev session"><span class="ev-icon">○</span><div class="ev-text">${esc(e.text)}</div><span class="ev-meta">${when}</span></div>`;
  return `<div class="ev message ${e.parent ? 'sub' : ''}" style="--role:${ROLES[role]?.color}"><span class="ev-icon">${e.parent ? '⇉' : ROLES[role]?.letter || '·'}</span><div class="ev-text">${who}${esc(e.text)}</div><span class="ev-meta">${when}</span></div>`;
}

function resultHTML(r, role) {
  if (!r) return '';
  const list = (label, items, mono) => items?.length ? `<div class="section-label">${label}</div><ul class="bullets">${items.map(x => `<li${mono ? ' class="mono"' : ''}>${esc(x)}</li>`).join('')}</ul>` : '';
  let out = `<div class="section-label">Summary</div><p class="summary">${esc(r.summary)}</p>`;
  if (role === 'director') {
    out += r.rationale ? `<div class="section-label">Rationale</div><p class="prose">${esc(r.rationale)}</p>` : '';
    if (r.batch?.tasks?.length) out += `<div class="section-label">Selected batch · ${esc(r.batch.title)}</div><ol class="tasks">${r.batch.tasks.map(t => `<li><div><strong>${esc(t.title)}</strong><div>${esc(t.brief)}</div></div></li>`).join('')}</ol>`;
  } else if (role === 'orchestrator') {
    out += `<div class="pills" style="margin-top:10px"><span class="chip small accent">${esc(r.action)}</span>${r.review !== 'none' ? `<span class="chip small ${r.review === 'accept' ? 'ok' : 'warn'}">${r.review === 'accept' ? 'accepted' : 'revise'}</span>` : ''}</div>`;
    if (r.worker_prompt) out += `<div class="section-label">Sent to the worker</div>${pre(r.worker_prompt)}`;
    out += list('Acceptance criteria', r.acceptance_criteria) + list('Checks', r.checks, true);
  } else {
    out += `<div class="pills" style="margin-top:10px"><span class="chip small ${r.status === 'done' ? 'ok' : 'bad'}">${esc(r.status)}</span></div>`;
    if (r.delegation) out += `<div class="section-label">How the work was split</div><p class="prose">${esc(r.delegation)}</p>`;
    out += list('Changed files', r.changed_files, true) + list('Checks the worker ran', r.checks, true) + list('Evidence', r.evidence) + list('Blockers', r.blockers);
  }
  return out + list('Notes for the team', r.coordination_notes);
}

function renderLive() {
  const role = liveRole(), follow = !pinnedRole, isChecks = role === 'checks';
  const callForTabs = isChecks ? null : latest(role);
  const tabs = isChecks ? [['activity', 'Output'], ['prompt', 'Plan']]
    : [['activity', 'Activity'], ...(callForTabs?.subagents?.length ? [['agents', `Agents · ${callForTabs.subagents.length + 1}`]] : []), ['prompt', 'Prompt'], ['result', 'Result']];
  if (!tabs.some(t => t[0] === liveTab)) liveTab = 'activity';
  const shown = ['orchestrator', 'worker', 'checks'];
  if (data.outer_settings.enabled || data.state.outer || data.calls.some(c => c.role === 'director')) shown.unshift('director');
  const liveNow = data.active ? followRole() : null;
  html('roles', shown.map(r => `<button data-role="${r}" aria-pressed="${r === role}"><span class="role-dot ${r === liveNow ? 'live' : ''}" style="background:${ROLES[r].color}"></span>${ROLES[r].name}</button>`).join(''), false);
  html('live-tabs', tabs.map(([k, l]) => `<button data-live-tab="${k}" aria-pressed="${k === liveTab}">${l}</button>`).join(''), false);
  $('follow').hidden = follow;
  text('follow-note', follow ? (data.active ? 'Following the active agent' : 'Showing the latest turn') : 'Pinned');

  const feed = $('feed'), atEnd = feed.scrollHeight - feed.scrollTop - feed.clientHeight < 40;
  let body = '', foot = [];
  if (isChecks) {
    const live = data.workflow.nodes.find(n => n.id === 'verify')?.live_output || [];
    if (liveTab === 'prompt') {
      const names = [...new Set(data.state.plan?.checks || [])];
      body = (names.length ? `<div class="section-label">Reruns the orchestrator requested</div><ul class="clean">${names.map(n => `<li class="mono">${esc(n)}</li>`).join('')}</ul>` : '<div class="empty">No coordinator reruns requested.</div>') + `<p class="prose faint" style="margin-top:12px">There is no fixed test suite. The worker chooses and runs the minimal tests that prove its change; the orchestrator reviews that evidence and can request a specific cheap rerun here.</p>`;
    } else if (live.length) {
      body = live.map(c => `<div class="ev check"><span class="ev-icon pulse">…</span><div class="ev-text mono"><b>${esc(c.name)}</b> running${pre(c.text.slice(-4000) || 'No output yet.')}</div></div>`).join('');
    } else if (data.checks.length) {
      body = data.checks.map(c => c.skipped ? `<div class="ev check"><span class="ev-icon">–</span><div class="ev-text mono"><b>${esc(c.name)}</b> · skipped after an earlier failure</div></div>`
        : `<div class="ev check ${c.exit_code ? 'fail' : 'pass'}"><span class="ev-icon">${c.exit_code ? '✕' : '✓'}</span><div class="ev-text mono"><b>${esc(c.name)}</b> · exit ${c.exit_code} · ${dur(c.seconds)}${pre((c.stderr_text || '') + (c.stdout_text || '') || 'No output')}</div></div>`).join('');
    } else body = '<div class="empty">No checks have run yet.</div>';
    foot.push('<span>Checks run locally in the project folder, independent of the agents.</span>');
  } else {
    const call = latest(role);
    subagentNames = Object.fromEntries((call?.subagents || []).map(s => [s.id, `${s.type}${s.description ? ': ' + s.description : ''}`]));
    if (!call) body = `<div class="empty">The ${esc(ROLES[role].name.toLowerCase())} hasn't run yet.</div>`;
    else if (liveTab === 'prompt') body = pre(call.prompt);
    else if (liveTab === 'agents') body = `<div class="agent-grid">${lanes(call).map((l, i) => `<article class="agent-card ${l.status}">
        <header><span class="ev-icon">${l.id ? '⇉' : esc(ROLES[call.role]?.letter || '·')}</span><div class="agent-title"><b>${esc(l.label)}</b><span class="faint">${esc(l.type || 'lead')} · ${l.total ?? l.items.length} actions</span></div>${l.id ? laneStatus(l) : ''}</header>
        <div class="agent-acts">${l.items.slice(-6).reverse().map(e => `<div class="agent-act"><span class="ev-icon">${esc(e.kind === 'message' ? '“' : (TOOL_ICON[e.name] || '•'))}</span><span class="recent-text">${esc(summarize(e))}</span><span class="ev-meta">${statusHTML(e)}${e.at ? `<time class="ev-time">${esc(ago(e.at))}</time>` : ''}</span></div>`).join('') || '<div class="empty">Starting…</div>'}</div>
        <button class="btn small" data-lane-open="${i}">All actions${l.prompt ? ' & brief' : ''} ↗</button></article>`).join('')}</div>`;
    else if (liveTab === 'result') body = call.result ? resultHTML(call.result, role) : `<div class="empty">${esc(call.error || (call.live ? 'Still working. The result appears when this turn finishes.' : 'This turn has no result.'))}</div>`;
    else body = call.activity?.length ? call.activity.map((e, i) => eventHTML(e, role, `${call.id}-${i}`)).join('') : `<div class="empty">${call.live ? 'Starting up. Activity appears as the agent reads, runs and writes.' : 'No activity recorded for this turn.'}</div>`;
    if (call) {
      foot.push(`<span>Turn <b>#${call.number}</b></span>`);
      if (call.model) foot.push(`<span><b>${esc(call.model)}</b>${call.fast ? ' · <b>fast</b>' : ''}</span>`);
      foot.push(`<span>${call.live ? 'running' : 'took'} <b>${dur(call.elapsed_seconds)}</b></span>`);
      if (call.cost_usd != null) foot.push(`<span><b>${money(call.cost_usd)}</b></span>`);
      if (call.subagents?.length) foot.push(`<span><b>${esc(subagentSummary(call))}</b></span>`);
      foot.push(`<span>updated <b>${esc(ago(call.last_activity_at))}</b></span>`);
      foot.push(`<span style="margin-left:auto">${call.live ? '<span class="chip small running"><span class="dot"></span>live</span>' : call.finished ? (call.subtype === 'success' ? '<span class="chip small ok">finished</span>' : '<span class="chip small bad">error</span>') : '<span class="chip small warn">interrupted</span>'}</span>`);
    }
  }
  const changed = html('feed', body);
  if (changed && liveTab === 'activity' && (atEnd || !rendered.feedInit)) feed.scrollTop = feed.scrollHeight;
  rendered.feedInit = true;
  html('live-foot', foot.join(''));
}

function renderAssignment() {
  const plan = data.state.plan;
  if (!plan) {
    html('assign-chip', '');
    return html('assignment', '<div class="empty">No assignment yet. The orchestrator writes the first one when the run starts.</div>');
  }
  const chip = plan.action === 'complete' ? ['info', 'Complete'] : plan.action === 'blocked' ? ['bad', 'Blocked'] : plan.review === 'accept' ? ['ok', 'Last result accepted'] : plan.review === 'revise' ? ['warn', 'Revision requested'] : ['accent', 'Assigned'];
  html('assign-chip', `<span class="chip small ${chip[0]}">${chip[1]}</span>`);
  html('assignment', `<p class="summary">${esc(plan.summary)}</p>` +
    (plan.acceptance_criteria?.length ? `<div class="section-label">Deliver</div><ul class="clean criteria">${plan.acceptance_criteria.map(c => `<li>${esc(c)}</li>`).join('')}</ul>` : '') +
    (plan.checks?.length ? `<div class="section-label">Checks</div><div class="pills">${plan.checks.map(c => `<span class="codepill" title="${esc(c)}">${esc(c)}</span>`).join('')}</div>` : '') +
    (plan.worker_prompt ? `<div class="row-end"><span class="note"></span><button class="btn small" data-open="worker-prompt">Full worker prompt ↗</button></div>` : ''));
}

function renderChecklist() {
  const items = data.state.plan?.checklist || [], done = items.filter(i => i.status === 'verified').length;
  text('checklist-count', items.length ? `${done} of ${items.length}` : '');
  if (!items.length) return html('checklist', '<div class="empty">The orchestrator builds this with its first assignment.</div>');
  html('checklist', `<div class="progress-inline" style="margin-bottom:6px"><div class="bar ok"><span style="width:${pct(done, items.length)}%"></span></div></div>` +
    items.map(i => `<div class="check-row"><span class="status-dot ${esc(i.status)}"></span><div><div class="title">${esc(i.workflow)}</div>${i.evidence && i.evidence !== 'Not yet verified' ? `<div class="evidence" title="${esc(i.evidence)}">${esc(i.evidence)}</div>` : ''}</div><span class="state">${esc(i.status.replace('_', ' '))}</span></div>`).join(''));
}

function renderChecks() {
  const phase = data.state.phase, running = data.active && ['verify', 'precheck'].includes(phase);
  const waived = data.state.plan?.waived_checks || [];
  const passed = data.checks.filter(c => c.exit_code === 0).length;
  text('checks-count', running ? 'reruns in progress…' : data.checks.length ? `${passed} of ${data.checks.length} reruns passed` : '');
  const report = data.state.report || latest('worker')?.result;
  const own = report?.checks?.length ? `<div class="section-label" style="margin-top:4px">Worker's verification</div><ul class="clean">${report.checks.map(c => `<li class="mono">${esc(c)}</li>`).join('')}</ul>` : '';
  if (!data.checks.length) return html('checks', own || `<div class="empty">${running ? 'Requested checks are running.' : 'The worker verifies its own work with tests it chooses; its commands and results appear here.'}</div>`);
  html('checks', own + '<div class="section-label">Coordinator reruns</div>' + data.checks.map((c, i) => {
    let badge = c.seconds ? `<span class="faint" style="font-size:11px">${dur(c.seconds)}</span>` : '';
    if (c.skipped) return `<button class="receipt" data-check="${i}"><span class="mark-skip">–</span><span class="cmd faint" title="${esc(c.name)}">${esc(c.name)}</span><span class="chip small">skipped</span></button>`;
    if (c.exit_code) badge = waived.includes(c.name) ? '<span class="chip small">waived</span>' : c.before ? (c.before.exit_code ? '<span class="chip small warn">pre-existing</span>' : '<span class="chip small bad">new failure</span>') : '<span class="chip small bad">failed</span>';
    return `<button class="receipt" data-check="${i}"><span class="${c.exit_code ? 'mark-bad' : 'mark-ok'}">${c.exit_code ? '✕' : '✓'}</span><span class="cmd" title="${esc(c.name)}">${esc(c.name)}</span>${badge}</button>`;
  }).join(''));
}

function captureURL(c) { return '/captures/' + c.path.split('/').map(encodeURIComponent).join('/') + '?t=' + Math.floor(c.at); }
function renderShots() {
  const shots = data.captures || [];
  text('shots-count', shots.length ? String(shots.length) : '');
  if (!shots.length) return html('shots', '<div class="empty">Screenshots are off for this run; agents verify by reading the code.</div>');
  html('shots', `<div class="shots">${shots.slice(0, 9).map((c, i) => `<button class="shot" data-shot="${i}" title="${esc(c.path)}"><img loading="lazy" alt="${esc(c.path)}" src="${captureURL(c)}"><span>${esc(c.path)}</span></button>`).join('')}</div>`);
}

function renderCommits() {
  const g = data.git || {}, commits = g.commits || [];
  text('commits-count', commits.length ? String(commits.length) : '');
  const pending = (g.uncommitted || 0) + (g.untracked || 0);
  const tail = `<div class="faint" style="font-size:12px;margin-top:8px">${g.files ? `${plural(g.files, 'file')} changed since the run began · ` : ''}${pending ? plural(pending, 'uncommitted change') : 'nothing uncommitted'}</div>`;
  if (!commits.length) return html('commits', `<div class="empty">No commits yet in this run.</div>${tail}`);
  html('commits', commits.slice(0, 8).map(c => `<div class="commit"><span class="hash">${esc(c.hash)}</span><span class="subject" title="${esc(c.subject)}">${esc(c.subject)}</span><time>${esc(ago(c.at))}</time></div>`).join('') + tail);
}

function renderGuide() {
  if (!dirty.guidance && document.activeElement !== $('guidance')) $('guidance').value = data.steering?.text || '';
  html('guide-chip', data.steering_pending ? '<span class="chip small warn">queued</span>' : data.steering?.text ? '<span class="chip small ok">seen</span>' : '');
  text('guide-note', data.steering_pending ? 'Saved. The orchestrator reads it at its next turn.' : 'Read at the next orchestrator turn. A queued assignment is reconsidered first.');
}

/* ---------- timeline ---------- */
function callStatus(c) {
  return c.live ? '<span class="chip small running"><span class="dot"></span>live</span>' : !c.finished ? '<span class="chip small warn">interrupted</span>' : c.subtype === 'success' ? '<span class="chip small ok">done</span>' : '<span class="chip small bad">error</span>';
}
function renderTimeline() {
  if (!data.calls.length) return html('timeline', '<div class="card card-body empty">Every agent turn will be listed here, newest first.</div>');
  html('timeline', [...data.calls].reverse().map(c => {
    const role = ROLES[c.role], title = c.live ? ({ assign: 'Writing the assignment', review: 'Reviewing the result', worker: 'Implementing', director: 'Choosing the next batch' })[c.stage] : STAGE_DONE[c.stage] || role.name;
    const summary = c.result?.summary || c.error || c.prompt.split('\n').find(l => l.trim()) || '';
    return `<button class="turn" data-call="${esc(c.id)}" style="--role:${role.color}"><span class="stripe"></span><span class="avatar">${role.letter}</span><span style="min-width:0"><span class="turn-title">${esc(title)} <span class="num">#${c.number} · ${esc(role.name)}</span></span><div class="turn-sum">${esc(summary)}</div></span><span class="turn-meta">${c.subagents?.length ? `<span title="subagents">⇉ ${c.subagents.length}</span>` : ''}${c.cost_usd != null ? `<span>${money(c.cost_usd)}</span>` : ''}<span>${dur(c.elapsed_seconds)}</span>${callStatus(c)}</span></button>`;
  }).join(''));
}
function openCall(id) {
  const c = data.calls.find(x => x.id === id); if (!c) return;
  const act = c.activity?.length ? `<div class="section-label">Activity</div>${c.activity.map(e => eventHTML(e, c.role)).join('')}` : '';
  modal(`Turn #${c.number} · ${ROLES[c.role].name}`, (c.result ? resultHTML(c.result, c.role) : '') + act +
    `<div class="section-label">Prompt</div>${pre(c.prompt)}` + (c.error ? `<div class="section-label">Error</div>${pre(c.error)}` : '') + (c.stderr ? `<div class="section-label">Error log</div>${pre(c.stderr)}` : ''));
}

/* ---------- notebook ---------- */
function renderNotebook() {
  const book = data.notebook;
  text('nb-caption', book.total > book.entries.length ? `Latest ${book.entries.length} of ${book.total} entries, newest first` : `${plural(book.total, 'entry')}, newest first`);
  if (!book.entries.length) return html('notebook', '<div class="card card-body empty">The team notebook starts with the first agent turn.</div>');
  html('notebook', [...book.entries].reverse().map(e => {
    const role = ROLES[e.author] || ROLES.coordinator;
    return `<article class="entry" style="--role:${role.color}"><div class="entry-head"><span class="author">${esc(role.name)}</span><span>${esc(e.kind)}${e.historical ? ' · imported' : ''}</span><time title="${esc(new Date(e.at * 1000).toLocaleString())}">${esc(ago(e.at))}</time></div><p>${esc(e.summary)}</p>${e.notes?.length ? `<ul>${e.notes.map(n => `<li>${esc(n)}</li>`).join('')}</ul>` : ''}</article>`;
  }).join(''));
}

/* ---------- decisions ---------- */
function renderDecisions() {
  const log = data.state.decisions || [];
  if (!log.length) return html('decisions', '<div class="card card-body empty">No decisions yet. When an agent makes a call you would otherwise be asked about, it appears here with its reasons.</div>');
  html('decisions', [...log].reverse().map(d => {
    const role = ROLES[d.role] || ROLES.coordinator;
    return `<article class="entry" style="--role:${role.color}"><div class="entry-head"><span class="author">${esc(role.name)}</span><span>call ${d.call}${d.batch ? ' · batch ' + esc(d.batch) : ''}</span><time title="${esc(new Date(d.at * 1000).toLocaleString())}">${esc(ago(d.at))}</time></div><p><strong>${esc(d.decision)}</strong></p><ul><li><b>Why:</b> ${esc(d.why)}</li><li><b>Alternatives:</b> ${esc(d.alternatives)}</li><li><b>Revisit if:</b> ${esc(d.revisit_if)}</li></ul></article>`;
  }).join(''));
}

/* ---------- director ---------- */
function renderDirector() {
  const outer = data.state.outer, settings = data.outer_settings, batch = outer?.current_batch;
  const active = data.active && data.state.inflight?.role === 'director';
  const chip = active ? ['running', 'Choosing now'] : !settings.enabled ? ['', 'Automatic planning off'] : batch ? ['accent', 'In progress'] : outer?.last_decision?.action === 'stop' ? ['warn', 'Stopped with a reason'] : ['', 'Ready'];
  html('dir-chip', `<span class="chip small ${chip[0]}">${chip[1]}</span>`);
  html('dir-batch', batch ? `<h3 style="margin:0 0 4px;font-size:16px">${esc(batch.title)}</h3><p class="prose">${esc(batch.objective)}</p><div class="section-label">Milestones</div><ol class="tasks">${batch.tasks.map(t => `<li><div><strong>${esc(t.title)}</strong><div>${esc(t.brief)}</div></div></li>`).join('')}</ol>` + (batch.outcomes?.length ? `<div class="section-label">Outcomes</div><ul class="bullets">${batch.outcomes.map(o => `<li>${esc(o)}</li>`).join('')}</ul>` : '')
    : `<div class="empty">${settings.enabled ? 'The Director selects an epic after the current work is accepted.' : 'Turn on automatic planning to let the Director choose each next epic. Without it, the orchestrator works through the mission directly.'}</div>`);
  html('hopper', outer?.hopper?.length ? outer.hopper.map(c => `<article class="cand ${c.disposition}"><div class="pills"><span class="chip small ${c.disposition === 'select' ? 'accent' : c.disposition === 'completed' ? 'ok' : ''}">${esc(c.disposition)}</span><span class="chip small">${esc(c.category.replace('_', ' '))}</span><span class="chip small">${esc(c.effort)}</span></div><h3>${esc(c.title)}</h3><p>${esc(c.benefit)}</p><details><summary>Why, evidence and risk</summary><p>${esc(c.reason)}</p><p>${esc(c.problem)}</p><p>Risk: ${esc(c.risk)}</p><ul class="bullets">${c.evidence.map(x => `<li>${esc(x)}</li>`).join('')}</ul></details></article>`).join('') : '<div class="empty">No candidates yet.</div>');
  const aside = outer?.set_aside || [];
  $('aside-card').hidden = !aside.length; text('aside-count', aside.length ? String(aside.length) : '');
  html('dir-aside', aside.map(b => `<div style="padding:6px 0"><strong style="font-size:13px">${esc(b.title)}</strong><div class="faint" style="font-size:12px">${esc(b.reason)}</div></div>`).join(''));
  const history = (outer?.history || []).filter(b => !b.legacy);
  text('history-count', history.length ? String(history.length) : '');
  html('dir-history', history.length ? history.map((b, i) => `<button class="btn ghost small" data-batch="${i}" style="display:flex;width:100%;justify-content:space-between">${esc(b.title)}<span class="faint">${esc(ago(b.completed_at))} ↗</span></button>`).join('') : '<div class="empty">None yet.</div>');
  if (!dirty.outer) { $('outer-enabled').checked = settings.enabled; $('max-batches').value = settings.max_batches ?? ''; $('max-batches').placeholder = 'No limit'; }
}

/* ---------- settings ---------- */
function renderSettings() {
  for (const k of LIMIT_KEYS) { $(k).disabled = data.active; $(k).placeholder = 'No limit'; if (!dirty.limits && document.activeElement !== $(k)) $(k).value = data.limits[k] ?? ''; }
  $('save-limits').disabled = data.active || busy;
  text('limit-note', data.active ? 'Stop the run to change limits' : '');
  const sessions = Object.entries(data.state.sessions || {}).map(([r, id]) => `${r}: ${id}`).join('\n') || 'none yet';
  html('run-info', [['Workspace', data.workspace], ['Branch', data.branch || 'detached HEAD'], ['Baseline', data.baseline], ['Run state', data.state_dir], ['Fast mode', (data.fast_roles || []).join(', ') || 'off'], ['Sessions', sessions]]
    .map(([k, v]) => `<dt>${k}</dt><dd style="white-space:pre-wrap">${esc(v)}</dd>`).join(''));
}

/* ---------- networking ---------- */
async function refresh() {
  try {
    const r = await fetch('/api/state', { cache: 'no-store' });
    if (!r.ok) throw Error();
    data = await r.json();
    render();
  } catch {
    $('conn').classList.add('lost'); text('conn', 'Reconnecting');
    $('start').disabled = $('stop').disabled = true;
  }
}
async function post(path, payload = {}) {
  const r = await fetch('/api/' + path, { method: 'POST', headers: { 'Content-Type': 'application/json', 'X-Pair-Token': TOKEN }, body: JSON.stringify(payload) });
  const out = await r.json();
  if (!r.ok) { const e = Error(out.error || 'Request failed'); e.interrupted = out.interrupted; throw e; }
  return out;
}
async function act(path, payload = {}) {
  if (busy) return;
  busy = true; if (data) render();
  try { toast((await post(path, payload)).message); await refresh(); }
  catch (e) {
    if (e.interrupted) toast('The last turn was interrupted. Use "Resume interrupted turn" to continue it.');
    else toast(e.message);
  } finally { busy = false; if (data) render(); }
}

/* ---------- events ---------- */
function setView(v) {
  view = v;
  document.querySelectorAll('[data-view]').forEach(t => t.setAttribute('aria-selected', String(t.dataset.view === v)));
  document.querySelectorAll('[data-panel]').forEach(p => p.hidden = p.dataset.panel !== v);
  try { localStorage.setItem('pair-view', v); } catch {}
  if (data) render();
}
document.addEventListener('click', e => {
  const sum = e.target.closest('summary[data-ev]');
  if (sum) { const id = sum.dataset.ev; openEvents.has(id) ? openEvents.delete(id) : openEvents.add(id); return; }
  const ln = e.target.closest('[data-lane]');
  if (ln) return openLane(renderRecent.call, renderRecent.lanes[Number(ln.dataset.lane)]);
  const lo = e.target.closest('[data-lane-open]');
  if (lo) { const c = latest(liveRole()); return openLane(c, lanes(c)[Number(lo.dataset.laneOpen)]); }
  const rec = e.target.closest('[data-recent]');
  if (rec) { const ev = renderRecent.acts[Number(rec.dataset.recent)]; return modal(summarize(ev), ev.kind === 'message' ? `<p class="prose">${esc(ev.text)}</p>` : detailsHTML(ev) || '<div class="empty">No details.</div>'); }
  const t = e.target.closest('[data-view]'); if (t) return setView(t.dataset.view);
  const r = e.target.closest('[data-role]'); if (r) { pinnedRole = r.dataset.role === followRole() ? null : r.dataset.role; rendered.feedInit = false; return render(); }
  const lt = e.target.closest('[data-live-tab]'); if (lt) { liveTab = lt.dataset.liveTab; rendered.feedInit = false; return render(); }
  const call = e.target.closest('[data-call]'); if (call) return openCall(call.dataset.call);
  const chk = e.target.closest('[data-check]');
  if (chk) { const c = data.checks[Number(chk.dataset.check)]; if (c.skipped) return modal('Check · ' + c.name, `<p class="prose">${esc(c.note)}</p>`); return modal('Check · ' + c.name, `<div class="section-label">Command</div>${pre(c.command.join(' '))}<div class="section-label">Exit code ${c.exit_code} · ${dur(c.seconds)}</div>${pre((c.stdout_text || '') + (c.stderr_text ? '\n' + c.stderr_text : '') || 'No output')}` + (c.before ? `<div class="section-label">Before the assignment · exit ${c.before.exit_code}</div><p class="prose faint">${c.before.exit_code ? 'This check was already failing before this assignment.' : 'This check passed before this assignment, so the failure is new.'}</p>` : '')); }
  const shot = e.target.closest('[data-shot]');
  if (shot) { const c = data.captures[Number(shot.dataset.shot)]; return modal(c.path, `<div class="lightbox"><img alt="${esc(c.path)}" src="${captureURL(c)}"></div><p class="faint" style="font-size:12px;text-align:center">${esc(new Date(c.at * 1000).toLocaleString())} · ${(c.bytes / 1024).toFixed(0)} KB</p>`); }
  const open = e.target.closest('[data-open]'); if (open) return modal('Worker prompt', pre(data.state.plan?.worker_prompt || ''));
  const doc = e.target.closest('[data-doc]');
  if (doc) { const k = doc.dataset.doc; return modal(k === 'mission' ? 'Mission & boundaries' : ROLES[k].name + ' instructions', pre(k === 'mission' ? data.mission : data.roles[k] || '')); }
  const b = e.target.closest('[data-batch]'); if (b) return modal('Accepted batch', pre(JSON.stringify(data.state.outer.history.filter(x => !x.legacy)[Number(b.dataset.batch)], null, 2)));
});
$('follow').onclick = () => { pinnedRole = null; rendered.feedInit = false; render(); };
$('start').onclick = () => act('start');
$('stop').onclick = () => act('stop');
let bannerAction = 'resume';
$('banner-action').onclick = () => bannerAction === 'retry-now' ? act('retry-now') : act('start', { retry_interrupted: true });
$('modal-close').onclick = closeModal;
$('modal').onclick = e => { if (e.target === $('modal')) closeModal(); };
document.addEventListener('keydown', e => { if (e.key === 'Escape' && !$('modal').hidden) closeModal(); });
$('guidance').oninput = () => dirty.guidance = true;
$('save-guidance').onclick = async () => { try { toast((await post('steering', { text: $('guidance').value })).message); dirty.guidance = false; await refresh(); } catch (e) { toast(e.message); } };
LIMIT_KEYS.forEach(k => $(k).oninput = () => dirty.limits = true);
$('save-limits').onclick = async () => { try { toast((await post('limits', Object.fromEntries(LIMIT_KEYS.map(k => [k, $(k).value.trim() === '' ? null : Number($(k).value)])))).message); dirty.limits = false; await refresh(); } catch (e) { toast(e.message); } };
$('outer-enabled').onchange = $('max-batches').oninput = () => dirty.outer = true;
$('save-outer').onclick = async () => { try { toast((await post('outer', { enabled: $('outer-enabled').checked, max_batches: $('max-batches').value.trim() === '' ? null : Number($('max-batches').value) })).message); dirty.outer = false; await refresh(); } catch (e) { toast(e.message); } };
$('nb-system').onclick = () => modal('How the team works', pre(data.notebook.system));
$('nb-current').onclick = () => modal('Shared context', pre(JSON.stringify({ status: data.state.status, phase: data.state.phase, batch: data.state.outer?.current_batch, assignment: data.state.plan, latest_worker_report: data.state.report, checks: data.state.receipts, guidance: data.steering?.text }, null, 2)));
$('nb-full').onclick = async () => { try { const r = await fetch('/api/journal'); modal('Full journal', pre(await r.text())); } catch { toast('Could not read the journal'); } };
$('dir-prompt').onclick = () => modal("Director's principles", pre(data.roles.director || ''));
$('dir-decision').onclick = () => modal('Latest Director decision', pre(data.state.outer?.last_decision ? JSON.stringify(data.state.outer.last_decision, null, 2) : 'No decision yet.'));
$('dir-roadmap').onclick = () => modal('Long-term roadmap', pre(data.state.outer?.roadmap?.length ? data.state.outer.roadmap.map(i => `${i.status} · ${i.workflow}\n${i.evidence}`).join('\n\n') : 'Nothing recorded yet.'));

/* ---------- theme ---------- */
const THEMES = ['system', 'light', 'dark'];
function applyTheme(t) {
  if (t === 'system') document.documentElement.removeAttribute('data-theme'); else document.documentElement.dataset.theme = t;
  $('theme').title = 'Theme: ' + t; $('theme').textContent = t === 'light' ? '☀' : t === 'dark' ? '☾' : '◐';
}
let theme = 'system';
try { theme = localStorage.getItem('pair-theme') || 'system'; } catch {}
applyTheme(theme);
$('theme').onclick = () => { theme = THEMES[(THEMES.indexOf(theme) + 1) % 3]; applyTheme(theme); try { localStorage.setItem('pair-theme', theme); } catch {} };

try { const v = localStorage.getItem('pair-view'); if (v && document.querySelector(`[data-panel="${v}"]`)) setView(v); } catch {}
refresh();
setInterval(refresh, 2000);
