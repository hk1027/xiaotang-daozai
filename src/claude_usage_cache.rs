//! 只读 Chromium 桌面用量缓存，文件缺失或不完整时交由历史记录处理。
use super::{Snapshot,Window};
use std::{collections::{HashMap,HashSet},fs::{File,OpenOptions},io::{Read,Seek,SeekFrom},path::{Path,PathBuf}};

const MAX_BODY:usize=1024*1024;
const INDEX_HEADER:usize=368;

fn u32_at(raw:&[u8],at:usize)->Option<u32>{Some(u32::from_le_bytes(raw.get(at..at+4)?.try_into().ok()?))}
fn i64_at(raw:&[u8],at:usize)->Option<i64>{Some(i64::from_le_bytes(raw.get(at..at+8)?.try_into().ok()?))}

struct Cache {path:PathBuf,files:HashMap<PathBuf,File>}
impl Cache {
    fn bytes(&mut self,name:&str,offset:u64,length:usize)->Option<Vec<u8>>{
        if length>4*MAX_BODY{return None;}
        let path=self.path.join(name);
        if !self.files.contains_key(&path){
            let mut options=OpenOptions::new();options.read(true);
            // 与运行中的 Chromium 兼容，只申请读取权限，不阻止它更新或替换文件。
            #[cfg(windows)]{use std::os::windows::fs::OpenOptionsExt;options.share_mode(7);}
            self.files.insert(path.clone(),options.open(&path).ok()?);
        }
        let file=self.files.get_mut(&path)?;file.seek(SeekFrom::Start(offset)).ok()?;
        let mut raw=vec![0;length];file.read_exact(&mut raw).ok()?;Some(raw)
    }
    fn stream(&mut self,address:u32,length:usize)->Option<Vec<u8>>{
        if address&0x80000000==0||length==0||length>MAX_BODY{return None;}
        let kind=(address>>28)&7;
        if kind==0{return self.bytes(&format!("f_{:06x}",address&0x0fffffff),0,length);}
        let size=[0,36,256,1024,4096,8,256,256][kind as usize];
        if length>size*(((address>>24)&3) as usize+1){return None;}
        self.bytes(&format!("data_{}",(address>>16)&255),8192+(address&65535) as u64*size as u64,length)
    }
    fn response(&mut self,address:u32,entry:&[u8],url:&[u8],earliest:i64,now:i64)->Option<Snapshot>{
        if u32_at(entry,20)?!=0{return None;}
        let key_len=u32_at(entry,32)? as usize;if key_len==0||key_len>4096{return None;}
        let long_key=u32_at(entry,36)?;
        let key=if long_key!=0{self.stream(long_key,key_len)?}else{self.stream(address,96+key_len)?.get(96..)?.to_vec()};
        let start=key.windows(8).rposition(|w|w==b"https://")?;
        if key[start..].split(|b|*b==b'?').next()?!=url{return None;}
        if u32_at(&self.stream(u32_at(entry,8)?,36)?,28)?!=0{return None;}
        let metadata_address=u32_at(entry,56)?;let metadata_len=u32_at(entry,40)? as usize;
        let metadata=self.stream(metadata_address,metadata_len)?;
        let (at,headers)=response_info(&metadata)?;
        if at<earliest-1||at<now-7200||at>now+60{return None;}
        let body_address=u32_at(entry,60)?;let body_len=u32_at(entry,44)? as usize;
        let body=self.stream(body_address,body_len)?;
        let snapshot=parse_body(&body,headers,at,now)?;
        // 写入中的条目直接跳过，不锁定、不复制或修复 Claude 的缓存。
        if self.stream(address,96)?!=entry[..96]||self.stream(metadata_address,metadata_len)?!=metadata||self.stream(body_address,body_len)?!=body{return None;}
        Some(snapshot)
    }
}

fn response_info(raw:&[u8])->Option<(i64,&[u8])>{
    if u32_at(raw,0)? as usize!=raw.len().checked_sub(4)?{return None;}
    let flags=u32_at(raw,4)?;if flags&255!=3||flags&(1<<12)!=0{return None;}
    let extra=if flags&(1<<31)!=0{u32_at(raw,8)?}else{0};
    let mut offset=if flags&(1<<31)!=0{12}else{8};
    let at=i64_at(raw,offset+8)?/1_000_000-11644473600;
    offset+=16+if extra&4!=0{8}else{0};
    let length=u32_at(raw,offset)? as usize;let headers=raw.get(offset+4..offset+4+length)?;
    let status=headers.split(|b|*b==0||*b==b'\r'||*b==b'\n').next()?;
    if status.split(|b|*b==b' ').nth(1)!=Some(b"200".as_slice())||!headers.windows(16).any(|w|w.eq_ignore_ascii_case(b"application/json")){return None;}
    Some((at,headers))
}

#[derive(Default,serde::Deserialize)]
#[serde(default)]
struct Limit {utilization:Option<f64>,used_percentage:Option<f64>,percent:Option<f64>,resets_at:serde_json::Value,kind:Option<String>}
#[derive(serde::Deserialize)]
struct Usage {five_hour:Option<Limit>,seven_day:Option<Limit>,#[serde(default)]limits:Vec<Limit>}

fn parse_body(raw:&[u8],headers:&[u8],at:i64,now:i64)->Option<Snapshot>{
    let mut decoded=Vec::new();
    if raw.starts_with(&[0x28,0xb5,0x2f,0xfd]){
        let mut decoder=zstd::stream::read::Decoder::new(raw).ok()?;decoder.window_log_max(23).ok()?;
        decoder.take((MAX_BODY+1) as u64).read_to_end(&mut decoded).ok()?;
    }else if raw.starts_with(&[0x1f,0x8b]){
        flate2::read::GzDecoder::new(raw).take((MAX_BODY+1) as u64).read_to_end(&mut decoded).ok()?;
    }else if headers.windows(25).any(|w|w.eq_ignore_ascii_case(b"content-encoding: deflate")){
        flate2::read::ZlibDecoder::new(raw).take((MAX_BODY+1) as u64).read_to_end(&mut decoded).ok()?;
    }else{decoded.extend_from_slice(raw);}
    if decoded.len()>MAX_BODY{return None;}
    let data:Usage=serde_json::from_slice(&decoded).ok()?;let mut snapshot=Snapshot{at,..Default::default()};
    for (window,kind,label,seconds) in [(data.five_hour,"session","5 小时",18000),(data.seven_day,"weekly_all","本周",604800)]{
        let Some(window)=window.as_ref().or_else(||data.limits.iter().find(|w|w.kind.as_deref()==Some(kind)))else{continue;};
        let Some(used)=window.utilization.or(window.used_percentage).or(window.percent).filter(|n|n.is_finite()&&(0.0..=100.0).contains(n))else{continue;};
        let reset=window.resets_at.as_str().and_then(|s|chrono::DateTime::parse_from_rfc3339(s).ok()).map(|t|t.timestamp()).filter(|t|*t>now&&*t>at&&*t<=at+seconds+60);
        snapshot.windows.push(Window{label:label.into(),seconds:Some(seconds),remaining:(100.0-used) as f32,reset,amount:None});
    }
    (!snapshot.windows.is_empty()).then_some(snapshot)
}

pub(super) fn read(path:&Path,org:&str,at:i64,now:i64)->Option<Snapshot>{
    let mut cache=Cache{path:path.into(),files:HashMap::new()};
    let index_len=std::fs::metadata(path.join("index")).ok()?.len().try_into().ok()?;
    let index=cache.bytes("index",0,index_len)?;
    if u32_at(&index,0)?!=0xc103cac3||!matches!(u32_at(&index,4)?,0x20000|0x20001|0x30000){return None;}
    let table_len=match u32_at(&index,28)?{0=>65536,n=>n as usize};
    if !table_len.is_power_of_two()||table_len>(4*MAX_BODY-INDEX_HEADER)/4||index.len()!=INDEX_HEADER+4*table_len{return None;}
    let url=format!("https://claude.ai/api/organizations/{org}/usage");let mut seen=HashSet::new();let mut latest:Option<Snapshot>=None;
    for (bucket,head) in index[INDEX_HEADER..].chunks_exact(4).enumerate(){
        let mut address=u32_at(head,0)?;
        while address!=0&&seen.len()<40000&&seen.insert(address){
            let Some(entry)=cache.stream(address,256)else{break;};let current=address;address=u32_at(&entry,4)?;
            if (u32_at(&entry,0)? as usize)&(table_len-1)!=bucket{break;}
            if let Some(snapshot)=cache.response(current,&entry,url.as_bytes(),at,now){
                if cache.bytes("index",(INDEX_HEADER+bucket*4) as u64,4)?.as_slice()!=head{break;}
                if latest.as_ref().is_none_or(|s|snapshot.at>s.at){latest=Some(snapshot);}
            }
        }
    }
    latest
}

#[cfg(test)]
pub(super) fn fixture(path:&Path,org:&str,at:i64,body:&[u8]){
    use std::fs;
    fs::create_dir_all(path).unwrap();
    let mut index=vec![0u8;368+16*4];
    index[..4].copy_from_slice(&0xc103cac3u32.to_le_bytes());index[4..8].copy_from_slice(&0x30000u32.to_le_bytes());index[28..32].copy_from_slice(&16i32.to_le_bytes());index[372..376].copy_from_slice(&0xa0010000u32.to_le_bytes());
    fs::write(path.join("index"),index).unwrap();
    let headers=b"HTTP/1.1 200 OK\0content-type: application/json\0\0";
    let mut metadata=vec![0u8;28];metadata[4..8].copy_from_slice(&3u32.to_le_bytes());
    let time=((at+11644473600)*1_000_000).to_le_bytes();metadata[8..16].copy_from_slice(&time);metadata[16..24].copy_from_slice(&time);metadata[24..28].copy_from_slice(&(headers.len() as u32).to_le_bytes());metadata.extend_from_slice(headers);
    while metadata.len()%4!=0{metadata.push(0);}
    let size=(metadata.len()-4) as u32;metadata[..4].copy_from_slice(&size.to_le_bytes());
    let mut entries=vec![0u8;8192+256];let entry=&mut entries[8192..];
    let key=format!("1/0/https://claude.ai/api/organizations/{org}/usage");
    entry[..4].copy_from_slice(&1u32.to_le_bytes());entry[8..12].copy_from_slice(&0x90000000u32.to_le_bytes());entry[32..36].copy_from_slice(&(key.len() as u32).to_le_bytes());entry[40..44].copy_from_slice(&(metadata.len() as i32).to_le_bytes());entry[44..48].copy_from_slice(&(body.len() as i32).to_le_bytes());entry[56..60].copy_from_slice(&0xb0020000u32.to_le_bytes());entry[60..64].copy_from_slice(&0x80000001u32.to_le_bytes());entry[96..96+key.len()].copy_from_slice(key.as_bytes());
    fs::write(path.join("data_0"),vec![0u8;8192+36]).unwrap();fs::write(path.join("data_1"),entries).unwrap();
    let mut data=vec![0u8;8192+1024];data[8192..8192+metadata.len()].copy_from_slice(&metadata);fs::write(path.join("data_2"),data).unwrap();fs::write(path.join("f_000001"),body).unwrap();
}

#[cfg(test)]
mod tests{
    use super::*;
    use serde_json::json;
    fn dir()->std::path::PathBuf{let path=std::env::temp_dir().join(format!("daozai-usage-cache-{}",std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));std::fs::create_dir_all(&path).unwrap();path}
    fn body(at:i64)->Vec<u8>{serde_json::to_vec(&json!({"five_hour":{"utilization":24,"resets_at":chrono::DateTime::from_timestamp(at+3600,0).unwrap().to_rfc3339()},"seven_day":{"utilization":84,"resets_at":chrono::DateTime::from_timestamp(at+86400,0).unwrap().to_rfc3339()}})).unwrap()}
    #[test]
    fn desktop_cache_reads_official_reset_times_without_changing_files(){
        let path=dir();let now=chrono::Utc::now().timestamp();fixture(&path,"local-org",now-10,&body(now-10));
        let files=std::fs::read_dir(&path).unwrap().map(|e|{let path=e.unwrap().path();let bytes=std::fs::read(&path).unwrap();(path,bytes)}).collect::<Vec<_>>();
        let snapshot=read(&path,"local-org",now-20,now).expect("应读取已有缓存中的额度和重置时间");assert_eq!(snapshot.at,now-10);assert_eq!(snapshot.windows.iter().map(|w|w.remaining).collect::<Vec<_>>(),vec![76.0,16.0]);assert_eq!(snapshot.windows[0].reset,Some(now-10+3600));assert_eq!(snapshot.windows[1].reset,Some(now-10+86400));
        for(path,bytes)in files{assert_eq!(std::fs::read(path).unwrap(),bytes);}let _=std::fs::remove_dir_all(path);
    }
    #[test]
    fn desktop_cache_rejects_other_organizations_and_old_or_future_responses(){
        let path=dir();let now=chrono::Utc::now().timestamp();fixture(&path,"other-org",now,&body(now));assert!(read(&path,"local-org",now-20,now).is_none());
        for at in [now-7201,now-120,now+61]{fixture(&path,"local-org",at,&body(at));assert!(read(&path,"local-org",now-20,now).is_none());}let _=std::fs::remove_dir_all(path);
    }
    #[test]
    fn desktop_cache_keeps_missing_expired_and_unzoned_resets_unknown(){
        let path=dir();let now=chrono::Utc::now().timestamp();
        for reset in [serde_json::Value::Null,json!("not-a-date"),json!("2030-01-01T12:00:00"),json!(chrono::DateTime::from_timestamp(now-1,0).unwrap().to_rfc3339()),json!(chrono::DateTime::from_timestamp(now+7*86400,0).unwrap().to_rfc3339())]{
            fixture(&path,"local-org",now-10,&serde_json::to_vec(&json!({"five_hour":{"utilization":24,"resets_at":reset}})).unwrap());let snapshot=read(&path,"local-org",now-20,now).expect("缺少有效重置时间不能影响额度");assert_eq!(snapshot.windows[0].remaining,76.0);assert_eq!(snapshot.windows[0].reset,None);
        }let _=std::fs::remove_dir_all(path);
    }
    #[test]
    fn desktop_cache_decodes_compressed_responses_and_rejects_damage_or_excessive_output(){
        let path=dir();let now=chrono::Utc::now().timestamp();let raw=body(now);
        let compressed=zstd::stream::encode_all(raw.as_slice(),1).unwrap();fixture(&path,"local-org",now,&compressed);assert!(read(&path,"local-org",now,now).unwrap().windows.iter().all(|w|w.reset.is_some()));
        let mut gzip=flate2::write::GzEncoder::new(Vec::new(),flate2::Compression::fast());std::io::Write::write_all(&mut gzip,&raw).unwrap();fixture(&path,"local-org",now,&gzip.finish().unwrap());assert!(read(&path,"local-org",now,now).is_some());
        for bytes in [compressed[..compressed.len()-2].to_vec(),b"invalid-json".to_vec(),zstd::stream::encode_all(vec![b' ';MAX_BODY+1].as_slice(),1).unwrap()]{fixture(&path,"local-org",now,&bytes);assert!(read(&path,"local-org",now,now).is_none());}
        std::fs::write(path.join("index"),[0u8;32]).unwrap();assert!(read(&path,"local-org",now,now).is_none());let _=std::fs::remove_dir_all(path);
    }
    #[test]
    fn desktop_cache_supports_limit_rows_and_never_infers_missing_windows(){
        let path=dir();let now=chrono::Utc::now().timestamp();
        let data=json!({"limits":[{"kind":"weekly_all","percent":84,"is_active":false,"resets_at":chrono::DateTime::from_timestamp(now+86400,0).unwrap().to_rfc3339()}],"private-field":"not-retained"});
        fixture(&path,"local-org",now,&serde_json::to_vec(&data).unwrap());let snapshot=read(&path,"local-org",now,now).unwrap();assert_eq!(snapshot.windows.len(),1);assert_eq!(snapshot.windows[0].label,"本周");assert_eq!(snapshot.windows[0].remaining,16.0);assert_eq!(snapshot.windows[0].reset,Some(now+86400));let _=std::fs::remove_dir_all(path);
    }
    #[cfg(windows)]
    #[test]
    fn desktop_cache_reads_files_held_open_with_delete_access(){
        use std::os::windows::fs::OpenOptionsExt;
        let path=dir();let now=chrono::Utc::now().timestamp();fixture(&path,"local-org",now,&body(now));
        let held=OpenOptions::new().read(true).access_mode(0x80010000).share_mode(7).open(path.join("index")).unwrap();
        assert!(read(&path,"local-org",now,now).is_some());drop(held);let _=std::fs::remove_dir_all(path);
    }
}
