// Archive all search outcomes, including rejected candidates and source hashes.
import {readFileSync, writeFileSync} from 'node:fs';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const [run, output] = process.argv.slice(2);
assert(run && output, 'usage: summarize_policy_suite.mjs run-directory report.json');
const read = p => JSON.parse(readFileSync(p));
const hash = p => createHash('sha256').update(readFileSync(p)).digest('hex');
const recipe = read(`${run}/recipe.json`), search = read(`${run}/search.json`);
let selected = 0;
const evaluations = Array.from({length: 1 + search.trials.length}, (_, index) => {
  const prefix = `${run}/evaluation-${String(index).padStart(4,'0')}`;
  const evaluated = read(`${prefix}.json`);
  const cases = recipe.cases.map((definition, i) => {
    const {name, report} = read(`${prefix}-case-${String(i).padStart(2,'0')}.json`);
    assert.equal(name, definition.name);
    const t = report.final_transition, w = t?.walking;
    if (report.score != null) {
      assert(!report.error && t.truncated && !t.terminated);
      assert.equal(report.score, report.accrued_reward);
      assert.equal(report.completed_actions, read(definition.actions).length);
    }
    return {name, score: report.score, reward_per_s: report.score == null ? null : report.score/t.time_s,
      error: report.error, completed_actions: report.completed_actions, time_s: t?.time_s,
      qualified: w?.qualified_steps, failed: w?.failed_steps,
      final_body_error_m: w ? Math.hypot(...w.body_error_world_m) : null,
      walking_outcomes: report.walking_outcomes};
  });
  const expected = cases.some(c => c.score == null) ? null : Math.min(...cases.map(c => c.reward_per_s));
  assert.equal(evaluated.score, expected);
  if (index > 0) {
    const trial = search.trials[index-1];
    assert.equal(trial.score, evaluated.score);
    if (trial.accepted) selected = index;
  }
  return {index, score: evaluated.score, error: evaluated.error, accepted: index === 0 || search.trials[index-1].accepted,
    policy_sha256: createHash('sha256').update(JSON.stringify(evaluated.policy)).digest('hex'), cases};
});
assert.equal(evaluations[0].score, search.initial_score);
assert.equal(evaluations[selected].score, search.best_score);
assert.deepEqual(read(`${run}/evaluation-${String(selected).padStart(4,'0')}.json`).policy, search.policy);
const paths = [...new Set([recipe.scene,...recipe.cases.flatMap(c => [c.config,c.task,c.actions]),
  'crates/sim-runtime/src/policy_evaluation.rs','crates/sim-runtime/examples/train_policy_suite.rs',
  'crates/sim-domain-control/src/policy_search.rs'])];
const report = {version:1, scope:'Development search, not controller promotion or independent walking acceptance. Physics, action limits and actor observation features unchanged; only neural parameters vary.',
  aggregation:'worst_reward_per_simulated_second', search:recipe.search, initial_score:search.initial_score,
  best_score:search.best_score, selected_evaluation:selected, evaluations,
  sources:paths.map(path=>({path,sha256:hash(path)})),
  resolved_recipe_sha256:hash(`${run}/resolved.json`)};
writeFileSync(output, JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify({initial:report.initial_score,best:report.best_score,selected,evaluations:evaluations.length}));
