/* tslint:disable */
/* eslint-disable */

/**
 * The same incremental mechanism/motor runner used by integrate_embedding.
 * Hosts choose bounded work chunks, never a rendering-dependent physics dt.
 */
export class EmbeddedSimulation {
    free(): void;
    [Symbol.dispose](): void;
    advance(steps: number): string;
    frame(): string;
    inputs(): string;
    metadata(): string;
    constructor(scene_json: string, config_json: string, seed: number);
    /**
     * Return the required replay step count. Transport advances it in bounded
     * chunks so progress and cancellation remain available between calls.
     */
    prepare_replay(json: string): number;
    recording(): string;
    set_inputs(values: Float64Array): void;
}

/**
 * Teacher training transitions use exactly the native environment adapter.
 * Invoke from a worker: one action interval can take longer than a display frame.
 */
export class EnvironmentSimulation {
    free(): void;
    [Symbol.dispose](): void;
    advance_replay(): string;
    contract(): string;
    frame(): string;
    inputs(): string;
    metadata(): string;
    constructor(scene_json: string, config_json: string, task_json: string, seed: number);
    /**
     * Same typed, read-only forecast query as the native environment.
     */
    predict_controller_trajectory(model_json: string, previous_json: string, actions_json: string): string;
    prepare_replay(json: string): number;
    recording(): string;
    reset(seed: number): string;
    step(action: Float64Array): string;
}

/**
 * Host-driven motion trial, including incremental checkpoint reconstruction.
 */
export class MotionEvaluation {
    free(): void;
    [Symbol.dispose](): void;
    advance(maximum_actions: number): string;
    checkpoint(): string;
    frame(): string;
    metadata(): string;
    constructor(experiment_json: string, proposal_json: string);
    static resume(experiment_json: string, checkpoint_json: string): MotionEvaluation;
}

export class Simulation {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Method-consistent linear contact impulses on a fully audited window.
     */
    contact_impulse_report(start: number, end: number): string;
    frame(): string;
    implicit_attempt_report(): string;
    inputs(): string;
    constructor(scene_json: string, seed: number);
    recording(): string;
    replay(recording_json: string): string;
    reset(seed: number): string;
    /**
     * Diagnostic only: capture the same implicit stages as native sim-validate.
     */
    set_attempt_audit_limit(limit: number): void;
    step(action: Float64Array): string;
}

/**
 * Validate and bind an immutable motion experiment to current library sources.
 */
export function bind_motion_experiment(spec_json: string): string;

/**
 * Read-only comparison using the same typed fidelity API as native experiments.
 */
export function compare_environment_fidelity(reference_json: string, candidate_json: string, plan_json: string): string;

/**
 * Inspect original CAD JSON before model parsing supplies legacy defaults.
 * Call from a worker; no simulation session or source geometry is modified.
 */
export function inspect_robot_contract(document_json: string): string;

/**
 * Pure preparation for the ordinary shared environment; call from a worker.
 */
export function materialize_motion(scene_json: string, actions_json: string, recipe_json: string, values_json: string): string;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_embeddedsimulation_free: (a: number, b: number) => void;
    readonly __wbg_environmentsimulation_free: (a: number, b: number) => void;
    readonly __wbg_motionevaluation_free: (a: number, b: number) => void;
    readonly __wbg_simulation_free: (a: number, b: number) => void;
    readonly bind_motion_experiment: (a: number, b: number) => [number, number, number, number];
    readonly compare_environment_fidelity: (a: number, b: number, c: number, d: number, e: number, f: number) => [number, number, number, number];
    readonly embeddedsimulation_advance: (a: number, b: number) => [number, number, number, number];
    readonly embeddedsimulation_frame: (a: number) => [number, number, number, number];
    readonly embeddedsimulation_inputs: (a: number) => [number, number, number, number];
    readonly embeddedsimulation_metadata: (a: number) => [number, number, number, number];
    readonly embeddedsimulation_new: (a: number, b: number, c: number, d: number, e: number) => [number, number, number];
    readonly embeddedsimulation_prepare_replay: (a: number, b: number, c: number) => [number, number, number];
    readonly embeddedsimulation_recording: (a: number) => [number, number, number, number];
    readonly embeddedsimulation_set_inputs: (a: number, b: number, c: number) => [number, number];
    readonly environmentsimulation_advance_replay: (a: number) => [number, number, number, number];
    readonly environmentsimulation_contract: (a: number) => [number, number, number, number];
    readonly environmentsimulation_frame: (a: number) => [number, number, number, number];
    readonly environmentsimulation_inputs: (a: number) => [number, number, number, number];
    readonly environmentsimulation_metadata: (a: number) => [number, number, number, number];
    readonly environmentsimulation_new: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => [number, number, number];
    readonly environmentsimulation_predict_controller_trajectory: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => [number, number, number, number];
    readonly environmentsimulation_prepare_replay: (a: number, b: number, c: number) => [number, number, number];
    readonly environmentsimulation_recording: (a: number) => [number, number, number, number];
    readonly environmentsimulation_reset: (a: number, b: number) => [number, number, number, number];
    readonly environmentsimulation_step: (a: number, b: number, c: number) => [number, number, number, number];
    readonly inspect_robot_contract: (a: number, b: number) => [number, number, number, number];
    readonly materialize_motion: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number) => [number, number, number, number];
    readonly motionevaluation_advance: (a: number, b: number) => [number, number, number, number];
    readonly motionevaluation_checkpoint: (a: number) => [number, number, number, number];
    readonly motionevaluation_frame: (a: number) => [number, number, number, number];
    readonly motionevaluation_metadata: (a: number) => [number, number, number, number];
    readonly motionevaluation_new: (a: number, b: number, c: number, d: number) => [number, number, number];
    readonly motionevaluation_resume: (a: number, b: number, c: number, d: number) => [number, number, number];
    readonly simulation_contact_impulse_report: (a: number, b: number, c: number) => [number, number, number, number];
    readonly simulation_frame: (a: number) => [number, number, number, number];
    readonly simulation_implicit_attempt_report: (a: number) => [number, number, number, number];
    readonly simulation_inputs: (a: number) => [number, number, number, number];
    readonly simulation_new: (a: number, b: number, c: number) => [number, number, number];
    readonly simulation_recording: (a: number) => [number, number, number, number];
    readonly simulation_replay: (a: number, b: number, c: number) => [number, number, number, number];
    readonly simulation_reset: (a: number, b: number) => [number, number, number, number];
    readonly simulation_set_attempt_audit_limit: (a: number, b: number) => [number, number];
    readonly simulation_step: (a: number, b: number, c: number) => [number, number, number, number];
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
