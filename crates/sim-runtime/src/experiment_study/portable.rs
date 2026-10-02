//! SIMSTUDY v1: bounded manifest plus exact hash-keyed objects, never extracted.
//! All integers are little endian. Header: magic[8], version:u32,
//! manifest_length:u64, object_count:u32. Each object: lowercase hash[64],
//! length:u64, raw bytes. Sorted publication; decoding accepts any order but
//! rejects every duplicate (including identical bytes). No paths or receipts added.
use super::Study;
use std::{collections::BTreeMap, io::{Read, Write}, path::Path, sync::Arc};
pub const MAGIC: &[u8; 8] = b"SIMSTUDY";
pub const VERSION: u32 = 1;
pub const MAX_ARTIFACT_BYTES: usize = 256 * 1024 * 1024;
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_OBJECT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_OBJECTS: usize = 4096;
pub const MAX_JSON_DEPTH: usize = 64;
fn error(name: &str, detail: impl std::fmt::Display) -> String { format!("study.portable.{name}: {detail}") }

/// Bounds apply before reads, including files growing after metadata inspection.
pub(super) fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("study.source {}: {e}", path.display()))?;
    if file.metadata().map_err(|e|e.to_string())?.len() > limit as u64 { return Err(error("size", path.display())); }
    let mut bytes = Vec::new();
    file.take((limit as u64) + 1).read_to_end(&mut bytes).map_err(|e|format!("study.source {}: {e}",path.display()))?;
    if bytes.len() > limit { return Err(error("size", path.display())); }
    Ok(bytes)
}
/// Lexical nesting scan runs before serde allocations; escaped quotes/brackets
/// inside strings are ignored. Serde supplies syntax and Unicode validation.
pub(super) fn json_bounds(bytes: &[u8], limit: usize) -> Result<(), String> {
    if bytes.len() > limit { return Err(error("json_size", bytes.len())); }
    let (mut depth, mut quoted, mut escaped) = (0usize, false, false);
    for &byte in bytes {
        if quoted { if escaped { escaped=false; } else if byte==b'\\' { escaped=true; } else if byte==b'"' { quoted=false; } }
        else { match byte { b'"'=>quoted=true, b'{'|b'['=>{depth=depth.checked_add(1).ok_or_else(||error("json_depth","overflow"))?;if depth>MAX_JSON_DEPTH{return Err(error("json_depth",depth));}}, b'}'|b']'=>{depth=depth.saturating_sub(1);}, _=>{} } }
    }
    Ok(())
}
struct BoundedWriter { bytes: Vec<u8>, depth: usize, quoted: bool, escaped: bool }
impl Write for BoundedWriter {
    fn write(&mut self, bytes:&[u8])->std::io::Result<usize>{
        if self.bytes.len().checked_add(bytes.len()).is_none_or(|n|n>MAX_MANIFEST_BYTES){return Err(std::io::Error::other("study.portable.manifest_size"));}
        for &byte in bytes {
            if self.quoted { if self.escaped { self.escaped=false; } else if byte==b'\\' { self.escaped=true; } else if byte==b'"' { self.quoted=false; } }
            else {match byte {b'"'=>self.quoted=true,b'{'|b'['=>{self.depth+=1;if self.depth>MAX_JSON_DEPTH{return Err(std::io::Error::other("study.portable.json_depth"));}},b'}'|b']'=>self.depth=self.depth.saturating_sub(1),_=>{}}}
        }
        self.bytes.extend_from_slice(bytes);Ok(bytes.len())
    }
    fn flush(&mut self)->std::io::Result<()>{Ok(())}
}
pub(super) fn store_bounds(store:&super::input_content::Store)->Result<usize,String>{
    store.validate()?;
    if store.references.len()>MAX_OBJECTS{return Err(error("object_count",store.references.len()));}
    let mut total=0usize;
    for r in store.references.values(){
        let len=usize::try_from(r.byte_length).map_err(|_|error("object_size",r.byte_length))?;
        if len>MAX_OBJECT_BYTES{return Err(error("object_size",len));}
        total=total.checked_add(72).and_then(|n|n.checked_add(len)).ok_or_else(||error("aggregate_size","overflow"))?;
        if total>MAX_ARTIFACT_BYTES{return Err(error("aggregate_size",total));}
    }Ok(total)
}
pub(super) fn encode(study:&Study)->Result<Vec<u8>,String>{
    let objects_size=store_bounds(&study.input_contents)?;
    let mut manifest=BoundedWriter{bytes:Vec::new(),depth:0,quoted:false,escaped:false};
    serde_json::to_writer(&mut manifest,study).map_err(|e|error("manifest",e))?;
    json_bounds(&manifest.bytes,MAX_MANIFEST_BYTES)?;
    study.validate()?;
    let total=24usize.checked_add(manifest.bytes.len()).and_then(|n|n.checked_add(objects_size)).ok_or_else(||error("aggregate_size","overflow"))?;
    if total>MAX_ARTIFACT_BYTES{return Err(error("aggregate_size",total));}
    // Verify all recoverable bytes before allocating the container.
    for (hash,r) in &study.input_contents.references {
        let bytes=study.input_contents.resolve(hash)?;
        if bytes.len() as u64!=r.byte_length || blake3::hash(bytes).to_hex().as_str()!=hash.as_str() {return Err(error("hash_identity",hash));}
    }
    let mut out=Vec::with_capacity(total);out.extend_from_slice(MAGIC);out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&(manifest.bytes.len() as u64).to_le_bytes());out.extend_from_slice(&(study.input_contents.references.len() as u32).to_le_bytes());out.extend_from_slice(&manifest.bytes);
    for (hash,r) in &study.input_contents.references {out.extend_from_slice(hash.as_bytes());out.extend_from_slice(&r.byte_length.to_le_bytes());out.extend_from_slice(study.input_contents.resolve(hash)?);}
    Ok(out)
}
struct Cursor<'a>{bytes:&'a [u8],at:usize}
impl<'a> Cursor<'a>{fn take(&mut self,n:usize)->Result<&'a [u8],String>{let end=self.at.checked_add(n).ok_or_else(||error("length","overflow"))?;let value=self.bytes.get(self.at..end).ok_or_else(||error("truncated",self.at))?;self.at=end;Ok(value)}fn u32(&mut self)->Result<u32,String>{Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))}fn u64(&mut self)->Result<u64,String>{Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))}}
pub(super) fn decode(bytes:&[u8])->Result<Study,String>{
    if bytes.len()>MAX_ARTIFACT_BYTES{return Err(error("size",bytes.len()));}
    let mut c=Cursor{bytes,at:0};if c.take(8)?!=MAGIC{return Err(error("magic","SIMSTUDY required"));}
    let version=c.u32()?;if version!=VERSION{return Err(error("version",version));}
    let manifest_len=usize::try_from(c.u64()?).map_err(|_|error("manifest_size","overflow"))?;
    if manifest_len>MAX_MANIFEST_BYTES{return Err(error("manifest_size",manifest_len));}
    let count=c.u32()? as usize;if count>MAX_OBJECTS{return Err(error("object_count",count));}
    let manifest=c.take(manifest_len)?;json_bounds(manifest,MAX_MANIFEST_BYTES)?;
    manifest_preflight(manifest,true)?;
    let mut study:Study=serde_json::from_slice(manifest).map_err(|e|error("manifest",e))?;
    let objects_size=store_bounds(&study.input_contents)?;
    if study.input_contents.references.len()!=count{return Err(error(if count<study.input_contents.references.len(){"missing_object"}else{"membership"},"object count differs from references"));}
    let expected=c.at.checked_add(objects_size).ok_or_else(||error("aggregate_size","overflow"))?;
    if expected>MAX_ARTIFACT_BYTES{return Err(error("aggregate_size",expected));}
    let mut recovered=BTreeMap::new();
    for _ in 0..count {
        let hash=std::str::from_utf8(c.take(64)?).map_err(|e|error("identity",e))?;
        if !hash.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b)){return Err(error("identity",hash));}
        if c.at==bytes.len(){return Err(error("truncated","object length missing"));}
        if recovered.contains_key(hash){return Err(error("duplicate_object",hash));}
        let reference=study.input_contents.references.get(hash).ok_or_else(||error("membership",hash))?;
        let len=c.u64()?;if len!=reference.byte_length{return Err(error("conflicting_object",hash));}
        let len=usize::try_from(len).map_err(|_|error("object_size",len))?;
        if len>MAX_OBJECT_BYTES{return Err(error("object_size",len));}
        let content=c.take(len)?;
        if blake3::hash(content).to_hex().as_str()!=hash{return Err(error("hash_identity",hash));}
        recovered.insert(hash.to_owned(),content);
    }
    if recovered.len()!=study.input_contents.references.len(){return Err(error("missing_object","reference not supplied"));}
    if c.at!=bytes.len(){return Err(error("trailing",bytes.len()-c.at));}
    // No attachment until the complete container has passed framing and hashes.
    study.input_contents.contents=recovered.into_iter().map(|(hash,bytes)|(hash,Arc::new(bytes.to_vec()))).collect();
    Ok(study)
}

/// Serde maps otherwise silently accept duplicate hash/reference declarations.
/// This bounded first pass rejects duplicate keys before the Study is decoded.
pub(super) fn manifest_preflight(bytes:&[u8],reject_duplicates:bool)->Result<(),String>{
    use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
    #[derive(Clone,Copy)] enum Location { Root, Store, References, Reference, Other }
    struct Check(Location,bool);
    impl<'de> DeserializeSeed<'de> for Check {type Value=usize;fn deserialize<D:serde::Deserializer<'de>>(self,d:D)->Result<usize,D::Error>{d.deserialize_any(self)}}
    impl<'de> Visitor<'de> for Check {
        type Value=usize;fn expecting(&self,f:&mut std::fmt::Formatter)->std::fmt::Result{f.write_str("bounded JSON with unique keys")}
        fn visit_map<M:MapAccess<'de>>(self,mut m:M)->Result<usize,M::Error>{
            let mut keys=std::collections::BTreeSet::new();let mut length=0usize;
            while let Some(key)=m.next_key::<String>()? {
                if matches!(self.0,Location::References)&&keys.len()>=MAX_OBJECTS{return Err(serde::de::Error::custom("study.portable.object_count"));}
                if !keys.insert(key.clone())&&self.1{return Err(serde::de::Error::custom("duplicate manifest key"));}
                if matches!(self.0,Location::Reference)&&key=="byte_length" {
                    let n=m.next_value::<u64>()?;
                    if n>MAX_OBJECT_BYTES as u64{return Err(serde::de::Error::custom("study.portable.object_size"));}
                    length=n as usize;
                } else {
                    let location=match (self.0,key.as_str()) {
                        (Location::Root,"input_contents")=>Location::Store,
                        (Location::Store,"references")=>Location::References,
                        (Location::References,_)=>Location::Reference,
                        _=>Location::Other,
                    };
                    let n=m.next_value_seed(Check(location,self.1))?;
                    if matches!(self.0,Location::References) {
                        length=length.checked_add(72).and_then(|v|v.checked_add(n)).ok_or_else(||<M::Error as serde::de::Error>::custom("study.portable.aggregate_size"))?;
                        if length>MAX_ARTIFACT_BYTES{return Err(serde::de::Error::custom("study.portable.aggregate_size"));}
                    }
                }
            }Ok(length)
        }
        fn visit_seq<S:SeqAccess<'de>>(self,mut s:S)->Result<usize,S::Error>{while s.next_element_seed(Check(Location::Other,self.1))?.is_some(){}Ok(0)}
        fn visit_bool<E:serde::de::Error>(self,_:bool)->Result<usize,E>{Ok(0)}
        fn visit_i64<E:serde::de::Error>(self,_:i64)->Result<usize,E>{Ok(0)}
        fn visit_u64<E:serde::de::Error>(self,_:u64)->Result<usize,E>{Ok(0)}
        fn visit_f64<E:serde::de::Error>(self,_:f64)->Result<usize,E>{Ok(0)}
        fn visit_str<E:serde::de::Error>(self,_:&str)->Result<usize,E>{Ok(0)}
        fn visit_string<E:serde::de::Error>(self,_:String)->Result<usize,E>{Ok(0)}
        fn visit_unit<E:serde::de::Error>(self)->Result<usize,E>{Ok(0)}
    }
    let mut d=serde_json::Deserializer::from_slice(bytes);Check(Location::Root,reject_duplicates).deserialize(&mut d).map_err(|e|error("manifest",e))?;d.end().map_err(|e|error("manifest",e))?;
    Ok(())
}

/// Only bounded diagnostic projection. Full decoding remains the attachment gate.
pub(super) fn manifest_slice(bytes:&[u8])->Result<&[u8],String>{
    if bytes.len()>MAX_ARTIFACT_BYTES{return Err(error("size",bytes.len()));}
    let manifest=if bytes.starts_with(MAGIC){
        let mut c=Cursor{bytes,at:8};let version=c.u32()?;
        if version!=VERSION{return Err(error("version",version));}
        let len=usize::try_from(c.u64()?).map_err(|_|error("manifest_size","overflow"))?;
        if len>MAX_MANIFEST_BYTES{return Err(error("manifest_size",len));}
        let count=c.u32()? as usize;if count>MAX_OBJECTS{return Err(error("object_count",count));}
        let manifest=c.take(len)?;json_bounds(manifest,MAX_MANIFEST_BYTES)?;manifest_preflight(manifest,true)?;manifest
    }else{bytes};
    json_bounds(manifest,MAX_MANIFEST_BYTES)?;Ok(manifest)
}
