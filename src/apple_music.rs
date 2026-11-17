//! Apple Music 系统会话中的歌手与专辑字段。
pub(super) fn is_source(source:&str)->bool{
    source.eq_ignore_ascii_case("AppleMusic.exe") || source.eq_ignore_ascii_case("AppleInc.AppleMusicWin_nzyj5cx40ttqa!App")
}

pub(super) fn metadata(artist:String,album:String)->(String,String){
    if let Some((singer,record))=artist.split_once(" — "){
        let (singer,record)=(singer.trim(),record.trim());
        if !singer.is_empty() && !record.is_empty(){
            if album.trim().is_empty(){return(singer.into(),record.into());}
            if record==album.trim(){return(singer.into(),album);}
        }
    }
    (artist,album)
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn combined_display_field_does_not_become_a_lyric_search_artist(){
        for (artist,album,singer,record) in [
            ("王菲 — 只爱陌生人","","王菲","只爱陌生人"),
            ("任贤齐 — 台湾男儿任贤齐认真精选辑","","任贤齐","台湾男儿任贤齐认真精选辑"),
            ("孙燕姿 — 未完成","未完成","孙燕姿","未完成"),
            ("Artist — Album — Deluxe","","Artist","Album — Deluxe"),
            ("王菲","只爱陌生人","王菲","只爱陌生人"),
            ("Artist — Alias","Other Album","Artist — Alias","Other Album"),
            (" — Album",""," — Album",""),("Artist — ","","Artist — ",""),
        ]{assert_eq!(metadata(artist.into(),album.into()),(singer.into(),record.into()));}
        assert!(is_source("AppleMusic.exe"));assert!(is_source("AppleInc.AppleMusicWin_nzyj5cx40ttqa!App"));
        assert!(!is_source("AppleInc.AppleMusicWin_nzyj5cx40ttqa!App.other"));assert!(!is_source("msedge.exe"));
    }
}
