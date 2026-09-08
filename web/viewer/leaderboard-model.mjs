// Shared by packaging, browser display and integrity tests. Missing evidence
// never satisfies a gate; physical travel and compute throughput are distinct.
export const requiredGates = ['sustained_walk', 'turn_reverse_stop', 'disturbances',
  'terrain', 'contact_motion', 'numerical_accuracy', 'browser_realtime', 'replay_parity'];
export function eligible(entry) {
  return requiredGates.every(key => entry.gates?.[key]?.status === 'pass') &&
    entry.metrics.task_passed === true &&
    Number.isFinite(entry.metrics.sustained_speed_m_s) && entry.metrics.sustained_speed_m_s >= 0 &&
    entry.metrics.simulated_s >= 60 && entry.metrics.speed_window_s >= 30;
}
export function rankEntries(entries) {
  const groups = new Map(), ranks = new Map();
  for (const e of entries) if (eligible(e)) {
    const group = groups.get(e.comparison_group) ?? []; group.push(e); groups.set(e.comparison_group, group);
  }
  for (const group of groups.values()) group.sort((a, b) => b.metrics.sustained_speed_m_s - a.metrics.sustained_speed_m_s || a.id.localeCompare(b.id))
    .forEach((e, i) => ranks.set(e.id, i + 1));
  return ranks;
}
export function validateEntry(e) {
  const hash = v => typeof v === 'string' && /^[a-f0-9]{64}$/.test(v);
  const source = s => s && typeof s.path === 'string' && s.path.length > 0 && hash(s.sha256);
  if (!e || !/^[a-z0-9-]+$/.test(e.id) || !e.name || !e.description || !e.comparison_group ||
      !e.load || !hash(e.load.asset_sha256) || !Number.isSafeInteger(e.load.seed) || e.load.seed < 0 ||
      ![e.load.scene, e.load.config, e.load.task].every(source) ||
      ![e.controller_sha256, e.model_sha256, e.environment_sha256, e.fidelity_sha256, e.cad_sha256].every(hash) || !e.benchmark_version ||
      !e.metrics || !Number.isFinite(e.metrics.simulated_s) || e.metrics.simulated_s <= 0 ||
      !Number.isFinite(e.metrics.speed_window_s) || e.metrics.speed_window_s < 0 ||
      !(e.metrics.sustained_speed_m_s === null || Number.isFinite(e.metrics.sustained_speed_m_s) && e.metrics.sustained_speed_m_s >= 0) ||
      !requiredGates.every(k => ['pass', 'fail', 'missing'].includes(e.gates?.[k]?.status) && e.gates[k].detail) ||
      !Array.isArray(e.replay?.input_events) || !Number.isSafeInteger(e.replay.completed_steps) || e.replay.completed_steps <= 0 ||
      !e.replay.input_events.every((event, i, events) => Number.isSafeInteger(event.at_step) && event.at_step >= 0 &&
        event.at_step < e.replay.completed_steps && (!i || event.at_step > events[i - 1].at_step) &&
        Array.isArray(event.values) && event.values.length > 0 && event.values.every(Number.isFinite)) ||
      !Array.isArray(e.evidence) || e.evidence.length === 0 || !e.evidence.every(source)) throw Error('Invalid controller evaluation entry');
  if(e.command_response){
    const r=e.command_response, duration=v=>v===null||Number.isFinite(v)&&v>=0;
    if(!r.method||!r.scope||!Array.isArray(r.cases)||!r.cases.length||
      new Set(r.cases.map(c=>c.command)).size!==r.cases.length||r.cases.some(c=>!c.command||
        ![c.simulated_response_s,c.received_wall_s,c.drawn_wall_s].every(duration)||
        !Number.isFinite(c.threshold)||c.threshold<=0||!['m','rad'].includes(c.threshold_unit)||
        !Number.isFinite(c.hold_s)||c.hold_s<=0))throw Error('Invalid command response evidence');
  }
  return e;
}
