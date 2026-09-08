// Reproducible, isolated compiler-profile experiments. No physics overrides.
import {readFileSync, writeFileSync, mkdirSync, readdirSync, existsSync, copyFileSync} from 'node:fs';
import {resolve, join, relative} from 'node:path';
import {execFileSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import assert from 'node:assert/strict';
const root = resolve(import.meta.dirname, '..'), output = resolve(process.argv[2] || 'runs/wasm-builds/scalar');
const profile = process.argv[3] || 'scalar';
const profiles = {scalar: {rustflags: '', lto: 'false', codegen_units: '16'}, 'simd-lto': {rustflags: '-C target-feature=+simd128', lto: 'fat', codegen_units: '1'}};
assert(profiles[profile], 'profile must be scalar or simd-lto');
assert(!process.env.RUSTFLAGS, 'clear external RUSTFLAGS; this command records its complete compiler profile');
const cargo = process.env.CARGO_BIN || 'cargo', rustc = process.env.RUSTC || 'rustc';
const manifestPath = join(output, 'build.json'), artifact = join(output, 'sim_web.wasm');
assert(!existsSync(manifestPath) && !existsSync(artifact), 'refusing to overwrite a recorded compiler experiment');
mkdirSync(output, {recursive: true});
const hash = path => createHash('sha256').update(readFileSync(path)).digest('hex');
const files = ['Cargo.toml', 'Cargo.lock', ...readdirSync(join(root, 'crates'), {recursive: true}).filter(p => p.endsWith('.rs') || p.endsWith('Cargo.toml')).map(p => `crates/${p}`)];
const settings = profiles[profile];
const report = {version: 1, completed: false, profile, settings,
  target: 'wasm32-unknown-unknown', cargo: execFileSync(cargo, ['-Vv'], {encoding: 'utf8'}).trim(),
  rustc: execFileSync(rustc, ['-Vv'], {encoding: 'utf8'}).trim(),
  source_revision: execFileSync('git', ['rev-parse', 'HEAD'], {cwd: root, encoding: 'utf8'}).trim(),
  sources: files.map(path => ({path, sha256: hash(join(root, path))})),
  builder: {path: 'web/build-wasm.mjs', sha256: hash(import.meta.filename)},
  scope: 'Compiler optimization only. SIMD profile requires a browser supporting WebAssembly SIMD; physical equations and controller recipes are unchanged. Native/WASM agreement and browser performance require separate measured checks.'};
const save = () => writeFileSync(manifestPath, JSON.stringify(report, null, 2) + '\n'); save();
try {
  execFileSync(cargo, ['build', '--locked', '--release', '-p', 'sim-web', '--target', report.target, '--target-dir', join(output, 'target')],
    {cwd: root, stdio: 'inherit', env: {...process.env, RUSTFLAGS: settings.rustflags, CARGO_PROFILE_RELEASE_LTO: settings.lto, CARGO_PROFILE_RELEASE_CODEGEN_UNITS: settings.codegen_units}});
  copyFileSync(join(output, 'target', report.target, 'release/sim_web.wasm'), artifact);
  report.artifact = {path: relative(root, artifact), sha256: hash(artifact)}; report.completed = true; save();
  console.log(`Built ${profile}: ${artifact}`);
} catch (error) { report.error = error.message; save(); throw error; }
