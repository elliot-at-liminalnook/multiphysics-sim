import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { resolve, sep, extname } from 'node:path';
const root=resolve(process.argv[2] || 'runs/interactive/viewer');
const port=Number(process.argv[3] || 4173);
const server=createServer(async (req,res)=>{
  try {
    const path=decodeURIComponent(new URL(req.url,'http://localhost').pathname);
    const file=resolve(root,'.'+(path==='/'?'/index.html':path));
    if (!file.startsWith(root+sep)) throw new Error('path');
    res.setHeader('Cross-Origin-Opener-Policy','same-origin');res.setHeader('Cross-Origin-Embedder-Policy','require-corp');
    res.setHeader('Content-Type',({'.html':'text/html','.js':'text/javascript','.mjs':'text/javascript','.css':'text/css','.json':'application/json','.txt':'text/plain','.wasm':'application/wasm'})[extname(file)]||'application/octet-stream');
    res.end(await readFile(file));
  } catch {res.statusCode=404;res.end('Not found');}
}).listen(port,'127.0.0.1',()=>console.log(`Robot viewer: http://127.0.0.1:${server.address().port}`));
