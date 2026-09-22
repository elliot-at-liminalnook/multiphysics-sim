// Verify source immutability and equal measurement content across UART runs.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import {fileURLToPath} from 'node:url';
const base=path.dirname(fileURLToPath(import.meta.url));
const digest=p=>crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const provenance=JSON.parse(fs.readFileSync(path.join(base,'provenance.json')));
for(const [p,h] of Object.entries(provenance.sources))
    if(digest(path.resolve(base,p))!==h)throw Error('Input changed: '+p);
function parse(name) {
    const bytes=fs.readFileSync(path.join(base,name+'-uart.hex'),'utf8').trim().split(/\s+/).map(x=>parseInt(x,16));
    const records=[];let packets=0;
    for(let i=0;i<bytes.length;) {
        if(bytes[i]!==255||bytes[i+1]!==255)throw Error('Header');
        const n=bytes[i+3]+4,b=bytes.slice(i,i+n);
        if(n<6||b.length!==n||b.slice(2).reduce((s,x)=>s+x,0)%256!==255)throw Error('Packet checksum');
        packets++;
        if(b[2]===253&&b[7]>=1&&b[7]<=3)records.push([...b.slice(0,17),...b.slice(33,-1)]);
        i+=n;
    }
    if(records.length!==175)throw Error('Record count');
    return {packets,records};
}
const a=parse('host-1mbaud'),b=parse('host-2mbaud');
if(JSON.stringify(a.records)!==JSON.stringify(b.records))throw Error('Measurement content differs');
const files=Object.fromEntries(fs.readdirSync(base).filter(f=>f!=='verification.json').sort().map(f=>[f,digest(path.join(base,f))]));
const v={input_hashes_verified:Object.keys(provenance.sources).length,
    uart_packets:[a.packets,b.packets],transaction_records_each:a.records.length,
    transaction_identity_and_payload_parity:true,
    excluded_from_parity:'Request/completion timestamp bytes and their packet checksum; other transaction-event bytes match.',
    files_sha256:files};
fs.writeFileSync(path.join(base,'verification.json'),JSON.stringify(v,null,2)+'\n');
console.log(JSON.stringify({...v,files_sha256:'retained in verification.json'},null,2));
