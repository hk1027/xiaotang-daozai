//! 只读解析汽水 3.7 媒体缓存中的原生歌词；不读取登录配置或音频。
use serde_json::{Value,Map};
use std::collections::{HashMap,HashSet};

fn le(data:&[u8],offset:usize,n:usize)->Option<usize>{
    Some(data.get(offset..offset.checked_add(n)?)?.iter().enumerate().fold(0,|v,(i,b)|v|((*b as usize)<<(8*i))))
}

/// 仅遍历当前已提交快照的主数据库，忽略空闲页及旧事务内容。
fn entries(data:&[u8])->Option<Vec<&[u8]>> {
    if le(data,24,4)?!=0xbeefc0de || le(data,28,4)?!=2{return None;}
    let size=le(data,48,4)?;
    if !size.is_power_of_two() || !(512..=65536).contains(&size){return None;}
    if le(data,size+24,4)?!=0xbeefc0de || le(data,size+28,4)?!=2{return None;}
    let meta=if le(data,152,8)?>le(data,size+152,8)?{0}else{size};
    let mut pending=vec![le(data,meta+136,8)?];let mut visited=HashSet::new();let mut result=Vec::new();
    // 每页最多访问一次：上限按文件实际页数，不再按固定页数（原先 4096 页约 16MB 即放弃）。
    let pages=data.len()/size;
    while let Some(page)=pending.pop(){
        if !visited.insert(page) || visited.len()>pages{return None;}
        let base=page.checked_mul(size)?;let bytes=data.get(base..base.checked_add(size)?)?;
        if le(bytes,0,8)?!=page{return None;}
        let flags=le(bytes,18,2)?;if flags!=1 && flags!=2{return None;}
        let lower=le(bytes,20,2)?;if lower%2!=0 || lower>size-24{return None;}
        for i in 0..lower/2 {
            let node=24+le(bytes,24+i*2,2)?;
            let len=le(bytes,node,4)?;let flags_node=le(bytes,node+4,2)?;let key=le(bytes,node+6,2)?;
            if flags==1 {pending.push(len | (flags_node<<32));continue;}
            if flags_node & 6!=0 {continue;}
            // 歌词记录远小于 2MB；过大的其他记录直接跳过，不因一条大记录放弃整份缓存。
            if len>2*1024*1024{continue;}
            let start=node.checked_add(8+key)?;
            let value=if flags_node==1 {
                let page=le(bytes,start,8)?;let start=page.checked_mul(size)?;
                if le(data,start,8)?!=page || le(data,start+18,2)?!=4{return None;}
                let count=le(data,start+20,4)?;
                if len>count.checked_mul(size)?.checked_sub(24)?{return None;}
                data.get(start+24..(start+24).checked_add(len)?)?
            }else if flags_node==0{bytes.get(start..start.checked_add(len)?)?}else{return None;};
            result.push(value);
        }
    }
    Some(result)
}

// 汽水使用 MessagePack 的内联 record 扩展；记录表仅在单个值内有效。
struct Decoder<'a>{data:&'a[u8],pos:usize,records:HashMap<i32,Vec<String>>,two:HashSet<i32>,budget:usize}
impl<'a> Decoder<'a>{
    fn take(&mut self,n:usize)->Option<&'a[u8]>{let end=self.pos.checked_add(n)?;let b=self.data.get(self.pos..end)?;self.pos=end;Some(b)}
    fn number(&mut self,n:usize)->Option<u64>{Some(self.take(n)?.iter().fold(0,|v,b|(v<<8)|*b as u64))}
    fn string(&mut self,n:usize)->Option<Value>{Some(Value::String(std::str::from_utf8(self.take(n)?).ok()?.into()))}
    fn record(&mut self,id:i32,depth:usize)->Option<Value>{
        let keys=self.records.get(&id)?.clone();let mut map=Map::new();
        for key in keys {map.insert(key,self.value(depth+1)?);}Some(Value::Object(map))
    }
    fn value(&mut self,depth:usize)->Option<Value>{
        if depth>64 || self.budget==0{return None;}self.budget-=1;
        let t=self.number(1)? as u8;
        match t {
            0x40..=0x7f if self.records.contains_key(&((t-0x40) as i32))=>{
                let mut id=(t-0x40) as i32;
                if self.two.contains(&id){let high=self.number(1)? as i32;id=if id<32{-((high<<5)+id)}else{(high<<5)+id};}
                self.record(id,depth)
            },
            0x00..=0x7f=>Some(Value::from(t)),0xe0..=0xff=>Some(Value::from(t as i8)),
            0xc0=>Some(Value::Null),0xc2=>Some(Value::Bool(false)),0xc3=>Some(Value::Bool(true)),
            0xa0..=0xbf=>self.string((t&31) as usize),
            0xd9..=0xdb=>{let n=self.number(1<<(t-0xd9))? as usize;self.string(n)},
            0xcc..=0xcf=>Some(Value::from(self.number(1<<(t-0xcc))?)),
            0xd0..=0xd3=>{let n=1usize<<(t-0xd0);let v=self.number(n)?;Some(Value::from(((v<<(64-n*8)) as i64)>>(64-n*8)))},
            0xca=>{let v=f32::from_bits(self.number(4)? as u32);serde_json::Number::from_f64(v as f64).map(Value::Number)},
            0xcb=>{let v=f64::from_bits(self.number(8)?);serde_json::Number::from_f64(v).map(Value::Number)},
            0x90..=0x9f|0xdc|0xdd=>{
                let n=if t<0xa0{(t&15) as usize}else{self.number(if t==0xdc{2}else{4})? as usize};
                if n>self.budget{return None;}let mut a=Vec::new();for _ in 0..n{a.push(self.value(depth+1)?);}Some(Value::Array(a))
            },
            0x80..=0x8f|0xde|0xdf=>{
                let n=if t<0x90{(t&15) as usize}else{self.number(if t==0xde{2}else{4})? as usize};
                if n>self.budget/2{return None;}let mut map=Map::new();for _ in 0..n{let key=self.value(depth+1)?.as_str()?.to_string();map.insert(key,self.value(depth+1)?);}Some(Value::Object(map))
            },
            0xd4|0xd5=>{
                let kind=self.number(1)?;let first=self.number(1)? as i32;
                if kind==0 && t==0xd4{return Some(Value::Null);}
                if kind!=0x72{return None;}
                let first=first&63;let id=if t==0xd5{let high=self.number(1)? as i32;self.two.insert(first);if first<32{-((high<<5)+first)}else{(high<<5)+first}}else{first};
                let keys=self.value(depth+1)?.as_array()?.iter().map(|v|v.as_str().map(str::to_string)).collect::<Option<Vec<_>>>()?;
                self.records.insert(id,keys);self.record(id,depth)
            },
            _=>None,
        }
    }
}
fn decode(data:&[u8])->Option<Value>{
    let mut d=Decoder{data,pos:0,records:HashMap::new(),two:HashSet::new(),budget:200000};
    let v=d.value(0)?;(d.pos==data.len()).then_some(v)
}

fn select(value:&Value,title:&str,artist:&str,duration:f64)->Option<String>{
    let detail=value.pointer("/info/mediaDetail")?;let track=detail.get("playable")?;
    let identity=super::song_identity(title,artist);
    let artists=track.get("artists")?.as_array()?.iter().map(|v|v.get("name")?.as_str()).collect::<Option<Vec<_>>>()?.join(",");
    if identity.1.is_empty() || super::song_identity(track.get("name")?.as_str()?,&artists)!=identity{return None;}
    let actual=track.get("duration")?.as_f64()?/1000.0;
    if duration>0.0 && (actual-duration).abs()>2.0{return None;}
    let lyric=detail.get("lyrics")?;
    if !matches!(lyric.get("type")?.as_str()?,"krc"|"lrc"){return None;}
    lyric.get("content")?.as_str().filter(|s|!s.is_empty()).map(str::to_string)
}
fn path()->Option<std::path::PathBuf>{Some(std::path::PathBuf::from(std::env::var_os("APPDATA")?).join("SodaMusic/LunaCacheV2/entries.db"))}

/// 缓存文件的修改时间与大小；没变化时轮询不再整份读取（文件最大 32MB）。
pub fn fingerprint()->Option<(std::time::SystemTime,u64)>{
    let meta=std::fs::metadata(path()?).ok()?;
    Some((meta.modified().ok()?,meta.len()))
}

/// 本地读取未命中的原因，写入歌词诊断日志，便于定位「为什么没词」。
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Miss {Missing,TooLarge,Busy,Format,NoMatch}
impl Miss {
    pub fn label(self)->&'static str {
        match self {
            Self::Missing=>"缓存文件不存在",
            Self::TooLarge=>"缓存文件超过上限",
            Self::Busy=>"汽水正在写入",
            Self::Format=>"缓存格式无法识别",
            Self::NoMatch=>"缓存中暂无此歌歌词",
        }
    }
}

/// 只防异常文件：Windows 下按需映射，不再整份复制进内存，正常缓存再大也不受影响。
const LIMIT:u64=1024*1024*1024;

/// 只读打开的缓存内容：Windows 下为内存映射，其余平台整份读入（仅开发用途）。
struct Snapshot {
    #[cfg(windows)] map:memmap2::Mmap,
    #[cfg(not(windows))] data:Vec<u8>,
}
impl Snapshot {
    fn open(file:&std::fs::File)->Option<Self>{
        // SAFETY：只读映射；LMDB 打开期间不会截短数据文件，且有映射时 Windows 拒绝截短；所有偏移都经越界检查。
        #[cfg(windows)] {Some(Self{map:unsafe{memmap2::Mmap::map(file)}.ok()?})}
        #[cfg(not(windows))] {use std::io::Read;let mut data=Vec::new();(&*file).read_to_end(&mut data).ok()?;Some(Self{data})}
    }
    fn bytes(&self)->&[u8]{
        #[cfg(windows)] {&self.map}
        #[cfg(not(windows))] {&self.data}
    }
}

pub fn read(title:&str,artist:&str,duration:f64)->Option<String>{read_detail(title,artist,duration).ok()}

pub fn read_detail(title:&str,artist:&str,duration:f64)->Result<String,Miss>{
    read_detail_at(&path().ok_or(Miss::Missing)?,title,artist,duration)
}

fn read_detail_at(path:&std::path::Path,title:&str,artist:&str,duration:f64)->Result<String,Miss>{
    let file=std::fs::File::open(path).map_err(|_|Miss::Missing)?;
    let len=file.metadata().map_err(|_|Miss::Missing)?.len();
    if len>LIMIT{return Err(Miss::TooLarge);}
    if len==0{return Err(Miss::Format);}
    let snapshot=Snapshot::open(&file).ok_or(Miss::Busy)?;
    let data=snapshot.bytes();
    let size=le(data,48,4).ok_or(Miss::Format)?;if !(512..=65536).contains(&size){return Err(Miss::Format);}
    // 记下两份元页，遍历后再核对：客户端写入期间放弃本次读取，由后续轮询重试。
    let headers=data.get(..size*2).ok_or(Miss::Format)?.to_vec();
    let changed=||data.get(..headers.len())!=Some(&headers[..]);
    let Some(entries)=entries(data) else {return Err(if changed(){Miss::Busy}else{Miss::Format});};
    // 解码是主要开销（约 0.25ms/条）：原始字节里连歌名片段都没有的记录直接跳过。
    let needle=title_needle(title);
    let found=entries.into_iter().filter(|entry|needle.as_ref().is_none_or(|needle|needle.is_match(entry)))
        .find_map(|entry|select(&decode(entry)?,title,artist,duration));
    if changed(){return Err(Miss::Busy);}
    found.ok_or(Miss::NoMatch)
}

/// 歌曲身份的前几个字符在缓存原名中必须依次出现，其间可能有标点、空格或被清理的版本后缀。
fn title_needle(title:&str)->Option<regex::bytes::Regex>{
    let name=super::song_identity(title,"").0;
    let parts:Vec<_>=name.chars().take(5).map(|c|match c {
        // Unicode 小写化会把 İ 展开为 i 和组合点，后者在歌曲身份中被移除。
        'i'=>"[iİ]".to_owned(),
        _=>regex::escape(&c.to_string()),
    }).collect();
    if parts.len()<2 {return None;}
    regex::bytes::RegexBuilder::new(&parts.join("(?s-u:.*?)")).case_insensitive(true).build().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn duet_order_is_not_a_different_recording(){
        let v=serde_json::json!({"info":{"mediaDetail":{"playable":{"name":"合唱","artists":[{"name":"甲"},{"name":"乙"}],"duration":120000},"lyrics":{"type":"krc","content":"[1000,500]<0,500,0>词"}}}});
        assert!(select(&v,"合唱","乙/甲",120.0).is_some());
        for artist in ["甲","甲/丙",""] {assert!(select(&v,"合唱",artist,120.0).is_none());}
    }
    #[test]
    #[ignore="只读验证本机汽水歌词缓存及合唱歌手顺序，不输出歌词正文"]
    fn live_cached_metadata_variants(){
        let path=std::path::PathBuf::from(std::env::var_os("APPDATA").unwrap()).join("SodaMusic/LunaCacheV2/entries.db");
        let data=std::fs::read(path).unwrap();let mut count=0;
        for entry in entries(&data).unwrap() {
            let Some(value)=decode(entry) else{continue;};
            let Some(track)=value.pointer("/info/mediaDetail/playable") else{continue;};
            let Some(title)=track["name"].as_str() else{continue;};
            let Some(artists)=track["artists"].as_array() else{continue;};
            let mut names=artists.iter().filter_map(|a|a["name"].as_str()).collect::<Vec<_>>();
            let duration=track["duration"].as_f64().unwrap_or_default()/1000.0;
            let Some(raw)=select(&value,title,&names.join("/"),duration) else{continue;};
            names.reverse();assert_eq!(select(&value,title,&names.join("、"),duration),Some(raw));count+=1;
            if count==1 {assert!(read(title,&names.join("/"),duration).is_some());}
        }
        assert!(count>0);println!("汽水原生缓存：{count} 条歌词通过元数据及歌手顺序检查");
    }
    #[test] fn featured_credits_match_across_metadata_fields(){
        let v=serde_json::json!({"info":{"mediaDetail":{"playable":{"name":"Track","artists":[{"name":"Artist"},{"name":"Guest"}],"duration":120000},"lyrics":{"type":"krc","content":"[1000,500]<0,500,0>词"}}}});
        assert!(select(&v,"Track (feat. Guest)","Artist",120.0).is_some());
        assert!(select(&v,"Track","Artist featuring Guest",120.0).is_some());
        for (title,artist,duration) in [("Track (Live) feat. Guest","Artist",120.0),("Track feat. Other","Artist",120.0),("Track feat. Guest","Cover",120.0),("Track feat. Guest","Artist",150.0)] {
            assert!(select(&v,title,artist,duration).is_none());
        }
    }
    #[test] fn record_and_invalid_input(){
        let data=b"\xd4r\x40\x91\xa4name\xa4song";
        assert_eq!(decode(data),Some(serde_json::json!({"name":"song"})));
        for end in 0..data.len(){assert!(decode(&data[..end]).is_none());}
        assert!(entries(&[0;4096]).is_none());
    }
    #[test] fn snapshot_follows_branch_and_overflow_and_rejects_corruption(){
        let mut db=vec![0u8;4096*6];
        fn put(db:&mut[u8],p:usize,v:usize,n:usize){for i in 0..n{db[p+i]=(v>>(8*i)) as u8;}}
        for meta in [0,4096]{put(&mut db,meta+24,0xbeefc0de,4);put(&mut db,meta+28,2,4);put(&mut db,meta+48,4096,4);}
        put(&mut db,136,2,8);put(&mut db,152,2,8);
        put(&mut db,4096+136,5,8);put(&mut db,4096+152,1,8);
        for (page,kind) in [(2,1),(3,2),(4,4)]{put(&mut db,page*4096,page,8);put(&mut db,page*4096+18,kind,2);}
        for page in [2,3]{put(&mut db,page*4096+20,2,2);put(&mut db,page*4096+24,100,2);}
        put(&mut db,2*4096+124,3,4);
        let node=3*4096+124;let payload=b"\xd4r\x40\x91\xa4name\xa4song";
        put(&mut db,node,payload.len(),4);put(&mut db,node+4,1,2);put(&mut db,node+8,4,8);
        put(&mut db,4*4096+20,1,4);db[4*4096+24..4*4096+24+payload.len()].copy_from_slice(payload);
        assert_eq!(entries(&db).unwrap(),vec![payload.as_slice()]);
        assert!(entries(&db[..4*4096]).is_none());
        put(&mut db,node,8192,4);assert!(entries(&db).is_none());
        put(&mut db,node,payload.len(),4);put(&mut db,2*4096+124,2,4);assert!(entries(&db).is_none());
    }
    /// 测试用 MessagePack 编码（只覆盖对象、数组、字符串、非负整数）。
    fn pack(value:&serde_json::Value,out:&mut Vec<u8>){
        match value {
            Value::Object(map)=>{out.push(0xde);out.extend_from_slice(&(map.len() as u16).to_be_bytes());for (k,v) in map{pack(&Value::String(k.clone()),out);pack(v,out);}}
            Value::Array(items)=>{out.push(0xdd);out.extend_from_slice(&(items.len() as u32).to_be_bytes());for v in items{pack(v,out);}}
            Value::String(s)=>{out.push(0xdb);out.extend_from_slice(&(s.len() as u32).to_be_bytes());out.extend_from_slice(s.as_bytes());}
            Value::Number(n)=>{out.push(0xcf);out.extend_from_slice(&n.as_u64().unwrap().to_be_bytes());}
            _=>unreachable!(),
        }
    }
    /// 两页元页 + 根叶页（一条溢出记录、一条超过 2MB 的其他记录）+ 溢出页。
    fn fixture(payload:&[u8])->Vec<u8>{
        let mut db=vec![0u8;4096*5];
        fn put(db:&mut[u8],p:usize,v:usize,n:usize){for i in 0..n{db[p+i]=(v>>(8*i)) as u8;}}
        for meta in [0,4096]{put(&mut db,meta+24,0xbeefc0de,4);put(&mut db,meta+28,2,4);put(&mut db,meta+48,4096,4);}
        put(&mut db,136,2,8);put(&mut db,152,2,8);put(&mut db,4096+136,2,8);put(&mut db,4096+152,1,8);
        put(&mut db,2*4096,2,8);put(&mut db,2*4096+18,2,2);put(&mut db,2*4096+20,4,2);
        put(&mut db,2*4096+24,100,2);put(&mut db,2*4096+26,200,2);
        let node=2*4096+124;put(&mut db,node,payload.len(),4);put(&mut db,node+4,1,2);put(&mut db,node+8,3,8);
        put(&mut db,2*4096+224,3*1024*1024,4);
        put(&mut db,3*4096,3,8);put(&mut db,3*4096+18,4,2);put(&mut db,3*4096+20,2,4);
        db[3*4096+24..3*4096+24+payload.len()].copy_from_slice(payload);
        db
    }
    #[test] fn large_cache_is_mapped_and_read_with_reasons_for_misses(){
        let song=serde_json::json!({"info":{"mediaDetail":{"playable":{"name":"歌","artists":[{"name":"歌手"}],"duration":120000},"lyrics":{"type":"krc","content":"[1000,500]<0,500,0>词"}}}});
        let mut payload=Vec::new();pack(&song,&mut payload);
        assert_eq!(decode(&payload),Some(song));
        let db=fixture(&payload);
        assert_eq!(entries(&db).unwrap(),vec![payload.as_slice()],"超过 2MB 的其他记录应跳过，而不是放弃整份缓存");
        let path=std::env::temp_dir().join(format!("daozai-soda-{}-{:?}.db",std::process::id(),std::thread::current().id()));
        std::fs::write(&path,&db).unwrap();
        // 撑到 40MB：超过原先整份读取的 32MB 上限。
        std::fs::OpenOptions::new().write(true).open(&path).unwrap().set_len(40*1024*1024).unwrap();
        assert_eq!(read_detail_at(&path,"歌","歌手",120.0).as_deref(),Ok("[1000,500]<0,500,0>词"));
        assert_eq!(read_detail_at(&path,"歌 (Official Audio)","歌手",120.0).as_deref(),Ok("[1000,500]<0,500,0>词"));
        assert_eq!(read_detail_at(&path,"别的歌","歌手",120.0),Err(Miss::NoMatch));
        std::fs::write(&path,[0u8;8192]).unwrap();
        assert_eq!(read_detail_at(&path,"歌","歌手",120.0),Err(Miss::Format));
        std::fs::remove_file(&path).unwrap();
        assert_eq!(read_detail_at(&path,"歌","歌手",120.0),Err(Miss::Missing));
    }
    #[test]
    fn title_needle_accepts_equivalent_spellings(){
        assert!(title_needle("Go (Official Audio)").unwrap().is_match(b"Go"));
        assert!(title_needle("Sunrise (feat. Guest)").unwrap().is_match(b"Sun-rise"));
        assert!(title_needle("Iz").unwrap().is_match("İ-z".as_bytes()));
        let needle=title_needle("江南-林俊杰(JJ Lin)（Cover 林俊杰）").unwrap();
        assert!(needle.is_match("…江南-林俊杰(JJ Lin)（Cover 林俊杰）…".as_bytes()));
        let needle=title_needle("Fever Pitch").unwrap();
        assert!(needle.is_match(b"name\xdb\x00\x00\x00\x0bFEVER PITCH"),"拉丁字母不区分大小写");
        assert!(!needle.is_match(b"other song"));
        assert!(title_needle("A").is_none() && title_needle("…").is_none(),"太短的片段不预筛");
    }
    #[test]
    #[ignore="性能测量：在 release 下运行，SODA_DB 指定缓存文件（默认本机汽水缓存）；测最坏情况（找不到、遍历到底）的单次读取耗时"]
    fn live_worst_case_read_cost(){
        let path=std::env::var_os("SODA_DB").filter(|path|!path.is_empty()).map(std::path::PathBuf::from)
            .unwrap_or_else(||std::path::PathBuf::from(std::env::var_os("APPDATA").unwrap()).join("SodaMusic/LunaCacheV2/entries.db"));
        let data=std::fs::read(&path).unwrap();
        let records=entries(&data).unwrap().len();
        let rounds=20;let started=std::time::Instant::now();
        for _ in 0..rounds {assert_eq!(read_detail_at(&path,"不存在的歌曲","不存在的歌手",120.0),Err(Miss::NoMatch));}
        let each=started.elapsed()/rounds;
        let mb=data.len() as f64/1024.0/1024.0;
        println!("汽水缓存 {mb:.2} MB、{records} 条记录：最坏情况单次读取 {each:?}，约 {:.1} ms/MB",each.as_secs_f64()*1000.0/mb);
    }
    #[test]
    #[ignore="只读对比本机汽水缓存：整份读取与内存映射（撑大到 40MB 的副本）逐条结果一致，不输出歌词正文"]
    fn live_mapped_read_matches_full_read_on_padded_copy(){
        let source=std::path::PathBuf::from(std::env::var_os("APPDATA").unwrap()).join("SodaMusic/LunaCacheV2/entries.db");
        let data=std::fs::read(&source).unwrap();
        let copy=std::env::temp_dir().join(format!("daozai-soda-live-{}.db",std::process::id()));
        std::fs::write(&copy,&data).unwrap();
        let file=std::fs::OpenOptions::new().write(true).open(&copy).unwrap();
        if file.metadata().unwrap().len()<40*1024*1024 {file.set_len(40*1024*1024).unwrap();}
        drop(file);
        let all=entries(&data).unwrap();let mut count=0;
        for entry in &all {
            let Some(value)=decode(entry) else{continue;};
            let Some(track)=value.pointer("/info/mediaDetail/playable") else{continue;};
            let (Some(title),Some(artists))=(track["name"].as_str(),track["artists"].as_array()) else{continue;};
            let artist=artists.iter().filter_map(|a|a["name"].as_str()).collect::<Vec<_>>().join("/");
            let duration=track["duration"].as_f64().unwrap_or_default()/1000.0;
            if select(&value,title,&artist,duration).is_none(){continue;}
            // 旧读法：整份数据中第一条匹配；新读法：映射撑大的副本。两者应完全相同。
            let old=all.iter().find_map(|e|select(&decode(e)?,title,&artist,duration));
            assert!(old.is_some());
            assert_eq!(read_detail_at(&copy,title,&artist,duration).ok(),old,"第 {count} 首不一致");
            count+=1;
        }
        std::fs::remove_file(&copy).unwrap();
        assert!(count>0);println!("汽水缓存 {} KB：{count} 首歌两种读法结果一致",data.len()/1024);
    }
    #[test] fn rejects_other_versions(){
        let v=serde_json::json!({"info":{"mediaDetail":{"playable":{"name":"歌 (Live)","artists":[{"name":"歌手"}],"duration":120000},"lyrics":{"type":"krc","content":"[1000,500]<0,500,0>词"}}}});
        assert!(select(&v,"歌 (Live)","歌手",120.0).is_some());
        assert!(select(&v,"歌","歌手",120.0).is_none());
        assert!(select(&v,"歌 (Live)","他人",120.0).is_none());
        assert!(select(&v,"歌 (Live)","歌手",150.0).is_none());
    }
}
