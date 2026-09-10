// Archive an explicit, caller-verified terminal file set into a shared blob store.
// Each blob round-trips before the immutable manifest is written.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
import {gzipSync, gunzipSync} from 'node:zlib';
const [specPath] = process.argv.slice(2);
assert(specPath, 'usage: archive_evidence_files spec.json');
const bytes = fs.readFileSync(specPath), spec = JSON.parse(bytes);
assert.equal(spec.version, 1);
assert(Array.isArray(spec.files) && spec.files.length > 0);
assert.equal(new Set(spec.files).size, spec.files.length);
assert(!fs.existsSync(spec.manifest), 'refusing to replace evidence manifest');
assert(spec.scope && spec.terminal_evidence);
const sha = b => crypto.createHash('sha256').update(b).digest('hex');
fs.mkdirSync(spec.blob_directory, {recursive:true});
const archives = [];
for (const file of spec.files.slice().sort()) {
  const stat = fs.lstatSync(file);
  assert(stat.isFile() && !stat.isSymbolicLink(), 'explicit ordinary files required');
  const originalBytes = fs.readFileSync(file), digest = sha(originalBytes);
  const archive = path.join(spec.blob_directory, digest+'.gz');
  if (!fs.existsSync(archive)) fs.writeFileSync(archive, gzipSync(originalBytes, {level:6}), {flag:'wx'});
  const compressed = fs.readFileSync(archive);
  assert(gunzipSync(compressed).equals(originalBytes), 'archive round-trip failed');
  assert.equal(sha(fs.readFileSync(file)), digest, 'source changed during archival');
  archives.push({original:{path:file,bytes:originalBytes.length,sha256:digest},
    archive:{path:archive,bytes:compressed.length,sha256:sha(compressed)}});
}
// Recheck the entire set after compression; never certify a moving input set.
for (const row of archives) assert.equal(sha(fs.readFileSync(row.original.path)), row.original.sha256);
const manifest = {version:1, archives, scope:spec.scope, terminal_evidence:spec.terminal_evidence,
  specification:{path:specPath,sha256:sha(bytes)},
  code:{path:import.meta.filename,sha256:sha(fs.readFileSync(import.meta.filename))}};
fs.writeFileSync(spec.manifest, JSON.stringify(manifest,null,2)+'\n', {flag:'wx'});
console.log(JSON.stringify({manifest:spec.manifest,files:archives.length,roundtrip_verified:true}));
