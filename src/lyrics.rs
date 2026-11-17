//! 歌词：按播放器与歌曲信息匹配 QQ 音乐、网易云及 LRCLIB，同步歌词成功后缓存。

const MAX_TEXT:usize=1024*1024;
const MAX_LINES:usize=5000;
const MAX_LINE_TEXT:usize=8192;
fn rejected_lyrics()->Lyrics{crate::resources::REJECTED_DATA.fetch_add(1,std::sync::atomic::Ordering::Relaxed);Lyrics::default()}

/// 确认「所有来源都没有同步歌词」后记住 7 天，期间不再联网；网络失败不会写入。
const NEGATIVE_TTL:std::time::Duration=std::time::Duration::from_secs(7*24*3600);
const TRACE_LIMIT:usize=24;

thread_local!{static TRACE:std::cell::RefCell<Vec<String>>=const{std::cell::RefCell::new(Vec::new())};}
/// 记录本次查词经过的步骤，只写本机诊断日志，便于定位「为什么没词」。
fn trace(step:impl FnOnce()->String){
    TRACE.with(|trace|{let mut trace=trace.borrow_mut();if trace.len()<TRACE_LIMIT{trace.push(step());}});
}
fn take_trace()->Vec<String>{TRACE.with(|trace|std::mem::take(&mut *trace.borrow_mut()))}
fn outcome(result:&Result<Lyrics,FetchError>)->String{
    match result {
        Ok(lyrics) if lyrics.synced=>format!("同步{}行",lyrics.lines.len()),
        Ok(lyrics)=>format!("文本{}行",lyrics.lines.len()),
        Err(error)=>match error {FetchError::NotFound=>"未找到".into(),FetchError::IncompleteMetadata=>"信息不完整".into(),FetchError::Unavailable(_)=>"网络失败".into(),FetchError::InvalidResponse(_)=>"响应异常".into()},
    }
}

fn is_native_script(c:char)->bool {
    // 韩文音节与字母、日文假名：这类歌名对应的歌词正文应以同一种文字为主。
    matches!(c,'\u{AC00}'..='\u{D7A3}'|'\u{1100}'..='\u{11FF}'|'\u{3130}'..='\u{318F}'|'\u{3040}'..='\u{30FF}')
}

/// 歌名是韩文或日文假名，歌词正文却几乎全是拉丁字母：判定为罗马音（音译）版本。
/// 只看歌名不看歌手：韩国歌手唱英文歌（歌名是英文）时歌词本来就是英文，不能误判。
pub(crate) fn romanized(track:&str,lyrics:&Lyrics)->bool {
    if !track.chars().any(is_native_script){return false;}
    let (mut native,mut latin)=(0usize,0usize);
    for line in lyrics.lines.iter().filter(|line|!is_credit(&line.text)) {
        for c in line.text.chars() {
            if is_native_script(c){native+=1;} else if c.is_ascii_alphabetic(){latin+=1;}
        }
    }
    latin>=40 && native*20<native+latin
}

fn negative_path(path:&std::path::Path)->std::path::PathBuf {path.with_extension("none")}
thread_local!{static SKIP_NEGATIVE:std::cell::Cell<bool>=const{std::cell::Cell::new(false)};}
/// 用户手动点「重试」时，本线程的这次查询跳过「近期已确认没有歌词」的记录，真正重新联网。
/// 查询结束（含提前返回）后自动恢复，不影响之后的自动查询。
struct SkipNegative(bool);
impl SkipNegative {fn set(on:bool)->Self {Self(SKIP_NEGATIVE.with(|skip|skip.replace(on)))}}
impl Drop for SkipNegative {fn drop(&mut self){SKIP_NEGATIVE.with(|skip|skip.set(self.0));}}

fn read_negative(path:&std::path::Path)->Option<Result<Lyrics,FetchError>> {
    use std::io::Read;
    if SKIP_NEGATIVE.with(|skip|skip.get()){trace(||"手动重试:跳过无歌词记录".into());return None;}
    let path=negative_path(path);
    let file=std::fs::File::open(&path).ok()?;
    let meta=file.metadata().ok()?;
    if meta.len()>MAX_TEXT as u64 || meta.modified().ok()?.elapsed().map_or(true,|age|age>NEGATIVE_TTL){return None;}
    let mut raw=String::new();file.take(MAX_TEXT as u64).read_to_string(&mut raw).ok()?;
    let lyrics=from_plain(&raw);
    Some(if lyrics.lines.is_empty(){Err(FetchError::NotFound)}else{Ok(lyrics)})
}
fn write_negative(path:&std::path::Path,plain:Option<&Lyrics>) {
    if let Some(parent)=path.parent(){if std::fs::create_dir_all(parent).is_err(){return;}}
    let raw=plain.map(|lyrics|lyrics.lines.iter().map(|line|line.text.as_str()).collect::<Vec<_>>().join("\n")).unwrap_or_default();
    let _=crate::model::atomic_write(&negative_path(path),raw.as_bytes());
}

#[derive(Clone, Debug)]
pub struct LyricsLine {
    pub t: f32,
    pub text: String,
}

#[derive(Clone, Debug, Default)]
pub struct Lyrics {
    pub lines: Vec<LyricsLine>,
    pub synced: bool,
}

#[derive(Clone,Debug,PartialEq,Eq)]
pub enum FetchError {
    Unavailable(String),
    NotFound,
    InvalidResponse(String),
    IncompleteMetadata,
}
impl FetchError {
    pub fn label(&self)->&'static str {
        match self {
            Self::Unavailable(_)=>"暂未获取歌词，请重试",
            Self::NotFound=>"未找到匹配歌词",
            Self::InvalidResponse(_)=>"歌词响应异常",
            Self::IncompleteMetadata=>"播放器未提供完整歌曲信息",
        }
    }
}

impl Lyrics {
    /// 返回当前时间点应显示的行号。
    pub fn index_at(&self, secs: f32) -> Option<usize> {
        self.lines.iter().rposition(|line|line.t<=secs)
    }
}

fn parse_time(tag: &str) -> Option<f32> {
    // mm:ss.xx 或 mm:ss:xx
    let mut parts = tag.split(':');
    let m: u32 = parts.next()?.trim().parse().ok()?;
    let rest = parts.next()?.trim();
    let seconds=match parts.next() {Some(fraction)=>format!("{rest}.{fraction}"),None=>rest.replace(',', ".")};
    if parts.next().is_some(){return None;}
    let s: f32 = seconds.parse().ok()?;
    if !s.is_finite() || !(0.0..60.0).contains(&s){return None;}
    Some(m as f32 * 60.0 + s)
}

pub fn parse_lrc(raw: &str) -> Lyrics {
    if raw.len()>MAX_TEXT{return rejected_lyrics();}
    let mut lines = Vec::new();
    let mut copied=0usize;
    for line in raw.lines() {
        let mut times = Vec::new();
        let mut text = line;
        while text.starts_with('[') {
            let start=0;
            let Some(end_rel) = text[start..].find(']') else { break };
            let end = start + end_rel;
            let tag = &text[start + 1..end];
            if let Some(t) = parse_time(tag) {
                times.push(t);
            } else if !times.is_empty() { break; }
            text = &text[end + 1..];
        }
        let body = text.trim();
        copied=copied.saturating_add(body.len().saturating_mul(times.len()));
        if body.len()>MAX_LINE_TEXT || copied>2*MAX_TEXT || lines.len()+times.len()>MAX_LINES{return rejected_lyrics();}
        if body.is_empty() && times.is_empty() {
            continue;
        }
        for t in times {
            lines.push(LyricsLine { t, text: body.to_string() });
        }
    }
    lines.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap_or(std::cmp::Ordering::Equal));
    let synced = !lines.is_empty();
    Lyrics { lines, synced }
}

#[test]
fn quality_lyrics_keep_brackets_in_the_actual_text() {
    let parsed=parse_lrc("[00:01.00]唱到[副歌]这里\n[00:02.00][00:03.00]下一句");
    assert_eq!(parsed.lines[0].text,"唱到[副歌]这里");
    assert_eq!(parsed.lines.len(),3);
}

/// 纯文本没有同步时间，保留正文但不伪造逐行时间戳。
pub fn from_plain(raw: &str) -> Lyrics {
    if raw.len()>MAX_TEXT || raw.lines().count()>MAX_LINES || raw.lines().any(|line|line.len()>MAX_LINE_TEXT){return rejected_lyrics();}
    let lines: Vec<LyricsLine> = raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| LyricsLine { t: 0.0, text: l.trim().to_string() })
        .collect();
    Lyrics { lines, synced: false }
}

#[path="soda_cache.rs"]
mod soda_cache;
#[path="kugou_cache.rs"]
pub(crate) mod kugou_cache;

pub fn is_soda(source:&str)->bool {source=="汽水音乐" || source.to_ascii_lowercase().contains("sodamusic")}

/// 本地歌词缓存的「版本」：文件修改时间与大小都没变时不必再完整读取。目前只有汽水音乐的单文件缓存可靠适用。
pub(crate) fn native_fingerprint(source:&str)->Option<(std::time::SystemTime,u64)> {
    if is_soda(source){return soda_cache::fingerprint();}
    None
}

pub(crate) fn read_native(source:&str,track:&str,artist:&str,duration:f64)->Option<Lyrics> {
    read_native_detail(source,track,artist,duration).ok()
}

/// 本地缓存读取；未命中时给出原因，供诊断日志使用。
pub(crate) fn read_native_detail(source:&str,track:&str,artist:&str,duration:f64)->Result<Lyrics,&'static str> {
    if source.to_ascii_lowercase().contains("kugou") {return kugou_cache::read(track,artist,duration).ok_or("未命中");}
    if is_soda(source) {
        let lyrics=parse_krc(&soda_cache::read_detail(track,artist,duration).map_err(|miss|miss.label())?);
        return if lyrics.synced {Ok(lyrics)}else{Err("缓存歌词无时间轴")};
    }
    Err("不支持本地读取")
}

/// 后台本地轮询（客户端稍后才写好歌词）的结果单独记一行，命中或放弃时各写一次。
pub(crate) fn log_native(source:&str,track:&str,artist:&str,outcome:&str,elapsed:std::time::Duration) {
    if cfg!(test) {return;}
    let line=format!("{} {source} · {track} - {artist} → 本地轮询:{outcome} 用时{}ms\n",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs(),elapsed.as_millis());
    crate::resources::append_log(&crate::model::config_dir().join("diagnostics"),"lyrics.log",&line);
}

/// 返回歌词及是否仍需等待酷狗缓存；等待期间复用已有歌词，不重复访问在线服务。
pub fn fetch_for_display(source:&str,track:&str,artist:&str,duration:f64,song_id:Option<u64>,cached:Option<Lyrics>)->Result<(Lyrics,bool),FetchError>{
    fetch_for_display_with(source,track,artist,duration,song_id,cached,false)
}

/// `manual`：用户手动重试，跳过近期「没有歌词」的记录。网络失败仍不会被记成没有歌词。
pub fn fetch_for_display_with(source:&str,track:&str,artist:&str,duration:f64,song_id:Option<u64>,cached:Option<Lyrics>,manual:bool)->Result<(Lyrics,bool),FetchError>{
    take_trace();
    let _skip=SkipNegative::set(manual);
    let started=std::time::Instant::now();
    let result=(||{
        if source.to_ascii_lowercase().contains("kugou"){
            if let Some(lyrics)=kugou_cache::read(track,artist,duration){trace(||"酷狗本地:命中".into());return Ok((lyrics,false));}
            trace(||"酷狗本地:无".into());
            return cached.map(Ok).unwrap_or_else(||fetch(track,artist,duration)).map(|lyrics|(lyrics,true));
        }
        #[cfg(windows)]
        if source.eq_ignore_ascii_case("bodian_pc.exe") {
            if let Some(lyrics)=song_id.and_then(crate::smtc::bodian::read_lyrics){trace(||"波点本地:命中".into());return Ok((lyrics,false));}
            return cached.map(Ok).unwrap_or_else(||fetch(track,artist,duration)).map(|lyrics|(lyrics,true));
        }
        #[cfg(windows)]
        if source.eq_ignore_ascii_case("kwmusic.exe") {
            if let Some(lyrics)=crate::smtc::kuwo::read_lyrics(track,artist){trace(||"酷我本地:命中".into());return Ok((lyrics,false));}
            return cached.map(Ok).unwrap_or_else(||fetch(track,artist,duration)).map(|lyrics|(lyrics,true));
        }
        fetch_for_source(source,track,artist,duration,song_id).map(|lyrics|(lyrics,false))
    })();
    let summary=outcome(&result.as_ref().map(|(lyrics,_)|lyrics.clone()).map_err(Clone::clone));
    let id=song_id.map(|id|format!(" ID={id}")).unwrap_or_default();
    let line=format!("{} {source} · {track} - {artist}{id} → {} → 结果:{summary} 用时{}ms\n",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs(),
        take_trace().join(" → "),started.elapsed().as_millis());
    // 测试时不写，免得把模拟数据混进用户的真实诊断日志。
    if !cfg!(test) {crate::resources::append_log(&crate::model::config_dir().join("diagnostics"),"lyrics.log",&line);}
    result
}

/// 原生缓存优先；未缓存的新歌仍可使用已有在线来源。
pub fn fetch_for_source(source:&str,track:&str,artist:&str,duration:f64,song_id:Option<u64>)->Result<Lyrics,FetchError> {
    #[cfg(windows)]
    if source.eq_ignore_ascii_case("kwmusic.exe") {
        if let Some(lyrics)=crate::smtc::kuwo::read_lyrics(track,artist){trace(||"酷我本地:命中".into());return Ok(lyrics);}
    }
    #[cfg(windows)]
    if source.eq_ignore_ascii_case("bodian_pc.exe") {
        if let Some(lyrics)=song_id.and_then(crate::smtc::bodian::read_lyrics){trace(||"波点本地:命中".into());return Ok(lyrics);}
    }
    if source.to_ascii_lowercase().contains("qqmusic") {
        let common=cache_path(track,artist,duration);
        let path=common.parent().unwrap().join("qqmusic").join(common.file_name().unwrap());
        if let Some(lyrics)=read_cache(&path).filter(|lyrics|!romanized(track,lyrics)){trace(||"本地缓存:命中".into());return Ok(lyrics);}
        if let Some(remembered)=read_negative(&path){trace(||"近期已确认无同步歌词".into());return remembered;}
        let preferred=fetch_qqmusic("https://c.y.qq.com",track,artist,duration);
        trace(||format!("QQ音乐:{}",outcome(&preferred)));
        if let Ok(lyrics)=&preferred {if lyrics.synced && !romanized(track,lyrics) {write_cache(&path,lyrics);return preferred;}}
        let (fallback,failed)=fetch_online_detail(track,artist,duration,Skip::QqMusic);
        if !failed && matches!(&preferred,Ok(_)|Err(FetchError::NotFound)) && !fallback.as_ref().is_ok_and(|l|l.synced) {
            write_negative(&path,preferred.as_ref().ok().or(fallback.as_ref().ok()));
        }
        return match (preferred,fallback) {
            (_,Ok(lyrics)) if lyrics.synced=>Ok(lyrics),
            (Ok(lyrics),_)|(_,Ok(lyrics))=>Ok(lyrics),
            (Err(FetchError::NotFound),Err(error))|(Err(error),_)=>Err(error),
        };
    }
    if source.to_ascii_lowercase().contains("kugou"){
        if let Some(lyrics)=kugou_cache::read(track,artist,duration){return Ok(lyrics);}
    }
    if source.eq_ignore_ascii_case("cloudmusic.exe"){
        if let Some(id)=song_id.filter(|id|*id>0){
            let path=crate::model::config_dir().join("lyrics").join(format!("netease-{id}.lrc"));
            if let Some(lyrics)=read_cache(&path).filter(|lyrics|!romanized(track,lyrics)){trace(||"本地缓存:命中".into());return Ok(lyrics);}
            if let Some(remembered)=read_negative(&path){trace(||"近期已确认无同步歌词".into());return remembered;}
            let by_id=fetch_netease_song("https://music.163.com",id);
            trace(||format!("网易云按ID:{}",outcome(&by_id)));
            match by_id{
                Ok(lyrics) if lyrics.synced=>{write_cache(&path,&lyrics);return Ok(lyrics);}
                // 网易云本身只有文本歌词（「暂无自动滚动」）：不再重复搜网易云，改到 QQ 音乐、LRCLIB 找同步版本。
                Ok(plain)=>{
                    let (other,failed)=fetch_online_detail(track,artist,duration,Skip::Netease);
                    if let Ok(lyrics)=&other {if lyrics.synced{if !romanized(track,lyrics){write_cache(&path,lyrics);}return other;}}
                    if !failed{write_negative(&path,Some(&plain));}
                    return Ok(plain);
                }
                // 按 ID 取词失败（限流、断网）时仍走搜索，避免一次失败就整首无词。
                Err(_)=>{}
            }
        }
    }
    if is_soda(source) {
        match soda_cache::read_detail(track,artist,duration) {
            Ok(raw)=>{
                let lyrics=parse_krc(&raw);
                if lyrics.synced {trace(||"汽水本地:命中".into());return Ok(lyrics);}
                trace(||"汽水本地:无时间轴".into());
            }
            Err(miss)=>trace(||format!("汽水本地:{}",miss.label())),
        }
    }
    fetch(track,artist,duration)
}

fn parse_krc(raw:&str)->Lyrics {
    if raw.len()>MAX_TEXT || raw.lines().count()>MAX_LINES/2{return rejected_lyrics();}
    static LINE_TAG:std::sync::OnceLock<regex::Regex>=std::sync::OnceLock::new();
    static WORD_TAG:std::sync::OnceLock<regex::Regex>=std::sync::OnceLock::new();
    // 本地歌词会被轮询读取，正则只编译一次。
    let line_tag=LINE_TAG.get_or_init(||regex::Regex::new(r"^\[(\d+),(\d+)\]").unwrap());
    let word_tag=WORD_TAG.get_or_init(||regex::Regex::new(r"<\d+,\d+,\d+>").unwrap());
    let mut timed=Vec::new();
    for line in raw.lines() {
        let Some(c)=line_tag.captures(line) else{continue;};
        let (Ok(start),Ok(length))=(c[1].parse::<u32>(),c[2].parse::<u32>()) else{continue;};
        let Some(end)=start.checked_add(length).filter(|end|*end<=86_400_000) else{continue;};
        let text=word_tag.replace_all(&line[c.get(0).unwrap().end()..],"").trim().to_string();
        if text.len()>MAX_LINE_TEXT{return rejected_lyrics();}
        timed.push((start,end,text));
    }
    if timed.is_empty(){return parse_lrc(raw);}
    timed.sort_by_key(|line|line.0);
    let mut lines=Vec::new();
    for (i,(start,end,text)) in timed.iter().enumerate() {
        lines.push(LyricsLine{t:*start as f32/1000.0,text:text.clone()});
        // 短暂换气保留上一句，只在至少 3 秒的间奏或尾奏清空歌词。
        if end>start && timed.get(i+1).is_none_or(|next|next.0.saturating_sub(*end)>=3000) {
            lines.push(LyricsLine{t:*end as f32/1000.0,text:String::new()});
        }
    }
    Lyrics{synced:lines.iter().any(|line|!line.text.is_empty()),lines}
}

pub fn fetch(track: &str, artist: &str, duration_secs: f64) -> Result<Lyrics, FetchError> {
    fetch_online(track,artist,duration_secs,Skip::None)
}

#[derive(Clone,Copy,PartialEq,Eq)]
enum Skip {None,Netease,QqMusic}

/// 通用在线查询：网易云 → QQ 音乐 → LRCLIB，任一来源有同步歌词即返回；已查过的来源可跳过。
fn fetch_online(track:&str,artist:&str,duration_secs:f64,skip:Skip)->Result<Lyrics,FetchError> {
    fetch_online_detail(track,artist,duration_secs,skip).0
}

/// 第二个返回值：是否有来源失败（结果可能不完整，调用方不应据此缓存「没有歌词」）。
fn fetch_online_detail(track:&str,artist:&str,duration_secs:f64,skip:Skip)->(Result<Lyrics,FetchError>,bool) {
    if track.trim().is_empty() || artist.trim().is_empty(){return (Err(FetchError::IncompleteMetadata),true);}
    let path=cache_path(track,artist,duration_secs);
    // 旧版本可能缓存过罗马音版本：忽略它重新查找原文。
    if let Some(lyrics)=read_cache(&path).filter(|lyrics|!romanized(track,lyrics)){trace(||"本地缓存:命中".into());return (Ok(lyrics),false);}
    if let Some(remembered)=read_negative(&path){trace(||"近期已确认无同步歌词".into());return (remembered,false);}
    let netease=(skip!=Skip::Netease).then_some("https://music.163.com");
    let qq=(skip!=Skip::QqMusic).then_some("https://c.y.qq.com");
    let (result,failed)=fetch_with_sources_detail(netease,qq,Some("https://lrclib.net"),track,artist,duration_secs);
    match &result {
        // 罗马音版本照常显示，但不缓存，下次仍尝试找原文。
        Ok(lyrics) if lyrics.synced=>{if !romanized(track,lyrics){write_cache(&path,lyrics);}},
        // 每个来源都正常回应却没有同步歌词，才记为「确认没有」；有来源断网或限流时下次照常重查。
        Ok(lyrics) if !failed=>write_negative(&path,Some(lyrics)),
        Err(FetchError::NotFound) if !failed=>write_negative(&path,None),
        _=>{}
    }
    (result,failed)
}

fn cache_path(track:&str,artist:&str,duration:f64)->std::path::PathBuf {
    let key=format!("{}|{}|{duration:.0}",normalized(track),normalized(artist));
    let hash=key.bytes().fold(14695981039346656037u64,|h,b|(h^b as u64).wrapping_mul(1099511628211));
    crate::model::config_dir().join("lyrics").join(format!("{hash:016x}.lrc"))
}

fn read_cache(path:&std::path::Path)->Option<Lyrics> {
    use std::io::Read;
    let file=std::fs::File::open(path).ok()?;
    if file.metadata().ok()?.len()>MAX_TEXT as u64{rejected_lyrics();return None;}
    let mut raw=String::new();file.take(MAX_TEXT as u64+1).read_to_string(&mut raw).ok()?;
    let lyrics=parse_lrc(&raw);
    (lyrics.synced && lyrics.lines.iter().any(|l|!l.text.is_empty())).then_some(lyrics)
}

fn write_cache(path:&std::path::Path,lyrics:&Lyrics) {
    if let Some(parent)=path.parent(){if std::fs::create_dir_all(parent).is_err(){return;}}
    let raw=lyrics.lines.iter().map(|line|format!("[{:02}:{:06.3}]{}\n",(line.t/60.0) as u32,line.t%60.0,line.text)).collect::<String>();
    let _=crate::model::atomic_write(path,raw.as_bytes());
}

#[cfg(test)]
fn fetch_with_sources(netease:Option<&str>,qq:Option<&str>,lrclib:Option<&str>,track:&str,artist:&str,duration:f64)->Result<Lyrics,FetchError> {
    fetch_with_sources_detail(netease,qq,lrclib,track,artist,duration).0
}

/// 电台、DJ、部分网页播放器把「歌名 - 歌手」整个放进标题、歌手栏留空：拆成两半，只拆唯一的一个分隔符。
pub(crate) fn split_title_artist(title:&str)->Option<(String,String)> {
    for separator in [" - "," – "," — "] {
        if title.matches(separator).count()!=1 {continue;}
        let (left,right)=title.split_once(separator)?;
        let (left,right)=(left.trim(),right.trim());
        if !left.is_empty() && !right.is_empty() {return Some((left.to_owned(),right.to_owned()));}
    }
    None
}

/// 去掉开头的节目标注（【电台】、[DJ]）和结尾的版本标注（(DJ版)、（伴奏）、【现场】），只留歌名本体。
pub(crate) fn bare_title(title:&str)->String {
    static LEADING:std::sync::OnceLock<regex::Regex>=std::sync::OnceLock::new();
    static TRAILING:std::sync::OnceLock<regex::Regex>=std::sync::OnceLock::new();
    let leading=LEADING.get_or_init(||regex::Regex::new(r"^\s*(?:【[^】]*】|\[[^\]]*\])\s*").unwrap());
    let trailing=TRAILING.get_or_init(||regex::Regex::new(r"\s*(?:\([^()]*\)|（[^（）]*）|\[[^\]]*\]|【[^】]*】)\s*$").unwrap());
    let mut text=title.trim().to_owned();
    loop {
        let next=trailing.replace(&leading.replace(&text,""),"").trim().to_owned();
        if next==text || next.is_empty() {return text;}
        text=next;
    }
}

/// 在线找词：先按播放器给的歌名、歌手找；落空时再按两种兜底各试一次——
/// 歌手为空时从标题里拆出歌手（两种顺序），以及去掉节目 / 版本标注（只在有时长时，靠时长校验避免套错版本）。
/// 有来源断网或限流时不再追加尝试，免得一首歌放大成多倍请求。
fn fetch_with_sources_detail(netease:Option<&str>,qq:Option<&str>,lrclib:Option<&str>,track:&str,artist:&str,duration:f64)->(Result<Lyrics,FetchError>,bool) {
    let mut attempts=vec![(track.to_owned(),artist.to_owned())];
    if artist.trim().is_empty() {
        if let Some((left,right))=split_title_artist(track) {attempts.push((left.clone(),right.clone()));attempts.push((right,left));}
    }
    let bare=bare_title(track);
    if duration>0.0 && bare!=track.trim() {attempts.push((bare,artist.to_owned()));}
    let mut best:Option<Result<Lyrics,FetchError>>=None;
    for (index,(title,singer)) in attempts.into_iter().enumerate() {
        if index>0 {trace(||format!("兜底:{title} - {}",if singer.is_empty(){"（无歌手）"}else{&singer}));}
        let (result,failed)=fetch_with_sources_once(netease,qq,lrclib,&title,&singer,duration);
        if failed || result.as_ref().is_ok_and(|lyrics|lyrics.synced) {return (result,failed);}
        // 纯文本先留着，继续找同步版本；都没有时用第一份纯文本。
        if best.as_ref().is_none_or(|kept|kept.is_err() && result.is_ok()) {best=Some(result);}
    }
    (best.unwrap_or(Err(FetchError::NotFound)),false)
}

/// 按顺序尝试，命中同步歌词立即停止；全部只有纯文本时取第一个，网络类错误优先于「未找到」上报以便重试。
/// 第二个返回值表示是否有来源失败（断网、限流、响应异常），这时结果不能当作「确认没有」缓存。
fn fetch_with_sources_once(netease:Option<&str>,qq:Option<&str>,lrclib:Option<&str>,track:&str,artist:&str,duration:f64)->(Result<Lyrics,FetchError>,bool) {
    let mut plain=None;
    let mut transliterated=None;
    let mut failure=None;
    let attempts:[(&str,Option<&str>,fn(&str,&str,&str,f64)->Result<Lyrics,FetchError>);3]=[("网易云",netease,fetch_netease),("QQ音乐",qq,fetch_qqmusic),("LRCLIB",lrclib,fetch_from)];
    for (name,base,fetch) in attempts {
        let Some(base)=base else{continue;};
        let result=fetch(base,track,artist,duration);
        let roman=result.as_ref().is_ok_and(|lyrics|lyrics.synced && romanized(track,lyrics));
        trace(||format!("{name}:{}{}",outcome(&result),if roman{"(罗马音)"}else{""}));
        match result {
            // 罗马音版本先留作备选，继续向后面的来源找原文。
            Ok(lyrics) if roman=>{if transliterated.is_none(){transliterated=Some(lyrics);}}
            Ok(lyrics) if lyrics.synced=>return (Ok(lyrics),failure.is_some()),
            Ok(lyrics)=>{if plain.is_none(){plain=Some(lyrics);}}
            Err(FetchError::NotFound)=>{}
            Err(error)=>{if failure.is_none(){failure=Some(error);}}
        }
    }
    let failed=failure.is_some();
    // 没有原文同步歌词时，罗马音同步歌词仍比纯文本更有用（岛上能逐句显示）。
    if let Some(lyrics)=transliterated {return (Ok(lyrics),failed);}
    (match (plain,failure) {
        (Some(lyrics),_)=>Ok(lyrics),
        (None,Some(error))=>Err(error),
        (None,None)=>Err(FetchError::NotFound),
    },failed)
}

/// 播放器两路来源（SMTC 与网易云日志）的歌名、歌手写法可能不同：统一规范化后再比较是否同一首。
pub(crate) fn same_song(title:&str,artist:&str,other_title:&str,other_artist:&str)->bool {
    let (a,b)=(song_identity(title,artist),song_identity(other_title,other_artist));
    !a.0.is_empty() && a.0==b.0 && (a.1.is_empty() || b.1.is_empty() || a.1==b.1)
}

/// 同一会话里前后两次歌曲信息的关系：`Some(true)` 同一首（歌手相同或一方为空），
/// `Some(false)` 同一首但署名有补全（如单人署名补全为合唱），`None` 不同歌曲。
pub(crate) fn song_relation(title:&str,artist:&str,other_title:&str,other_artist:&str)->Option<bool> {
    if same_song(title,artist,other_title,other_artist){return Some(true);}
    let (a,b)=(song_identity(title,artist),song_identity(other_title,other_artist));
    if a.0.is_empty() || a.0!=b.0 {return None;}
    let within=|x:&[String],y:&[String]|x.iter().all(|s|y.contains(s));
    (within(&a.1,&b.1) || within(&b.1,&a.1)).then_some(false)
}

fn normalized(text:&str)->String {
    text.chars().flat_map(char::to_lowercase).filter(|c|c.is_alphanumeric()).collect()
}

pub(super) fn artist_set(artist:&str)->Vec<String>{
    let mut parts=artist.split(['/', '、', ',', '，', '&', ';', '；']).map(normalized).filter(|s|!s.is_empty()).collect::<Vec<_>>();
    parts.sort();parts.dedup();parts
}

fn split_feature(text:&str)->(&str,&str) {
    static FEATURE:std::sync::OnceLock<regex::Regex>=std::sync::OnceLock::new();
    let pattern=FEATURE.get_or_init(||regex::Regex::new(r"(?i)\b(?:feat(?:uring)?|ft)(?:\.\s*|\s+)([^()（）\[\]]+)[)）\]]?\s*$").unwrap());
    match pattern.captures(text) {
        Some(c)=>(text[..c.get(0).unwrap().start()].trim_end_matches([' ','(','（','[']),c.get(1).unwrap().as_str().trim()),
        None=>(text.trim(),""),
    }
}

/// 合唱署名可能位于歌名或歌手字段；只移动明确的署名，保留 Live、Remix 等版本信息。
fn song_identity(title:&str,artist:&str)->(String,Vec<String>) {
    let cleaned=clean_title(title);
    let (title,guests)=split_feature(&cleaned);
    let (lead,featured)=split_feature(artist);
    let artists=if artist.trim().is_empty(){Vec::new()}else{artist_set(&format!("{lead}/{featured}/{guests}"))};
    (normalized(title),artists)
}

fn search_terms(track:&str,artist:&str)->(String,String) {
    (split_feature(&clean_title(track)).0.to_owned(),split_feature(artist).0.to_owned())
}

fn netease_candidates(items:&[serde_json::Value],track:&str,artist:&str,duration:f64)->Vec<u64> {
    let title=normalized(track);
    let identity=song_identity(track,artist);
    let mut matches=Vec::new();
    for item in items {
        let Some(id)=item.get("id").and_then(|v|v.as_u64()) else{continue;};
        let name=item.get("name").and_then(|v|v.as_str()).unwrap_or_default();
        let exact=normalized(name)==title;
        let artists=item.get("artists").or_else(||item.get("ar")).and_then(|v|v.as_array());
        let Some(artists)=artists else{continue;};
        let names=artists.iter().filter_map(|a|a.get("name")?.as_str()).collect::<Vec<_>>().join("、");
        if identity.1.is_empty() || song_identity(name,&names)!=identity{continue;}
        let candidate_duration=item.get("duration").or_else(||item.get("dt")).and_then(|v|v.as_f64()).unwrap_or_default()/1000.0;
        let difference=if duration>0.0 {(candidate_duration-duration).abs()}else{0.0};
        if duration>0.0 && difference>(duration*0.03).max(10.0){continue;}
        matches.push((id,!exact,difference));
    }
    matches.sort_by(|a,b|a.1.cmp(&b.1).then(a.2.total_cmp(&b.2)));
    matches.into_iter().map(|(id,_,_)|id).take(3).collect()
}

fn fetch_netease(base:&str,track:&str,artist:&str,duration:f64)->Result<Lyrics,FetchError> {
    let (query_title,query_artist)=search_terms(track,artist);
    let mut candidates=Vec::new();
    // 「歌名 歌手」搜不到时再只按歌名搜一次：多歌手、译名歌手常让组合关键词落空，匹配仍校验歌手和时长。
    for (i,query) in [format!("{query_title} {query_artist}"),query_title.clone()].into_iter().enumerate() {
        let url=format!("{base}/api/search/get/web?s={}&type=1&limit=30",urlencode(&query));
        let result=match request_json(&url,base){Ok(result)=>result,Err(error) if i==0=>return Err(error),Err(_)=>break};
        if result.get("code").and_then(|v|v.as_i64())!=Some(200){
            if i==0{return Err(FetchError::Unavailable("网易云搜索未成功".into()));}
            break;
        }
        let items=result.pointer("/result/songs").and_then(|v|v.as_array()).cloned().unwrap_or_default();
        candidates=netease_candidates(&items,track,artist,duration);
        if !candidates.is_empty(){break;}
    }
    let mut plain=None;
    let mut failure=FetchError::NotFound;
    for id in candidates {
        match fetch_netease_song(base,id){
            Ok(lyrics)if lyrics.synced=>return Ok(lyrics),
            Ok(lyrics)=>plain=Some(lyrics),
            Err(FetchError::NotFound)=>{},
            Err(error)=>failure=error,
        }
    }
    plain.ok_or(failure)
}

fn fetch_netease_song(base:&str,id:u64)->Result<Lyrics,FetchError>{
    let result=request_json(&format!("{base}/api/song/lyric?id={id}&lv=-1&kv=-1&tv=-1"),base)?;
    if result.get("code").and_then(|v|v.as_i64())!=Some(200){return Err(FetchError::Unavailable("网易云取词未成功".into()));}
    if result.get("nolyric").and_then(|v|v.as_bool())==Some(true) || result.get("uncollected").and_then(|v|v.as_bool())==Some(true){return Err(FetchError::NotFound);}
    let raw=result.pointer("/lrc/lyric").and_then(|v|v.as_str()).filter(|s|!s.trim().is_empty()).ok_or(FetchError::NotFound)?;
    Ok(lyrics_from_text(raw))
}

fn is_credit(text:&str)->bool {
    static CREDIT:std::sync::OnceLock<regex::Regex>=std::sync::OnceLock::new();
    CREDIT.get_or_init(||regex::Regex::new(r"^\s*(?:作词|作曲|编曲|词|曲|演唱|原唱|制作人|监制|混音|母带|和声|吉他|贝斯|鼓|弦乐|录音|出品|发行|OP|SP|Lyrics?|Lyricist|Composer|Arranger|Producer)\s*[:：]").unwrap()).is_match(text)
}

/// 网易云「暂无自动滚动」的歌常只有作词、作曲几行带时间，正文没有时间戳；这种不能当同步歌词，否则岛上会一直停在署名行。
fn lyrics_from_text(raw:&str)->Lyrics {
    let lyrics=parse_lrc(raw);
    let body_timed=lyrics.lines.iter().any(|line|!line.text.trim().is_empty() && !is_credit(&line.text));
    if lyrics.synced && body_timed {return lyrics;}
    // 去掉行首时间标签与网易云的 JSON 署名行，保留可读正文。
    let text=raw.lines().filter(|line|!line.trim_start().starts_with("{\"t\"")).map(|line|{
        let mut rest=line.trim_start();
        while rest.starts_with('[') {
            match rest.find(']') {Some(end) if parse_time(&rest[1..end]).is_some()=>rest=&rest[end+1..],_=>break}
        }
        rest
    }).collect::<Vec<_>>().join("\n");
    from_plain(&text)
}

fn qq_candidates(items:&[serde_json::Value],track:&str,artist:&str,duration:f64)->Vec<String> {
    // 统一字段后复用严格匹配，保留歌手、版本与时长约束。
    let candidates:Vec<_>=items.iter().enumerate().map(|(i,item)|serde_json::json!({
        "id":i,"name":item["songname"],"artists":item["singer"],
        "duration":item["interval"].as_f64().unwrap_or_default()*1000.0,
    })).collect();
    netease_candidates(&candidates,track,artist,duration).into_iter().filter_map(|i|
        items[i as usize]["songmid"].as_str().filter(|mid|!mid.is_empty() && mid.bytes().all(|b|b.is_ascii_alphanumeric())).map(str::to_owned)
    ).collect()
}

fn fetch_qqmusic(base:&str,track:&str,artist:&str,duration:f64)->Result<Lyrics,FetchError> {
    if track.trim().is_empty() || artist.trim().is_empty(){return Err(FetchError::IncompleteMetadata);}
    let (query_title,query_artist)=search_terms(track,artist);
    let query=urlencode(&format!("{query_title} {query_artist}"));
    let result=request_json_with_retry(&format!("{base}/soso/fcgi-bin/client_search_cp?w={query}&format=json&p=1&n=12"),"https://y.qq.com/",true)?;
    if result["code"].as_i64()!=Some(0){return Err(FetchError::Unavailable("QQ 音乐搜索未成功".into()));}
    let items=result.pointer("/data/song/list").and_then(|v|v.as_array()).ok_or_else(||FetchError::InvalidResponse("QQ 音乐未返回歌曲列表".into()))?;
    let mut plain=None;
    let mut failure=FetchError::NotFound;
    for mid in qq_candidates(items,track,artist,duration) {
        let result=match request_json_with_retry(&format!("{base}/lyric/fcgi-bin/fcg_query_lyric_new.fcg?songmid={mid}&format=json&nobase64=1"),"https://y.qq.com/",true) {
            Ok(result)=>result,
            Err(error)=>{failure=error;continue;},
        };
        if result["code"].as_i64()!=Some(0){failure=FetchError::Unavailable("QQ 音乐取词未成功".into());continue;}
        if let Some(raw)=result["lyric"].as_str().filter(|raw|!raw.trim().is_empty()) {
            let lyrics=lyrics_from_text(raw);
            if lyrics.synced{return Ok(lyrics);}
            if plain.is_none(){plain=Some(lyrics);}
        }
    }
    plain.ok_or(failure)
}

fn request_json(url:&str,referer:&str)->Result<serde_json::Value,FetchError> {
    request_json_with_retry(url,referer,false)
}

fn request_json_with_retry(url:&str,referer:&str,retry:bool)->Result<serde_json::Value,FetchError> {
    let request=ureq::get(url)
        .set("User-Agent",concat!("Daozai/",env!("CARGO_PKG_VERSION")," (Windows)"))
        .set("Referer",referer)
        .set("Accept","application/json")
        .timeout(std::time::Duration::from_secs(5));
    let mut result=request.clone().call();
    // QQ 的偶发连接超时、网关错误只重试一次；拒绝访问和无歌词不重复请求。
    if retry && result.as_ref().is_err_and(|error|matches!(error,ureq::Error::Transport(_)|ureq::Error::Status(408|500|502|503|504,_))) {
        std::thread::sleep(std::time::Duration::from_millis(200));
        result=request.call();
    }
    let response=result.map_err(|e|FetchError::Unavailable(e.to_string()))?;
    use std::io::Read;
    let mut text=String::new();
    response.into_reader().take((2*MAX_TEXT+1)as u64).read_to_string(&mut text).map_err(|e|FetchError::InvalidResponse(e.to_string()))?;
    if text.len()>2*MAX_TEXT{rejected_lyrics();return Err(FetchError::InvalidResponse("歌词响应超过大小上限".into()));}
    serde_json::from_str(&text).map_err(|e|FetchError::InvalidResponse(e.to_string()))
}

fn clean_title(track:&str)->String {
    static SUFFIX:std::sync::OnceLock<regex::Regex>=std::sync::OnceLock::new();
    let suffix=SUFFIX.get_or_init(||regex::Regex::new(r"(?i)\s*[\(\[（【]\s*(?:official(?:\s+music)?\s+(?:video|audio)|lyrics?|lyric\s+video)\s*[\)\]）】]").unwrap());
    suffix.replace_all(track,"").split_whitespace().collect::<Vec<_>>().join(" ")
}

fn fetch_from(base:&str,track:&str,artist:&str,duration_secs:f64)->Result<Lyrics,FetchError> {
    if track.trim().is_empty() {
        return Err(FetchError::NotFound);
    }
    let identity=song_identity(track,artist);
    let (query_title,query_artist)=search_terms(track,artist);
    let items=search(base,&query_title,&query_artist)?;
    let items=items.into_iter().filter(|item|{
        let name=item.get("trackName").and_then(|v|v.as_str()).unwrap_or_default();
        let singer=item.get("artistName").and_then(|v|v.as_str()).unwrap_or_default();
        !identity.1.is_empty() && song_identity(name,singer)==identity
    }).collect::<Vec<_>>();
    lyrics_from_items(&items,track,duration_secs)
}

fn search(base:&str,track:&str,artist:&str)->Result<Vec<serde_json::Value>,FetchError> {
    let url = format!(
        "{base}/api/search?track_name={}&artist_name={}",
        urlencode(track),
        urlencode(artist)
    );
    let arr=request_json(&url,base)?;
    arr.as_array().cloned().ok_or_else(||FetchError::InvalidResponse("预期歌词列表".into()))
}

fn lyrics_from_items(items:&[serde_json::Value],track:&str,duration_secs:f64)->Result<Lyrics,FetchError> {
    // LRCLIB 同一首韩文、日文歌常同时有原文和罗马音版本：原文优先，其次时长更接近。
    let mut synced:Option<(Lyrics,(bool,f64))>=None;
    let mut plain:Option<(Lyrics,f64)>=None;
    for it in items {
        let d = it.get("duration").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let score=if duration_secs>0.0{(d-duration_secs).abs()}else{0.0};
        if duration_secs>0.0 && d>0.0 && score>(duration_secs*0.03).max(10.0){continue;}
        if let Some(raw)=it.get("syncedLyrics").and_then(|v|v.as_str()) {
            let parsed=parse_lrc(raw);
            let rank=(romanized(track,&parsed),score);
            if !parsed.lines.is_empty() && synced.as_ref().is_none_or(|(_,s)|rank.0<s.0 || rank.0==s.0 && rank.1<s.1){synced=Some((parsed,rank));}
        }
        if let Some(raw)=it.get("plainLyrics").and_then(|v|v.as_str()).filter(|s|!s.trim().is_empty()) {
            if plain.as_ref().is_none_or(|(_,s)|score<*s){plain=Some((from_plain(raw),score));}
        }
    }
    synced.map(|(lyrics,_)|lyrics).or(plain.map(|(lyrics,_)|lyrics)).ok_or(FetchError::NotFound)
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn radio_style_titles_are_split_and_stripped_conservatively(){
        assert_eq!(split_title_artist("晴天 - 周杰伦"),Some(("晴天".into(),"周杰伦".into())));
        assert_eq!(split_title_artist("周杰伦 – 晴天"),Some(("周杰伦".into(),"晴天".into())));
        assert_eq!(split_title_artist("A - B - C"),None,"多个分隔符时不猜");
        assert_eq!(split_title_artist("Re-Make"),None,"连字符不是分隔符");
        assert_eq!(bare_title("【周末电台】晴天 (DJ版)"),"晴天");
        assert_eq!(bare_title("遥远的她（Live）【现场】"),"遥远的她");
        assert_eq!(bare_title("(没有歌名)"),"(没有歌名)","去掉后为空时保留原样");
        assert_eq!(bare_title("晴天"),"晴天");
    }
    #[test]
    fn resource_network_and_disk_lyrics_have_input_limits(){
        let oversized="x".repeat(2*1024*1024+1);
        let (base,worker)=serve(vec![("200 OK",oversized)]);
        assert!(matches!(request_json(&base,&base),Err(FetchError::InvalidResponse(_))));worker.join().unwrap();
        let path=std::env::temp_dir().join(format!("daozai-large-lyrics-{}.lrc",std::process::id()));
        std::fs::write(&path,format!("[00:01]{}","x".repeat(MAX_TEXT))).unwrap();
        assert!(read_cache(&path).is_none());std::fs::remove_file(path).unwrap();
        assert!(from_plain(&"x\n".repeat(MAX_LINES+1)).lines.is_empty());
    }
    #[test]
    fn resource_lyrics_reject_oversized_input_and_timestamp_amplification(){
        assert!(parse_lrc(&format!("[00:01]{}","x".repeat(MAX_LINE_TEXT+1))).lines.is_empty());
        assert!(parse_lrc(&format!("[00:01]{}","字".repeat(400_000))).lines.is_empty());
        let raw=format!("{}{}","[00:01]".repeat(64),"x".repeat(64*1024));
        let lyrics=parse_lrc(&raw);
        assert!(lyrics.lines.iter().map(|line|line.text.len()).sum::<usize>()<=2*1024*1024,"重复时间戳不能成倍复制大段歌词");
    }
    #[test]
    fn matching_unifies_featured_credits_across_sources_without_losing_versions() {
        for (title,artist,name,singers) in [
            ("Track (feat. Guest)","Artist","Track","Guest / Artist"),
            ("Track ft. Guest","Artist","Track (featuring Guest)","Artist"),
            ("Track ft.Guest","Artist","Track (feat. Guest)","Artist"),
            ("Track","Artist feat. Guest","Track","Artist、Guest"),
            ("Track (Live) [FEAT. Guest]","Artist","Track (Live)","Artist & Guest"),
        ] {
            let items=serde_json::json!([{"id":1,"name":name,"artists":[{"name":singers}],"duration":120000}]);
            assert_eq!(netease_candidates(items.as_array().unwrap(),title,artist,120.0),vec![1]);
            let rows=serde_json::json!([{"trackName":name,"artistName":singers,"duration":120,"syncedLyrics":"[00:01.00]正确歌词"}]);
            let (base,worker)=serve(vec![("200 OK",rows.to_string())]);
            let result=fetch_from(&base,title,artist,120.0);
            worker.join().unwrap();assert!(result.unwrap().synced);
        }
        for (name,singers) in [("Track (Live)","Artist / Guest"),("Track","Cover / Guest"),("Track","Artist / Other"),("Track","Artist / Guest / Other")] {
            let items=serde_json::json!([{"id":1,"name":name,"artists":[{"name":singers}],"duration":120000}]);
            assert!(netease_candidates(items.as_array().unwrap(),"Track (feat. Guest)","Artist",120.0).is_empty());
        }
    }

    #[test]
    fn online_candidates_continue_after_one_lyric_request_fails() {
        let search=serde_json::json!({"code":200,"result":{"songs":[
            {"id":1,"name":"Track","artists":[{"name":"Artist"}],"duration":120000},
            {"id":2,"name":"Track","artists":[{"name":"Artist"}],"duration":120000}
        ]}}).to_string();
        let lyric=serde_json::json!({"code":200,"lrc":{"lyric":"[00:01.00]正确歌词"}}).to_string();
        let (base,worker)=serve(vec![("200 OK",search),("403 Forbidden",String::new()),("200 OK",lyric)]);
        let result=fetch_netease(&base,"Track","Artist",120.0);
        assert!(result.unwrap().synced);worker.join().unwrap();
        let search=serde_json::json!({"code":0,"data":{"song":{"list":[
            {"songmid":"first","songname":"Track","singer":[{"name":"Artist"}],"interval":120},
            {"songmid":"second","songname":"Track","singer":[{"name":"Artist"}],"interval":120}
        ]}}}).to_string();
        let lyric=serde_json::json!({"code":0,"lyric":"[00:01.00]正确歌词"}).to_string();
        for failure in [("403 Forbidden",String::new()),("200 OK",r#"{"code":-1}"#.into())] {
            let (base,worker)=serve(vec![("200 OK",search.clone()),failure,("200 OK",lyric.clone())]);
            let result=fetch_qqmusic(&base,"Track","Artist",120.0);
            assert!(result.unwrap().synced);worker.join().unwrap();
        }
    }

    #[test]
    fn qq_featured_artist_in_title_completes_primary_artist_metadata() {
        let title="溯 (Reverse)feat. 马吟吟";
        let items=serde_json::json!([
            {"songmid":"correct","songname":title,"singer":[{"name":"CORSAK胡梦周"},{"name":"马吟吟"}],"interval":192},
            {"songmid":"cover","songname":title,"singer":[{"name":"其他歌手"},{"name":"马吟吟"}],"interval":192},
            {"songmid":"extra","songname":title,"singer":[{"name":"CORSAK胡梦周"},{"name":"马吟吟"},{"name":"其他歌手"}],"interval":192},
            {"songmid":"long","songname":title,"singer":[{"name":"CORSAK胡梦周"},{"name":"马吟吟"}],"interval":230}
        ]);
        for artist in ["CORSAK胡梦周","CORSAK胡梦周 / 马吟吟"] {
            assert_eq!(qq_candidates(items.as_array().unwrap(),title,artist,192.0),vec!["correct"]);
        }
        assert!(qq_candidates(items.as_array().unwrap(),title,"",192.0).is_empty());
        assert!(qq_candidates(items.as_array().unwrap(),"溯 (Reverse)feat. 马吟吟 (Live)","CORSAK胡梦周",192.0).is_empty());
    }

    #[test]
    #[ignore="按需验证用户反馈的合唱歌曲，只输出同步状态和行数"]
    fn live_qq_featured_track() {
        let lyrics=fetch_qqmusic("https://c.y.qq.com","溯 (Reverse)feat. 马吟吟","CORSAK胡梦周",192.0).unwrap();
        assert!(lyrics.synced && lyrics.lines.len()>10);
        assert!(lyrics.index_at(150.0)>lyrics.index_at(30.0));
        println!("同步歌词 {} 行，时间轴正常",lyrics.lines.len());
    }

    #[test]
    fn qq_matching_rejects_wrong_artist_version_duration_and_invalid_id() {
        let items=serde_json::json!([
            {"songmid":"wrongartist","songname":"Track","singer":[{"name":"Cover"}],"interval":215},
            {"songmid":"live","songname":"Track (Live)","singer":[{"name":"Artist"}],"interval":215},
            {"songmid":"long","songname":"Track","singer":[{"name":"Artist"}],"interval":300},
            {"songmid":"bad&id=1","songname":"Track","singer":[{"name":"Artist"}],"interval":215},
            {"songmid":"correct","songname":"Track","singer":[{"name":"Artist"}],"interval":215}
        ]);
        assert_eq!(qq_candidates(items.as_array().unwrap(),"Track","Artist",215.0),vec!["correct"]);
    }

    #[test]
    fn qq_fetch_handles_synced_missing_and_unavailable_lyrics() {
        let search=serde_json::json!({"code":0,"data":{"song":{"list":[{"songmid":"testMID","songname":"Track","singer":[{"name":"Artist"}],"interval":215}]}}}).to_string();
        for (code,raw,expected) in [(0,"[00:01.00]第一句\n[00:05.00]第二句",0),(0,"",1),(-1,"",2)] {
            let lyric=serde_json::json!({"code":code,"lyric":raw}).to_string();
            let (base,worker)=serve(vec![("200 OK",search.clone()),("200 OK",lyric)]);
            let result=fetch_qqmusic(&base,"Track","Artist",215.0);
            match expected {0=>assert!(result.unwrap().synced),1=>assert_eq!(result.unwrap_err(),FetchError::NotFound),_=>assert!(matches!(result,Err(FetchError::Unavailable(_))))}
            worker.join().unwrap();
        }
    }

    #[test]
    #[ignore="按需验证用户反馈的 QQ 音乐歌曲，只输出同步状态和行数"]
    fn live_qq_reported_track() {
        let lyrics=fetch_for_source("QQMusic.exe","兰香如故","叶炫清",215.0,None).unwrap();
        assert!(lyrics.synced && lyrics.lines.len()>10);
        assert!(lyrics.index_at(105.0)>lyrics.index_at(52.0));
        println!("QQ 音乐：同步歌词 {} 行，时间轴正常",lyrics.lines.len());
    }

    #[test]
    #[ignore="绕过歌词缓存，验证 QQ 在线搜索及取词"]
    fn live_qq_uncached_tracks() {
        for (track,artist,duration) in [("Gotcha","鹭卓",153.0),("不死鸟","霍尊",265.772),("兰香如故","叶炫清",215.0)] {
            let start=std::time::Instant::now();
            let result=fetch_qqmusic("https://c.y.qq.com",track,artist,duration);
            println!("{track}: {:.2}s {:?}",start.elapsed().as_secs_f64(),result.as_ref().map(|lyrics|(lyrics.synced,lyrics.lines.len())));
            assert!(result.unwrap().synced);
        }
    }

    #[test]
    fn qq_recovers_from_a_transient_search_failure() {
        let search=serde_json::json!({"code":0,"data":{"song":{"list":[{"songmid":"testMID","songname":"Track","singer":[{"name":"Artist"}],"interval":215}]}}}).to_string();
        let lyric=serde_json::json!({"code":0,"lyric":"[00:01.00]第一句\n[00:05.00]第二句"}).to_string();
        let (base,worker)=serve(vec![("502 Bad Gateway",String::new()),("200 OK",search),("200 OK",lyric)]);
        let result=fetch_qqmusic(&base,"Track","Artist",215.0).unwrap();
        assert!(result.synced);assert_eq!(result.lines.len(),2);
        worker.join().unwrap();
    }

    #[test]
    fn qq_timeout_retries_once_and_permanent_errors_stay_errors() {
        use std::{io::{Read,Write},net::TcpListener,time::Duration};
        let listener=TcpListener::bind("127.0.0.1:0").unwrap();
        let url=format!("http://{}",listener.local_addr().unwrap());
        let worker=std::thread::spawn(move||{
            let (mut stalled,_)=listener.accept().unwrap();
            let mut buffer=[0;4096];stalled.read(&mut buffer).unwrap();
            // 保持第一条连接不响应，第二次请求才返回，真实覆盖五秒读取超时。
            let (mut recovered,_)=listener.accept().unwrap();
            recovered.read(&mut buffer).unwrap();
            write!(recovered,"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}").unwrap();
        });
        let started=std::time::Instant::now();
        assert!(request_json_with_retry(&url,&url,true).is_ok());
        assert!(started.elapsed()<Duration::from_secs(8));worker.join().unwrap();
        for responses in [vec![("503 Service Unavailable",String::new());2],vec![("403 Forbidden",String::new())]] {
            let (base,worker)=serve(responses);
            assert!(matches!(request_json_with_retry(&base,&base,true),Err(FetchError::Unavailable(_))));
            worker.join().unwrap();
        }
    }

    #[test]
    #[ignore="需要本机酷狗已缓存这两首歌的原生歌词，用于复现跨来源时间戳偏差"]
    fn live_kugou_uses_native_timestamps(){
        for (title,artist,duration,text,expected) in [("一生有你","水木年华",258.951,"因为梦见你离开",22.991f32),("口是心非","张雨生",292.673,"口是心非",65.434f32)]{
            let lyrics=super::fetch_for_source("kugou",title,artist,duration,None).unwrap();
            let line=lyrics.lines.iter().find(|line|line.text==text).unwrap();
            assert!((line.t-expected).abs()<0.002,"{title} 应跟随酷狗原生时间戳：实际 {}，原生 {expected}",line.t);
            println!("{title} 原生歌词时间戳校验通过：{expected:.3}s");
        }
    }
    use super::*;

    #[test]
    #[ignore="需要本机酷狗已缓存《谁伴我闯荡》，校验实际短停顿"]
    fn live_kugou_short_pause_stays_on_previous_line() {
        let (l,waiting)=fetch_for_display("kugou","谁伴我闯荡","BEYOND",0.0,None,None).unwrap();
        assert!(!waiting,"应使用酷狗原生歌词");
        for t in [28.434,29.164,29.863] {
            assert_eq!(l.lines[l.index_at(t).unwrap()].text,"前面是那方谁伴我闯荡");
        }
        assert_eq!(l.lines[l.index_at(29.864).unwrap()].text,"沿路没有指引若我走上");
    }

    #[test]
    fn krc_short_gaps_hold_previous_line_without_changing_timing() {
        for gap in [20,390,1037,1984,2126,2999] {
            let next=3000+gap;
            let l=parse_krc(&format!("[1000,2000]<0,2000,0>上一句\n[{next},2000]<0,2000,0>下一句"));
            let next=next as f32/1000.0;
            assert!(l.index_at(0.9).is_none());
            for t in [3.0,3.0+gap as f32/2000.0,next-0.001] {
                assert_eq!(l.lines[l.index_at(t).unwrap()].text,"上一句","{gap} 毫秒的行间空隙应保留上一句");
            }
            assert_eq!(l.lines[l.index_at(next).unwrap()].text,"下一句");
            assert_eq!(l.lines[l.index_at(next+2.0).unwrap()].text,"");
        }
        for gap in [3000,8000] {
            let l=parse_krc(&format!("[1000,2000]上一句\n[{},2000]下一句",3000+gap));
            assert_eq!(l.lines[l.index_at(3.0).unwrap()].text,"");
        }
    }

    #[test]
    fn soda_krc_preserves_real_timing_and_instrumental_gaps() {
        let l=parse_krc("[25378,4523]<0,202,0>孤<202,201,0>单\n[34938,5563]<0,202,0>身<202,201,0>边");
        assert!(l.synced);
        assert!((l.lines[0].t-25.378).abs()<0.001);
        assert_eq!(l.lines[0].text,"孤单");
        assert_eq!(l.lines[l.index_at(30.0).unwrap()].text,"");
        assert_eq!(l.lines[l.index_at(35.0).unwrap()].text,"身边");
        assert!(!parse_krc("[bad,10]<0,10,0>错").synced);
        assert!(!parse_krc("纯文本歌词").synced);
        assert_eq!(parse_krc("[00:01.20]正文").lines[0].text,"正文");
    }


    #[test]
    #[ignore="需要实际访问外部歌词服务"]
    fn live_reference_track() {
        let result=fetch("Never Gonna Give You Up","Rick Astley",213.0);
        println!("reference: {:?}",result.as_ref().map(|l|(l.synced,l.lines.len())));
        assert!(result.is_ok());
    }

    #[test]
    #[ignore="需要实际访问外部歌词服务"]
    fn live_web_player_track() {
        let result=fetch("スパークル (movie ver.)","RADWIMPS",534.0);
        println!("web-player: {:?}",result.as_ref().map(|l|(l.synced,l.lines.len())));
        assert!(result.unwrap().synced);
    }

    #[test]
    #[ignore="需要实际访问外部歌词服务"]
    fn live_chinese_tracks() {
        for (title,artist,duration) in [("稀客","杨千嬅",190.0),("阿拉斯加海湾","蓝心羽",241.0)] {
            let lyrics=fetch_with_sources(Some("https://music.163.com"),Some("https://c.y.qq.com"),Some("https://lrclib.net"),title,artist,duration).unwrap();
            assert!(lyrics.synced);
            let first=lyrics.index_at(30.0).unwrap();let later=lyrics.index_at(90.0).unwrap();
            println!("{title} / {artist}: {} 条时间戳，30 秒行号 {first}，90 秒行号 {later}",lyrics.lines.len());
            assert!(later>first,"歌词应随播放进度前进");
        }
    }

    #[test]
    fn matching_rejects_covers_wrong_titles_and_other_versions() {
        let items=serde_json::json!([
            {"id":1,"name":"Track","artists":[{"name":"Cover Artist"}],"duration":120000},
            {"id":2,"name":"Different Track","artists":[{"name":"Artist"}],"duration":120000},
            {"id":3,"name":"Track (Live)","artists":[{"name":"Artist"}],"duration":120000},
            {"id":4,"name":"Track","artists":[{"name":"Artist"}],"duration":200000},
            {"id":5,"name":"Track","artists":[{"name":"Artist"}],"duration":121000},
            {"id":6,"name":"Track","ar":[{"name":"Artist"}],"dt":122000}
        ]);
        assert_eq!(netease_candidates(items.as_array().unwrap(),"Track","Artist",120.0),vec![5,6]);
    }

    fn serve(responses:Vec<(&'static str,String)>)->(String,std::thread::JoinHandle<()>) {
        use std::{io::{Read,Write},net::TcpListener};
        let listener=TcpListener::bind("127.0.0.1:0").unwrap();
        let base=format!("http://{}",listener.local_addr().unwrap());
        let worker=std::thread::spawn(move||for (status,body) in responses {
            let (mut stream,_)=listener.accept().unwrap();
            let mut buf=[0;4096];let _=stream.read(&mut buf).unwrap();
            write!(stream,"HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        });
        (base,worker)
    }

    #[test]
    fn fallback_checks_identity_even_when_wrong_recording_has_exact_duration(){
        let candidates=serde_json::json!([
            {"id":1,"name":"合唱","artists":[{"name":"甲"},{"name":"丙"}],"duration":120000},
            {"id":2,"name":"合唱","artists":[{"name":"乙"},{"name":"甲"}],"duration":120000}
        ]);
        assert_eq!(netease_candidates(candidates.as_array().unwrap(),"合唱","甲/乙",120.0),vec![2]);
        for (name,artist) in [("Other Track","Artist"),("Track (Live)","Artist"),("Track (movie ver.)","Artist"),("Track","Cover Artist"),("Track","")] {
            let rows=serde_json::json!([{"trackName":name,"artistName":artist,"duration":120,"syncedLyrics":"[00:01.00]不应采用"}]);
            let (base,worker)=serve(vec![("200 OK",rows.to_string())]);
            assert_eq!(fetch_from(&base,"Track","Artist",120.0).unwrap_err(),FetchError::NotFound);worker.join().unwrap();
        }
        let rows=serde_json::json!([{"trackName":"Track","artistName":"乙、甲","duration":120,"syncedLyrics":"[00:01.00]正确歌词"}]);
        let (base,worker)=serve(vec![("200 OK",rows.to_string())]);
        assert_eq!(fetch_from(&base,"Track","甲/乙",120.0).unwrap().lines[0].text,"正确歌词");worker.join().unwrap();
    }

    #[test]
    fn online_resolver_uses_a_working_source_when_the_other_is_down() {
        let search=serde_json::json!({"code":200,"result":{"songs":[{"id":5,"name":"Track","artists":[{"name":"Artist"}],"duration":120000}]}}).to_string();
        let lyric=serde_json::json!({"code":200,"lrc":{"lyric":"[00:01.00]first\n[00:05.00]second"}}).to_string();
        let (netease,worker)=serve(vec![("200 OK",search),("200 OK",lyric)]);
        // 不可达的备用地址不应影响主源正常返回。
        assert!(fetch_with_sources(Some(&netease),Some("http://127.0.0.1:1"),Some("http://127.0.0.1:1"),"Track","Artist",120.0).unwrap().synced);
        worker.join().unwrap();
        let (netease,first)=serve(vec![("503 Service Unavailable",String::new())]);
        let (lrclib,second)=serve(vec![("200 OK",r#"[{"trackName":"Track","artistName":"Artist","duration":120,"syncedLyrics":"[00:01.00]first\n[00:05.00]second"}]"#.into())]);
        assert!(fetch_with_sources(Some(&netease),Some("http://127.0.0.1:1"),Some(&lrclib),"Track","Artist",120.0).unwrap().synced);
        first.join().unwrap();second.join().unwrap();
    }

    #[test]
    fn netease_song_id_fetches_lyrics_without_a_search_request(){
        let lyric=serde_json::json!({"code":200,"lrc":{"lyric":"[00:01.00]第一句\n[00:05.00]第二句"}}).to_string();
        let (base,worker)=serve(vec![("200 OK",lyric)]);
        let lyrics=fetch_netease_song(&base,123).unwrap();
        assert!(lyrics.synced);assert_eq!(lyrics.lines.len(),2);worker.join().unwrap();
    }

    #[test]
    fn cached_lyrics_preserve_timing_and_invalid_caches_are_ignored() {
        let path=std::env::temp_dir().join(format!("daozai-lrc-cache-{}-{:?}.lrc",std::process::id(),std::thread::current().id()));
        let expected=parse_lrc("[00:01.234]first\n[00:12.567]second\n[01:03.891]third");
        write_cache(&path,&expected);
        let cached=read_cache(&path).unwrap();
        assert_eq!(cached.lines.len(),expected.lines.len());
        for (a,b) in cached.lines.iter().zip(&expected.lines) {
            assert_eq!(a.text,b.text);assert!((a.t-b.t).abs()<0.002);
        }
        std::fs::write(&path,"broken cache").unwrap();
        assert!(read_cache(&path).is_none());
        let _=std::fs::remove_file(path);
    }

    #[test]
    fn parser_keeps_fractional_colon_times_and_rejects_invalid_timing() {
        let lyrics=parse_lrc("[00:12:35]first\n[00:NaN]invalid\n[00:61.00]invalid\n[00:12:35:10]invalid");
        assert_eq!(lyrics.lines.len(),1);assert!((lyrics.lines[0].t-12.35).abs()<0.001);
    }

    #[test]
    fn parses_and_sorts_lrc() {
        let raw = "[00:12.50]second\n[00:01.00]first\n[00:20:00]third";
        let l = parse_lrc(raw);
        assert_eq!(l.lines.len(), 3);
        assert_eq!(l.lines[0].text, "first");
        assert_eq!(l.lines[1].text, "second");
        assert!((l.lines[2].t - 20.0).abs() < 1e-3);
        assert!(l.synced);
    }

    #[test]
    fn index_at_tracks_position() {
        let l = parse_lrc("[00:01.00]a\n[00:05.00]b\n[00:09.00]c");
        assert_eq!(l.index_at(0.0), None);
        assert_eq!(l.index_at(6.0), Some(1));
        assert_eq!(l.index_at(100.0), Some(2));
    }

    #[test]
    fn plain_lyrics_have_no_invented_timing() {
        let l = from_plain("one\ntwo\n\nthree");
        assert_eq!(l.lines.len(), 3);
        assert!(!l.synced);
        assert!(l.lines.iter().all(|line|line.t==0.0));
    }

    #[test]
    fn title_fallback_keeps_meaningful_versions() {
        assert_eq!(clean_title("スパークル (movie ver.)"),"スパークル (movie ver.)");
        assert_eq!(clean_title("Track (Official Music Video)"),"Track");
        assert_eq!(clean_title("Track (Live)"),"Track (Live)");
    }

    #[test]
    fn matching_prefers_synced_lyrics_and_rejects_other_versions() {
        let items=serde_json::json!([
            {"duration":534,"plainLyrics":"plain"},
            {"duration":535,"syncedLyrics":"[00:01.00]first\n[00:05.00]second"},
            {"duration":200,"syncedLyrics":"[00:01.00]wrong version"}
        ]);
        let result=lyrics_from_items(items.as_array().unwrap(),"Track",534.0).unwrap();
        assert!(result.synced);assert_eq!(result.lines[0].text,"first");
        assert_eq!(lyrics_from_items(&[items[2].clone()],"Track",534.0).unwrap_err(),FetchError::NotFound);
    }

    #[test]
    fn http_search_and_network_errors_are_distinct_from_missing_lyrics() {
        use std::{io::{Read,Write},net::TcpListener};
        for (status,body,expected) in [("200 OK",r#"[{"trackName":"Track Name","artistName":"Artist","duration":120,"syncedLyrics":"[00:01.00]first"}]"#,0),("200 OK","[]",1),("502 Bad Gateway","",2)] {
            let listener=TcpListener::bind("127.0.0.1:0").unwrap();
            let base=format!("http://{}",listener.local_addr().unwrap());
            let worker=std::thread::spawn(move||{
                let (mut stream,_)=listener.accept().unwrap();
                let mut buf=[0;4096];let n=stream.read(&mut buf).unwrap();
                let request=String::from_utf8_lossy(&buf[..n]);
                assert!(request.contains("track_name=Track%20Name&artist_name=Artist"));
                write!(stream,"HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            });
            let result=fetch_from(&base,"Track Name","Artist",120.0);
            worker.join().unwrap();
            match expected {0=>assert!(result.unwrap().synced),1=>assert_eq!(result.unwrap_err(),FetchError::NotFound),_=>assert!(matches!(result,Err(FetchError::Unavailable(_))))}
        }
    }

    #[test]
    fn netease_text_lyrics_with_timed_credits_are_not_treated_as_synced(){
        // 网易云「暂无自动滚动」：只有署名带时间，正文没有时间戳。
        let raw="[00:00.000] 作词 : 陈玥凝\n[00:01.000] 作曲 : 何佩欣\n第一句正文\n第二句正文\n";
        let lyrics=lyrics_from_text(raw);
        assert!(!lyrics.synced,"只有署名带时间时不能在岛上冒充同步歌词");
        assert_eq!(lyrics.lines.iter().map(|l|l.text.as_str()).collect::<Vec<_>>(),["作词 : 陈玥凝","作曲 : 何佩欣","第一句正文","第二句正文"]);
        let json="{\"t\":0,\"c\":[{\"tx\":\"作词: \"},{\"tx\":\"某人\"}]}\n正文";
        assert_eq!(lyrics_from_text(json).lines.iter().map(|l|l.text.as_str()).collect::<Vec<_>>(),["正文"]);
        let synced=lyrics_from_text("[00:00.00] 作词 : 甲\n[00:12.00]第一句\n[00:15.00]第二句");
        assert!(synced.synced && synced.lines.len()==3,"正常同步歌词保留署名行与时间");
    }

    #[test]
    fn netease_plain_only_song_falls_back_to_other_sources_for_synced_lyrics(){
        let search=serde_json::json!({"code":200,"result":{"songs":[{"id":7,"name":"Track","artists":[{"name":"Artist"}],"duration":120000}]}}).to_string();
        let plain=serde_json::json!({"code":200,"lrc":{"lyric":"只有文本\n第二行"}}).to_string();
        let (netease,first)=serve(vec![("200 OK",search),("200 OK",plain)]);
        let rows=serde_json::json!([{"trackName":"Track","artistName":"Artist","duration":120,"syncedLyrics":"[00:01.00]同步歌词"}]).to_string();
        let (lrclib,second)=serve(vec![("200 OK",rows)]);
        let lyrics=fetch_with_sources(Some(&netease),Some("http://127.0.0.1:1"),Some(&lrclib),"Track","Artist",120.0).unwrap();
        assert!(lyrics.synced);assert_eq!(lyrics.lines[0].text,"同步歌词");
        first.join().unwrap();second.join().unwrap();
        // 其他来源都没有同步歌词时退回网易云的文本歌词，不报错。
        let search=serde_json::json!({"code":200,"result":{"songs":[{"id":7,"name":"Track","artists":[{"name":"Artist"}],"duration":120000}]}}).to_string();
        let plain=serde_json::json!({"code":200,"lrc":{"lyric":"只有文本"}}).to_string();
        let (netease,worker)=serve(vec![("200 OK",search),("200 OK",plain)]);
        let lyrics=fetch_with_sources(Some(&netease),None,None,"Track","Artist",120.0).unwrap();
        assert!(!lyrics.synced);assert_eq!(lyrics.lines[0].text,"只有文本");worker.join().unwrap();
    }

    #[test]
    fn netease_search_retries_with_title_only_when_combined_query_misses(){
        let empty=serde_json::json!({"code":200,"result":{"songCount":0}}).to_string();
        let found=serde_json::json!({"code":200,"result":{"songs":[{"id":9,"name":"Track","artists":[{"name":"甲"},{"name":"乙"}],"duration":120000}]}}).to_string();
        let lyric=serde_json::json!({"code":200,"lrc":{"lyric":"[00:01.00]正确歌词"}}).to_string();
        let (base,worker)=serve(vec![("200 OK",empty),("200 OK",found),("200 OK",lyric)]);
        assert!(fetch_netease(&base,"Track","甲 / 乙",120.0).unwrap().synced);worker.join().unwrap();
        let (base,worker)=serve(vec![("200 OK",serde_json::json!({"code":200,"nolyric":true}).to_string())]);
        assert_eq!(fetch_netease_song(&base,1).unwrap_err(),FetchError::NotFound);worker.join().unwrap();
    }

    #[test]
    fn same_song_ignores_separator_and_feature_spelling_but_not_other_tracks(){
        assert!(same_song("花见渡月","何佩欣","花见渡月","何佩欣"));
        assert!(same_song("Track","甲 / 乙","Track","乙/甲"));
        assert!(same_song("Track (feat. 乙)","甲","Track","甲、乙"));
        assert!(same_song("Track","","Track","甲"));
        assert!(!same_song("Track (Live)","甲","Track","甲"));
        assert!(!same_song("Track","甲","Track","丙"));
    }

    #[test]
    fn song_relation_separates_fill_ins_from_other_songs(){
        assert_eq!(song_relation("Track","甲","Track","甲"),Some(true));
        assert_eq!(song_relation("Track","","Track","甲"),Some(true),"歌手从空补全仍是同一首");
        assert_eq!(song_relation("Track","甲 / 乙","Track","乙、甲"),Some(true));
        assert_eq!(song_relation("有点甜","汪苏泷","有点甜","汪苏泷、BY2"),Some(false),"单人署名补全为合唱");
        assert_eq!(song_relation("有点甜","汪苏泷、BY2","有点甜","汪苏泷"),Some(false));
        assert_eq!(song_relation("Track","甲","Track","丙"),None);
        assert_eq!(song_relation("Track","甲、乙","Track","甲、丙"),None,"部分重叠不算补全");
        assert_eq!(song_relation("Track (Live)","甲","Track","甲"),None,"不同版本是不同歌曲");
        assert_eq!(song_relation("","甲","","甲"),None);
    }

    #[test]
    fn confirmed_missing_lyrics_are_remembered_for_a_week_but_failures_are_not(){
        let path=std::env::temp_dir().join(format!("daozai-negative-{}-{:?}.lrc",std::process::id(),std::thread::current().id()));
        let _=std::fs::remove_file(negative_path(&path));
        assert!(read_negative(&path).is_none());
        write_negative(&path,None);
        assert_eq!(read_negative(&path).unwrap().unwrap_err(),FetchError::NotFound);
        write_negative(&path,Some(&from_plain("第一行\n第二行")));
        let plain=read_negative(&path).unwrap().unwrap();
        assert!(!plain.synced);assert_eq!(plain.lines.len(),2);
        // 超过 7 天视为过期，重新联网查询。
        let file=std::fs::OpenOptions::new().write(true).open(negative_path(&path)).unwrap();
        file.set_modified(std::time::SystemTime::now()-NEGATIVE_TTL-std::time::Duration::from_secs(60)).unwrap();drop(file);
        assert!(read_negative(&path).is_none());
        std::fs::remove_file(negative_path(&path)).unwrap();
        // 有来源断网时标记失败，调用方不会把「没找到」缓存下来。
        let rows=serde_json::json!([]).to_string();
        let (lrclib,worker)=serve(vec![("200 OK",rows)]);
        let (result,failed)=fetch_with_sources_detail(Some("http://127.0.0.1:1"),None,Some(&lrclib),"Track","Artist",120.0);
        assert!(result.is_err());assert!(failed,"网易云不可达时不能确认没有歌词");worker.join().unwrap();
        let (lrclib,worker)=serve(vec![("200 OK","[]".into())]);
        let (result,failed)=fetch_with_sources_detail(None,None,Some(&lrclib),"Track","Artist",120.0);
        assert_eq!(result.unwrap_err(),FetchError::NotFound);assert!(!failed);worker.join().unwrap();
    }

    #[test]
    fn manual_retry_skips_remembered_missing_lyrics_only_during_that_query(){
        let path=std::env::temp_dir().join(format!("daozai-manual-{}-{:?}.lrc",std::process::id(),std::thread::current().id()));
        write_negative(&path,None);
        assert!(read_negative(&path).is_some());
        {
            let _skip=SkipNegative::set(true);
            assert!(read_negative(&path).is_none(),"手动重试应跳过无歌词记录");
        }
        assert!(read_negative(&path).is_some(),"查询结束后恢复使用无歌词记录");
        std::fs::remove_file(negative_path(&path)).unwrap();
    }

    #[test]
    fn lookup_steps_are_traced_for_the_local_diagnostic_log(){
        take_trace();
        let (lrclib,worker)=serve(vec![("200 OK","[]".into())]);
        let _=fetch_with_sources_detail(None,None,Some(&lrclib),"Track","Artist",120.0);worker.join().unwrap();
        assert_eq!(take_trace(),vec!["LRCLIB:未找到".to_string()]);
        for _ in 0..100 {trace(||"步骤".into());}
        assert_eq!(take_trace().len(),TRACE_LIMIT,"单次记录有上限");
        assert!(take_trace().is_empty());
    }

    #[test]
    fn soda_fingerprint_is_cheap_metadata_only(){
        // 没有汽水音乐时返回 None，轮询照常读取（读取本身也会立即失败）。
        if std::env::var_os("APPDATA").is_none(){assert!(native_fingerprint("汽水音乐").is_none());}
        assert!(native_fingerprint("kugou").is_none(),"酷狗目录的修改时间不可靠，不做跳过");
    }

    #[test]
    fn korean_and_japanese_romanization_is_recognized_without_flagging_real_lyrics(){
        let roman=parse_lrc("[00:01.00]Hoksi ireon na ttaeme himdeureonni\n[00:04.00]Amu daedap eomneun neo\n[00:07.00]Babocheoreom wae\n[00:10.00]Neoreul jiuji mothae");
        let hangul=parse_lrc("[00:01.00]혹시 이런 나 땜에 힘들었니\n[00:04.00]아무 대답 없는 너\n[00:07.00]바보처럼 왜\n[00:10.00]너를 지우지 못해");
        let mixed=parse_lrc("[00:01.00]Baby you know 너를 지우지 못해\n[00:04.00]I just can't let you go 바보처럼 왜\n[00:07.00]Oh oh oh yeah yeah\n[00:10.00]아무 대답 없는 너");
        assert!(romanized("눈,코,입 (EYES,NOSE,LIPS)",&roman));
        assert!(!romanized("눈,코,입 (EYES,NOSE,LIPS)",&hangul));
        assert!(!romanized("눈,코,입",&mixed),"夹杂英文的韩文歌词仍是原文");
        assert!(!romanized("Dynamite",&roman),"英文歌名的歌曲不做判断，避免把英文歌当成音译");
        assert!(romanized("さくらんぼ",&parse_lrc("[00:01.00]Ai shiteru no hibiki dake de\n[00:05.00]Tsuyoku nareru ki ga shita yo\n[00:09.00]Sasayaka na shiawase")));
        assert!(!romanized("눈,코,입",&parse_lrc("[00:01.00]Eyes")),"字数太少不判断");
    }

    #[test]
    fn romanized_lyrics_are_only_a_fallback_and_never_cached(){
        let title="눈,코,입 (EYES,NOSE,LIPS)";
        let roman=r#"[{"trackName":"눈,코,입 (EYES,NOSE,LIPS)","artistName":"TAEYANG","duration":229,"syncedLyrics":"[00:01.00]Hoksi ireon na ttaeme himdeureonni\n[00:04.00]Amu daedap eomneun neo\n[00:07.00]Babocheoreom wae neoreul jiuji mothae"}]"#;
        let both=r#"[{"trackName":"눈,코,입 (EYES,NOSE,LIPS)","artistName":"TAEYANG","duration":229,"syncedLyrics":"[00:01.00]Hoksi ireon na ttaeme himdeureonni\n[00:04.00]Amu daedap eomneun neo\n[00:07.00]Babocheoreom wae neoreul jiuji mothae"},
            {"trackName":"눈,코,입 (EYES,NOSE,LIPS)","artistName":"TAEYANG","duration":235,"syncedLyrics":"[00:01.00]혹시 이런 나 땜에 힘들었니\n[00:04.00]아무 대답 없는 너"}]"#;
        // 同一来源里原文优先，即使罗马音版本时长更接近。
        let (base,worker)=serve(vec![("200 OK",both.into())]);
        assert_eq!(fetch_from(&base,title,"TAEYANG",229.0).unwrap().lines[0].text,"혹시 이런 나 땜에 힘들었니");worker.join().unwrap();
        // 只有罗马音时仍可显示，作为最后的备选。
        let (base,worker)=serve(vec![("200 OK",roman.into())]);
        let (result,_)=fetch_with_sources_detail(None,None,Some(&base),title,"TAEYANG",229.0);
        assert!(romanized(title,&result.unwrap()));worker.join().unwrap();
        // 前面的来源（网易云）给出罗马音时，继续向后（QQ 音乐）找原文。
        let search=serde_json::json!({"code":200,"result":{"songs":[{"id":3,"name":title,"artists":[{"name":"TAEYANG"}],"duration":229000}]}}).to_string();
        let lyric=serde_json::json!({"code":200,"lrc":{"lyric":"[00:01.00]Hoksi ireon na ttaeme himdeureonni\n[00:04.00]Amu daedap eomneun neo\n[00:07.00]Babocheoreom wae neoreul jiuji mothae"}}).to_string();
        let (netease,first)=serve(vec![("200 OK",search),("200 OK",lyric)]);
        let search=serde_json::json!({"code":0,"data":{"song":{"list":[{"songmid":"abc","songname":title,"singer":[{"name":"TAEYANG"}],"interval":229}]}}}).to_string();
        let lyric=serde_json::json!({"code":0,"lyric":"[00:01.00]혹시 이런 나 땜에 힘들었니\n[00:04.00]아무 대답 없는 너"}).to_string();
        let (qq,second)=serve(vec![("200 OK",search),("200 OK",lyric)]);
        take_trace();
        let (result,_)=fetch_with_sources_detail(Some(&netease),Some(&qq),None,title,"TAEYANG",229.0);
        assert_eq!(result.unwrap().lines[0].text,"혹시 이런 나 땜에 힘들었니");first.join().unwrap();second.join().unwrap();
        assert!(take_trace()[0].ends_with("(罗马音)"),"诊断日志标明罗马音");
        // 已缓存的罗马音版本会被忽略。
        let path=std::env::temp_dir().join(format!("daozai-roman-{}.lrc",std::process::id()));
        write_cache(&path,&parse_lrc("[00:01.00]Hoksi ireon na ttaeme himdeureonni\n[00:04.00]Amu daedap eomneun neo\n[00:07.00]Babocheoreom wae neoreul jiuji mothae"));
        assert!(read_cache(&path).filter(|lyrics|!romanized(title,lyrics)).is_none());std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn urlencode_escapes_spaces_and_symbols() {
        assert_eq!(urlencode("a b&c"), "a%20b%26c");
    }
}
