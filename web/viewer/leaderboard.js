import {eligible, rankEntries, requiredGates, validateEntry} from './leaderboard-model.mjs';
const el = (tag, text, className) => { const node = document.createElement(tag); if (text != null) node.textContent = text; if (className) node.className = className; return node; };
const value = (v, digits = 2, unit = '') => Number.isFinite(v) ? `${v.toFixed(digits)}${unit}` : 'Not measured';
const labels = {sustained_walk: 'Sustained walk', turn_reverse_stop: 'Turning, reverse, stop', disturbances: 'Held-out disturbances', terrain: 'Harder terrain', numerical_accuracy: 'Numerical accuracy', browser_realtime: 'Browser realtime', replay_parity: 'Replay and host parity'};
export async function installLeaderboard({load, pause}) {
  const response = await fetch('leaderboard.json'); if (!response.ok) throw Error('Controller evaluations could not load');
  const data = await response.json(); if (data.version !== 1) throw Error('Unsupported evaluation catalog');
  const entries = data.entries.map(validateEntry), ranks = rankEntries(entries), selected = new Set();
  const dialog = document.getElementById('leaderboard-dialog'), rows = document.getElementById('leaderboard-rows');
  const search = document.getElementById('controller-search'), status = document.getElementById('controller-status'), profile = document.getElementById('controller-profile');
  const compare = document.getElementById('compare-controllers'), detail = document.getElementById('controller-comparison');
  const groups = new Map(entries.map(e => [e.comparison_group, `${e.fidelity_label} · ${e.environment_label} · ${e.metrics.simulated_s}s · ${e.comparison_group.slice(0, 6)}`]));
  for (const [key, label] of groups) { const option = el('option', label); option.value = key; profile.append(option); }
  document.getElementById('leaderboard-summary').textContent = `${entries.length} reproducible recipes · ${entries.filter(eligible).length} meet every declared gate. Speed ranks are assigned only within identical model, environment, fidelity and benchmark groups.`;
  function inspect(items) {
    detail.replaceChildren();
    if (new Set(items.map(e => e.comparison_group)).size > 1) detail.append(el('p', 'Different comparison groups: inspect these results side by side, but do not interpret them as a speed ranking.', 'notice'));
    const grid = el('div', null, 'controller-detail-grid');
    for (const e of items) {
      const card = el('article'); card.append(el('h3', e.name), el('p', e.description));
      const dl = el('dl');
      for (const [name, text] of [
        ['Sustained walking', value(e.metrics.sustained_speed_m_s == null ? null : e.metrics.sustained_speed_m_s * 1000, 3, ' mm/s')],
        ['Measured window', value(e.metrics.speed_window_s, 2, ' s')],
        ['Travel in measured window', value(e.metrics.short_speed_m_s == null ? null : e.metrics.short_speed_m_s * 1000, 3, ' mm/s')],
        ['Heading error at stop', value(e.metrics.final_heading_error_rad * 180 / Math.PI, 3, '°')],
        ['Final position error', value(e.metrics.final_position_error_m * 1000, 3, ' mm')],
        ['Physical stop latency', 'Not established by endpoint error'],
        ['Positive shaft work', value(e.metrics.positive_mechanical_work_j, 3, ' J (sampled)')],
        ['Native compute throughput', value(e.metrics.native_throughput, 2, '× (hardware not recorded)')],
        ['Active browser throughput', value(e.metrics.browser_active_throughput, 3, '×')],
        ['Active browser p95', value(e.metrics.browser_active_p95_s == null ? null : e.metrics.browser_active_p95_s * 1000, 2, ' ms')],
        ['Browser hardware', e.browser_host ? `${e.browser_host.cpu}; ${e.browser_host.platform}; browser ${e.browser_host.browser}` : 'Not recorded'],
        ['Controller version', e.controller_sha256], ['CAD model', e.cad_sha256],
        ['Environment version', e.environment_sha256], ['Benchmark', e.benchmark_version], ['Seed', String(e.load.seed)],
      ]) { dl.append(el('dt', name), el('dd', text)); }
      card.append(dl);
      const gates = el('ul', null, 'controller-gates');
      for (const key of requiredGates) { const gate = e.gates[key], li = el('li', `${labels[key]}: ${gate.status} — ${gate.detail}`); li.dataset.gate = gate.status; gates.append(li); }
      card.append(gates, el('p', e.limitations, 'muted'));
      const proof = el('button', 'Download evaluation'); proof.onclick = () => {
        const url = URL.createObjectURL(new Blob([JSON.stringify(e, null, 2)], {type: 'application/json'}));
        const a = el('a'); a.href = url; a.download = `${e.id}.evaluation.json`; a.click(); setTimeout(() => URL.revokeObjectURL(url), 1000);
      }; card.append(proof); grid.append(card);
    }
    detail.append(grid); detail.hidden = false;
  }
  async function run(entry, replay) { dialog.close(); await load(entry, replay); }
  function render() {
    rows.replaceChildren();
    const shown = entries.filter(e => `${e.name} ${e.description}`.toLowerCase().includes(search.value.toLowerCase()) &&
      (!profile.value || e.comparison_group === profile.value) &&
      (status.value === 'all' || (status.value === 'validated') === eligible(e)));
    for (const e of shown) {
      const row = el('tr'); row.dataset.controller = e.id;
      const pick = el('input'); pick.type = 'checkbox'; pick.checked = selected.has(e.id); pick.setAttribute('aria-label', `Compare ${e.name}`);
      pick.onchange = () => { if (pick.checked) selected.add(e.id); else selected.delete(e.id); compare.disabled = selected.size < 2; };
      const cell = el('td'); cell.append(pick); row.append(cell, el('td', ranks.get(e.id) ?? '—'));
      const name = el('td'); name.append(el('strong', e.name), el('p', e.description), el('small', `${e.fidelity_label} · ${e.metrics.simulated_s}s`)); row.append(name);
      const travel = el('td'), sustained = e.metrics.sustained_speed_m_s != null;
      const speed = sustained ? e.metrics.sustained_speed_m_s : e.metrics.short_speed_m_s;
      travel.append(el('span', value(speed == null ? null : speed * 1000, 3, ' mm/s')),
        el('small', sustained ? 'Sustained window' : 'Short window only', 'controller-window'));
      row.append(travel);
      row.append(el('td', `${e.metrics.task_passed ? 'Pass' : 'Fail'} · ${e.metrics.qualified_swings}/${e.metrics.swings} swings`));
      row.append(el('td', value(e.metrics.final_heading_error_rad * 180 / Math.PI, 3, '°')));
      row.append(el('td', `${value(e.metrics.browser_active_throughput, 3, '×')} / ${value(e.metrics.browser_active_p95_s == null ? null : e.metrics.browser_active_p95_s * 1000, 1, ' ms')}`));
      row.append(el('td', eligible(e) ? 'Validated for group' : 'Experimental'));
      const actions = el('td', null, 'controller-actions');
      const drive = el('button', 'Load and run', 'primary'); drive.onclick = () => run(e, false);
      const replay = el('button', 'Replay tested inputs'); replay.onclick = () => run(e, true);
      const inspectButton = el('button', 'Evidence'); inspectButton.onclick = () => inspect([e]);
      actions.append(drive, replay, inspectButton); row.append(actions);
      const headings = ['Compare', 'Rank', 'Controller', 'Measured travel', 'Physical task', 'Stop heading', 'Browser × / p95', 'Validation', 'Run exact recipe'];
      [...row.children].forEach((cell, i) => { cell.dataset.label = headings[i]; });
      rows.append(row);
    }
    document.getElementById('leaderboard-empty').hidden = shown.length > 0;
  }
  search.oninput = status.onchange = profile.onchange = render;
  compare.onclick = () => inspect(entries.filter(e => selected.has(e.id)));
  document.getElementById('open-leaderboard').onclick = () => { pause(); dialog.showModal(); render(); };
  document.getElementById('close-leaderboard').onclick = () => dialog.close();
  render();
}
