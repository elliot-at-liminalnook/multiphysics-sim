// Restore one explicitly selected artifact; preserve source and existing files.
import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {gunzipSync} from 'node:zlib';
import assert from 'node:assert/strict';
const [manifestPath,originalPath,destination]=process.argv.slice(2);
assert(destination,'usage: restore_evidence_file.mjs manifest.json original-path fresh-destination');
const manifest=JSON.parse(fs.readFileSync(manifestPath));
const matches=manifest.archives.filter(a=>a.original.path===originalPath);
assert.equal(matches.length,1,'unique archived source required');
const entry=matches[0],compressed=fs.readFileSync(entry.archive.path);
const sha=b=>createHash('sha256').update(b).digest('hex');
assert.equal(sha(compressed),entry.archive.sha256);
const bytes=gunzipSync(compressed);
assert.equal(sha(bytes),entry.original.sha256);assert.equal(bytes.length,entry.original.bytes);
fs.mkdirSync(path.dirname(destination),{recursive:true});
fs.writeFileSync(destination,bytes,{flag:'wx'});
console.log(JSON.stringify({restored:destination,original:originalPath,sha256:entry.original.sha256}));
