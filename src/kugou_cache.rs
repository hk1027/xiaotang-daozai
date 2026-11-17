//! 只读酷狗本机 KRC，使用客户端原生时间戳，避免跨来源歌词偏移。
use std::{fs,io::Read,path::{Path,PathBuf}};
use super::{Lyrics,normalized,parse_krc,romanized,song_identity};

const LIMIT:usize=2*1024*1024;
// KRC 的固定替换表与 zlib 格式：https://gist.github.com/zengkid/68c455a860f29f215036be19171f0d6f
const KEY:[u8;16]=[0x40,0x47,0x61,0x77,0x5e,0x32,0x74,0x47,0x51,0x36,0x31,0x2d,0xce,0xd2,0x6e,0x69];

fn decode(bytes:&[u8])->Option<String>{
    if bytes.len()>LIMIT{return None;}
    let bytes=bytes.strip_prefix(b"krc1")?.iter().enumerate().map(|(i,b)|b^KEY[i%KEY.len()]).collect::<Vec<_>>();
    let mut raw=String::new();
    flate2::read::ZlibDecoder::new(bytes.as_slice()).take(LIMIT as u64+1).read_to_string(&mut raw).ok()?;
    (raw.len()<=LIMIT).then_some(raw)
}

fn header<'a>(raw:&'a str,key:&str)->Option<&'a str>{
    let prefix=format!("[{key}:");
    raw.lines().find_map(|line|line.trim_start_matches('\u{feff}').strip_prefix(&prefix)?.strip_suffix(']')).map(str::trim)
}

/// 去掉标题末尾括号里的译名或别名：「ANGEL (天使)」→「ANGEL」。
/// 括号里是版本信息（Live、Remix、伴奏、现场等）时保留，避免把其他版本的歌词当成原版。
fn without_alias(title:&str)->&str {
    static ALIAS:std::sync::OnceLock<regex::Regex>=std::sync::OnceLock::new();
    static VERSION:std::sync::OnceLock<regex::Regex>=std::sync::OnceLock::new();
    let alias=ALIAS.get_or_init(||regex::Regex::new(r"\s*[(（\[【]([^()（）\[\]【】]*)[)）\]】]\s*$").unwrap());
    let version=VERSION.get_or_init(||regex::Regex::new(r"(?i)live|remix|mix|ver\b|version|edit|acoustic|inst|伴奏|现场|混音|版|demo|cover|翻唱|remaster|演唱会|\bdj\b").unwrap());
    match alias.captures(title) {
        Some(c) if !c[1].trim().is_empty() && !version.is_match(&c[1])=>title[..c.get(0).unwrap().start()].trim_end(),
        _=>title,
    }
}

fn select(raw:&str,title:&str,artist:&str,duration:f64)->Option<Lyrics>{
    let mut lyrics=parse_krc(raw);
    if !lyrics.synced{return None;}
    // 文件名已经严格匹配完整歌手与歌名；头部可能为空，合唱也可能只标主歌手。
    if header(raw,"ti").filter(|s|!s.is_empty()).is_some_and(|s|song_identity(s,artist).0!=song_identity(title,artist).0
        && song_identity(without_alias(s),artist).0!=song_identity(without_alias(title),artist).0) {
        // 酷狗头部可能保留旧别名；仅在正文开头的完整歌名和歌手也吻合时接受。
        let credit=normalized(&format!("{title} - {artist}"));
        if !lyrics.lines.first().is_some_and(|line|line.t<=1.0 && normalized(&line.text)==credit){return None;}
    }
    if let Some(singer)=header(raw,"ar").filter(|s|!s.is_empty()) {
        let expected=song_identity(title,artist).1;
        let singers=song_identity(header(raw,"ti").unwrap_or(title),singer).1;
        if singers.is_empty() || !singers.iter().all(|s|expected.contains(s)){return None;}
    }
    if duration>0.0{
        if let Some(total)=header(raw,"total").and_then(|s|s.parse::<u32>().ok()){
            if total>0 && (total as f64/1000.0-duration).abs()>2.0{return None;}
        }
    }
    // 文件自身的校准量属于原生时间轴，用户的提前量仍由界面单独应用。
    let offset=header(raw,"offset").and_then(|s|s.parse::<i32>().ok()).unwrap_or_default() as f32/1000.0;
    for line in &mut lyrics.lines{line.t=(line.t+offset).max(0.0);}
    Some(lyrics)
}

fn directory_from_ini(bytes:&[u8])->Option<PathBuf>{
    let text=if let Some(bytes)=bytes.strip_prefix(&[0xff,0xfe]){
        String::from_utf16(&bytes.chunks_exact(2).map(|b|u16::from_le_bytes([b[0],b[1]])).collect::<Vec<_>>()).ok()?
    }else{String::from_utf8(bytes.to_vec()).ok()?};
    text.lines().find_map(|line|{
        let (key,value)=line.split_once('=')?;
        (key.trim().eq_ignore_ascii_case("LyricPath") && !value.trim().is_empty()).then(||PathBuf::from(value.trim()))
    })
}

fn read_directory(root:&Path,title:&str,artist:&str,duration:f64)->Option<Lyrics>{
    let identity=song_identity(title,artist);
    if identity.1.is_empty(){return None;}
    static SUFFIX:std::sync::OnceLock<regex::Regex>=std::sync::OnceLock::new();
    let suffix=SUFFIX.get_or_init(||regex::Regex::new(r"(?i)-[0-9a-f]{32}(?:-[0-9a-f]+){0,2}$").unwrap());
    let mut files=fs::read_dir(root).ok()?.filter_map(Result::ok).filter_map(|entry|{
        let path=entry.path();
        if !path.extension()?.eq_ignore_ascii_case("krc"){return None;}
        let filename=suffix.replace(path.file_stem()?.to_str()?,"");
        let (singer,name)=filename.split_once(" - ")?;
        if song_identity(name,singer)!=identity{return None;}
        let meta=entry.metadata().ok()?;
        (meta.is_file() && meta.len()<=LIMIT as u64).then_some((meta.modified().ok(),name.to_owned(),singer.to_owned(),path))
    }).collect::<Vec<_>>();
    files.sort_by(|a,b|b.0.cmp(&a.0));
    let mut fallback=None;
    let mut transliterated=None;
    for (_,name,singer,path) in files{
        let raw=(||{let mut bytes=Vec::new();fs::File::open(path).ok()?.take(LIMIT as u64+1).read_to_end(&mut bytes).ok()?;decode(&bytes)})();
        let Some(raw)=raw else{continue;};
        let Some(candidate)=select(&raw,&name,&singer,duration) else{continue;};
        // 同一首歌可能缓存过多份歌词：正文是罗马音的那份只作备选，优先用原文。
        if romanized(title,&candidate) {if transliterated.is_none(){transliterated=Some(candidate);}continue;}
        // 时长明确匹配的缓存优先；没有时长信息时才沿用最新的兼容缓存。
        if duration<=0.0 || header(&raw,"total").and_then(|s|s.parse::<u32>().ok()).is_some_and(|total|total>0){return Some(candidate);}
        if fallback.is_none(){fallback=Some(candidate);}
    }
    fallback.or(transliterated)
}

pub fn read(title:&str,artist:&str,duration:f64)->Option<Lyrics>{
    if title.trim().is_empty() || artist.trim().is_empty(){return None;}
    let ini=PathBuf::from(std::env::var_os("APPDATA")?).join("KuGou8/KuGou.ini");
    let root=fs::File::open(ini).ok().and_then(|file|{
        let mut bytes=Vec::new();file.take(65537).read_to_end(&mut bytes).ok()?;
        if bytes.len()>65536{return None;}
        directory_from_ini(&bytes)
    }).unwrap_or_else(||PathBuf::from(format!("{}\\KuGou\\Lyric",std::env::var("SystemDrive").unwrap_or_else(|_|"C:".into()))));
    read_directory(&root,title,artist,duration)
}

#[cfg(test)]
mod tests{
    #[test]
    fn translated_alias_in_the_lyric_header_still_matches_but_versions_do_not(){
        // 酷狗实际缓存：文件名为「尹美莱、Tiger JK、Bizzy - Angel」，头部标题带中文译名。
        let krc=|ti:&str|format!("[ar:尹美莱/Tiger JK/Bizzy]\n[ti:{ti}]\n[total:0]\n[offset:0]\n[819,3016]<0,328,0>ANGEL (天使) - 尹美莱/Tiger JK/Bizzy\n[15670,1288]<0,205,0>You will always be my\n[20000,1500]<0,300,0>넌 나의 ANGEL");
        assert!(select(&krc("ANGEL (天使)"),"Angel","尹美莱、Tiger JK、Bizzy",255.0).is_some(),"只多了译名的同一首歌应能读取");
        assert!(select(&krc("ANGEL"),"Angel","尹美莱、Tiger JK、Bizzy",255.0).is_some());
        for other in ["ANGEL (Live)","ANGEL (Remix)","ANGEL (伴奏)","ANGEL (现场版)","Devil (天使)"] {
            assert!(select(&krc(other),"Angel","尹美莱、Tiger JK、Bizzy",255.0).is_none(),"{other} 不能当成原版");
        }
        assert_eq!(without_alias("ANGEL (天使)"),"ANGEL");
        assert_eq!(without_alias("Track (Live)"),"Track (Live)");
        assert_eq!(without_alias("Track ()"),"Track ()");
    }

    use super::*;
    #[test]
    fn known_duration_cache_precedes_newer_unknown_duration(){
        let dir=std::env::temp_dir().join(format!("daozai-krc-duration-rank-{}",std::process::id()));fs::create_dir_all(&dir).unwrap();
        let known=dir.join("Artist - Track-0123456789abcdef0123456789abcdef.krc");
        let unknown=dir.join("Artist - Track-1123456789abcdef0123456789abcdef.krc");
        fs::write(&known,fixture("[ti:Track]\n[ar:Artist]\n[total:120000]\n[1000,500]已核对时长")).unwrap();
        fs::write(&unknown,fixture("[ti:Track]\n[ar:Artist]\n[total:0]\n[2000,500]缺少时长")).unwrap();
        let file=fs::File::options().write(true).open(&unknown).unwrap();
        file.set_times(fs::FileTimes::new().set_modified(std::time::SystemTime::now()+std::time::Duration::from_secs(60))).unwrap();drop(file);
        assert_eq!(read_directory(&dir,"Track","Artist",120.0).unwrap().lines[0].text,"已核对时长");
        assert_eq!(read_directory(&dir,"Track","Artist",0.0).unwrap().lines[0].text,"缺少时长");
        fs::remove_file(known).unwrap();fs::remove_file(unknown).unwrap();fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn cached_duet_identity_ignores_order_and_credit_field(){
        let dir=std::env::temp_dir().join(format!("daozai-krc-featured-identity-{}",std::process::id()));fs::create_dir_all(&dir).unwrap();
        let path=dir.join("Guest、Artist - Track-0123456789abcdef0123456789abcdef.krc");
        fs::write(&path,fixture("[ti:Track]\n[ar:Artist、Guest]\n[total:120000]\n[1000,500]第一句")).unwrap();
        for (title,artist) in [("Track","Artist/Guest"),("Track feat. Guest","Artist"),("Track","Artist featuring Guest")] {
            assert!(read_directory(&dir,title,artist,120.0).is_some());
        }
        for (title,artist,duration) in [("Track (Live)","Artist/Guest",120.0),("Track","Artist/Other",120.0),("Track","Artist",120.0),("Track","Artist/Guest",150.0)] {
            assert!(read_directory(&dir,title,artist,duration).is_none());
        }
        fs::remove_file(path).unwrap();fs::remove_dir(dir).unwrap();
    }
    #[test]
    #[ignore="只读验证本机已缓存的合唱及空头部歌词"]
    fn live_incomplete_krc_headers() {
        for (title,artist) in [("让我取暖","王力宏、彭羚"),("爱情买卖","慕容晓晓")] {
            let lyrics=read(title,artist,0.0).expect("本机已缓存的歌词应正常识别");
            assert!(lyrics.synced && lyrics.lines.len()>10);
            println!("{title}：原生歌词 {} 行",lyrics.lines.len());
        }
    }
    fn fixture(raw:&str)->Vec<u8>{
        use std::io::Write;
        let mut encoder=flate2::write::ZlibEncoder::new(Vec::new(),flate2::Compression::default());encoder.write_all(raw.as_bytes()).unwrap();
        let mut bytes=b"krc1".to_vec();bytes.extend(encoder.finish().unwrap().iter().enumerate().map(|(i,b)|b^KEY[i%16]));bytes
    }
    #[test]
    fn alias_header_requires_exact_filename_and_opening_credit(){
        let dir=std::env::temp_dir().join(format!("daozai-krc-alias-{}",std::process::id()));fs::create_dir_all(&dir).unwrap();
        let path=dir.join("歌手 - 测试歌 (副标题)-0123456789abcdef0123456789abcdef.krc");
        let raw="[ti:测试歌 (女生版)]\n[ar:歌手]\n[total:232000]\n[0,70]<0,35,0>测试歌 (副标题)<35,35,0> - 歌手\n[4000,2000]<0,2000,0>第一句";
        fs::write(&path,fixture(raw)).unwrap();
        assert!(read_directory(&dir,"测试歌 (副标题)","歌手",232.0).is_some(),"文件名和开头署名完全一致时，应接受头部别名");
        assert!(read_directory(&dir,"测试歌 (Live)","歌手",232.0).is_none());
        assert!(read_directory(&dir,"测试歌 (副标题)","其他歌手",232.0).is_none());
        assert!(read_directory(&dir,"测试歌 (副标题)","歌手",260.0).is_none());
        for wrong in [raw.replace("<0,35,0>测试歌 (副标题)","<0,35,0>测试歌 (Live)"),raw.replace(" - 歌手"," - 其他歌手")] {
            fs::write(&path,fixture(&wrong)).unwrap();
            assert!(read_directory(&dir,"测试歌 (副标题)","歌手",232.0).is_none());
        }
        fs::remove_file(path).unwrap();fs::remove_dir(dir).unwrap();
    }
    #[test]
    #[ignore="验证本机原歌曲的真实 KRC 别名与原生时间轴"]
    fn live_kugou_alias_lyrics(){
        let lyrics=read("想你就写信 (你说想哭就弹琴)","cici_",232.0).expect("必须读取本机原生歌词");
        assert!(lyrics.synced && lyrics.lines.len()>20);
        for time in [5.0,10.0,15.0] {
            let line=&lyrics.lines[lyrics.index_at(time).unwrap()];
            assert!(!line.text.is_empty());println!("{time}秒：{}（原生时间 {}）",line.text,line.t);
        }
    }
    #[test]
    fn hexadecimal_cache_suffix_is_not_part_of_the_song_title(){
        let dir=std::env::temp_dir().join(format!("daozai-krc-hex-{}",std::process::id()));fs::create_dir_all(&dir).unwrap();
        let path=dir.join("庄心妍 - 以后的以后-09832f40c5944aa9d3d06853213b59be-192001361-000002DA.krc");
        fs::write(&path,fixture("[ti:以后的以后]\n[ar:庄心妍]\n[1000,2000]第一句")).unwrap();
        let result=read_directory(&dir,"以后的以后","庄心妍",0.0);
        fs::remove_file(path).unwrap();fs::remove_dir(dir).unwrap();
        assert!(result.is_some(),"酷狗文件末段包含十六进制字母时仍应识别歌词");
    }

    #[test]
    fn native_timing_versions_offsets_and_corrupt_files(){
        let raw="[ti:测试歌]\n[ar:歌手]\n[total:120000]\n[offset:25]\n[22991,2000]<0,1000,0>第一句\n[27503,1500]<0,1500,0>第二句";
        assert_eq!(decode(&fixture(raw)).as_deref(),Some(raw));
        let lyrics=select(raw,"测试歌","歌手",120.0).unwrap();
        assert!((lyrics.lines[0].t-23.016).abs()<0.001);assert_eq!(lyrics.lines[0].text,"第一句");
        assert_eq!(lyrics.lines[lyrics.index_at(25.016).unwrap()].text,"第一句");
        assert!(lyrics.lines.iter().any(|line|line.text.is_empty() && (line.t-29.028).abs()<0.001));
        assert!(select(raw,"测试歌 (Live)","歌手",120.0).is_none());assert!(select(raw,"测试歌","其他歌手",120.0).is_none());assert!(select(raw,"测试歌","歌手",150.0).is_none());
        assert!(select(&raw.replace("[total:120000]","[total:0]"),"测试歌","歌手",120.0).is_some());
        for bytes in [b"".as_slice(),b"krc1",b"krc1broken",b"lrc1invalid"]{assert!(decode(bytes).is_none());}
        assert!(decode(&fixture(&"x".repeat(LIMIT+1))).is_none());
    }
    #[test]
    fn configured_unicode_directory_and_exact_track_selection(){
        let ini="[Download]\r\nLyricPath=D:\\音乐\\歌词\\\r\n";
        let mut bytes=vec![0xff,0xfe];bytes.extend(ini.encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(directory_from_ini(&bytes),Some(PathBuf::from("D:\\音乐\\歌词\\")));
        assert_eq!(directory_from_ini(ini.as_bytes()),directory_from_ini(&bytes));
        let dir=std::env::temp_dir().join(format!("daozai-krc-{}",std::process::id()));fs::create_dir_all(&dir).unwrap();
        let good=dir.join("歌手 - 测试歌-0123456789abcdef0123456789abcdef-1-00000000.krc");let wrong=dir.join("歌手 - 测试歌 (Live)-0123456789abcdef0123456789abcdef-2-00000000.krc");
        fs::write(&good,fixture("[ti:测试歌]\n[ar:歌手]\n[22991,2000]<0,2000,0>第一句")).unwrap();
        fs::write(&wrong,fixture("[ti:测试歌 (Live)]\n[ar:歌手]\n[23930,2000]<0,2000,0>第一句")).unwrap();
        assert!((read_directory(&dir,"测试歌","歌手",0.0).unwrap().lines[0].t-22.991).abs()<0.001);
        assert!(read_directory(&dir,"其他歌","歌手",0.0).is_none());
        fs::remove_file(good).unwrap();fs::remove_file(wrong).unwrap();fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn duet_cache_accepts_primary_artist_only_when_filename_matches_full_track() {
        let dir=std::env::temp_dir().join(format!("daozai-krc-duet-{}",std::process::id()));fs::create_dir_all(&dir).unwrap();
        let path=dir.join("歌手甲、歌手乙 - 合唱曲-0123456789abcdef0123456789abcdef.krc");
        fs::write(&path,fixture("[ti:合唱曲]\n[ar:歌手甲]\n[1000,2000]第一句")).unwrap();
        assert!(read_directory(&dir,"合唱曲","歌手甲、歌手乙",0.0).is_some(),"合唱歌词的头部可能只记录主歌手");
        assert!(read_directory(&dir,"合唱曲","歌手甲、歌手丙",0.0).is_none());
        assert!(read_directory(&dir,"合唱曲 (Live)","歌手甲、歌手乙",0.0).is_none());
        fs::remove_file(path).unwrap();fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn empty_headers_use_exact_filename_without_accepting_other_versions() {
        let dir=std::env::temp_dir().join(format!("daozai-krc-empty-{}",std::process::id()));fs::create_dir_all(&dir).unwrap();
        let path=dir.join("歌手 - 测试歌-0123456789abcdef0123456789abcdef-123-00000000.krc");
        fs::write(&path,fixture("[ti:]\n[ar:]\n[total:0]\n[1000,2000]第一句")).unwrap();
        assert!(read_directory(&dir,"测试歌","歌手",0.0).is_some());
        for title in ["测试","测试歌 (Live)"] {assert!(read_directory(&dir,title,"歌手",0.0).is_none());}
        assert!(read_directory(&dir,"测试歌","别的歌手",0.0).is_none());
        fs::remove_file(path).unwrap();fs::remove_dir(dir).unwrap();
    }
}
