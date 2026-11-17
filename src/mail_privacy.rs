//! Display privacy only: hiding a code never asserts that the issuer expired it.
use std::collections::HashMap;
use crate::mail::Item;

#[derive(Default)]
pub(crate) struct Privacy { seen:HashMap<(String,u32,String),Seen>, revealed:HashMap<(String,u32,String),f64>, bodies:std::cell::RefCell<HashMap<(String,u32,String),(std::sync::Arc<str>,std::sync::Arc<str>)>> }
struct Seen { at:f64, age:Option<u64>, new:bool, lifetime:Option<u64> }
fn key(item:&Item)->(String,u32,String){(item.account.to_ascii_lowercase(),item.uid,item.code.clone().unwrap_or_default())}

/// Accept only explicit relative durations in the same short sentence as the code.
/// Absolute dates, generic login/session durations, conflicting values and vague prose fall back.
pub fn explicit_lifetime(subject:&str,body:&str,code:&str)->Option<u64>{
    if code.is_empty(){return None;}
    use std::sync::OnceLock;
    static RE:OnceLock<regex::Regex>=OnceLock::new();
    let re=RE.get_or_init(||regex::Regex::new(r"(?i)(?:有效(?:时间|期)?(?:为|是)?\s*[:：]?\s*|valid\s+for\s+|expires?\s+in\s+)([0-9]{1,3})\s*(秒(?:钟)?|分钟|小时|seconds?|minutes?|hours?)|([0-9]{1,3})\s*(秒(?:钟)?|分钟|小时)\s*(?:内有效|后失效)").unwrap());
    let mut values=Vec::new();
    for sentence in subject.split(['。','!','！','\n',';','；']).chain(body.split(['。','!','！','\n',';','；'])) {
        if sentence.chars().count()>240 || !sentence.contains(code){continue;}
        let lower=sentence.to_ascii_lowercase();
        if !(sentence.contains("验证码")||lower.contains("verification code")||lower.contains("one-time")||lower.contains("otp")){continue;}
        for c in re.captures_iter(sentence) {
            let start=c.get(0)?.start();
            let Some(code_end)=sentence[..start].rfind(code).map(|i|i+code.len()) else{continue;};
            let gap=sentence[code_end..start].trim_matches(|c:char|c.is_whitespace()||",，:：.。".contains(c)).to_ascii_lowercase();
            // A nearby membership/session lifetime is not the code's lifetime.
            if !["","的","is","will","该验证码","此验证码","验证码","this code is","请于","请在"].contains(&gap.as_str()){return None;}
            let n=c.get(1).or_else(||c.get(3))?.as_str().parse::<u64>().ok()?;
            let unit=c.get(2).or_else(||c.get(4))?.as_str().to_ascii_lowercase();
            let multiplier=if unit.starts_with('分')||unit.starts_with("minute"){60}else if unit.starts_with('小')||unit.starts_with("hour"){3600}else{1};
            let seconds=n.checked_mul(multiplier)?;
            if seconds==0||seconds>3600{return None;}
            values.push(seconds);
        }
    }
    let first=*values.first()?;
    values.iter().all(|v|*v==first).then_some(first)
}

pub fn mask(text:&str,code:&str)->String{
    if code.is_empty(){return text.into();}
    // 提取器会归一化全角和零宽字符；遮挡时也匹配原文中的这些写法。
    let pattern=code.chars().filter(|c|c.is_ascii_alphanumeric()).map(|c|{
        format!("[{c}{}]",char::from_u32(c as u32+0xfee0).unwrap())
    }).collect::<Vec<_>>().join(r"[\s\-\u{ff0d}\u{200b}\u{200c}\u{200d}\u{feff}\u{2060}]*");
    if pattern.is_empty(){return text.replace(code,"••••••");}
    regex::RegexBuilder::new(&pattern).case_insensitive(true).build().map(|r|r.replace_all(text,"••••••").into_owned()).unwrap_or_else(|_|text.replace(code,"••••••"))
}

impl Privacy {
    pub fn masked_body(&self,item:&Item)->std::sync::Arc<str>{
        let k=key(item);let mut bodies=self.bodies.borrow_mut();
        if let Some((original,masked))=bodies.get(&k){if std::sync::Arc::ptr_eq(original,&item.summary){return masked.clone();}}
        let masked:std::sync::Arc<str>=mask(&item.summary,item.code.as_deref().unwrap_or("")).into();
        if bodies.len()>=64{bodies.clear();}
        bodies.insert(k,(item.summary.clone(),masked.clone()));masked
    }
    pub fn observe(&mut self,item:&Item,new:bool,at:f64,unix:i64){
        let Some(code)=&item.code else{return;};
        let seen=self.seen.entry(key(item)).or_insert_with(||Seen{
            at,new,age:item.received_unix.filter(|t|*t>0&&*t<=unix).map(|t|(unix-t) as u64),
            lifetime:explicit_lifetime(&item.subject,&item.summary,code),
        });
        seen.new|=new;
    }
    pub fn hidden(&self,item:&Item,now:f64,fallback:u32)->bool{
        if item.code.is_none(){return false;}
        let k=key(item);
        if self.revealed.get(&k).is_some_and(|until|now<*until){return false;}
        let Some(s)=self.seen.get(&k) else{return true;};
        if !s.new{return true;}
        let elapsed=(now-s.at).max(0.0);
        let limit=(fallback.clamp(15,300) as u64).min(s.lifetime.unwrap_or(u64::MAX));
        s.age.unwrap_or(0) as f64+elapsed>=limit as f64
    }
    pub fn reveal(&mut self,item:&Item,now:f64){self.revealed.insert(key(item),now+15.0);}
    pub fn retain<'a>(&mut self,items:impl IntoIterator<Item=&'a Item>){
        let keys:std::collections::HashSet<_>=items.into_iter().filter(|i|i.code.is_some()).map(key).collect();
        self.seen.retain(|k,_|keys.contains(k));self.revealed.retain(|k,_|keys.contains(k));
        self.bodies.get_mut().retain(|k,_|keys.contains(k));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn item()->Item{Item{account:"test@example.com".into(),from:"Example".into(),subject:"验证码 123456 有效期为30秒".into(),time:String::new(),received_unix:Some(100),code:Some("123456".into()),html:String::new().into(),summary:"验证码 123456 有效期为30秒".into(),links:vec![],meeting:None,uid:1}}
    #[test] fn new_event_after_a_snapshot_reveals_the_code_once(){
        let item=item();let mut p=Privacy::default();
        p.observe(&item,false,0.0,100);assert!(p.hidden(&item,0.0,60));
        p.observe(&item,true,2.0,102);assert!(!p.hidden(&item,2.0,60));
        p.observe(&item,true,35.0,135);assert!(p.hidden(&item,35.0,60));
    }
    #[test] fn explicit_lifetime_requires_code_context_and_unambiguous_duration(){
        assert_eq!(explicit_lifetime("验证码 123456 有效期为30秒","","123456"),Some(30));
        assert_eq!(explicit_lifetime("","Your verification code 123456 is valid for 5 minutes.","123456"),Some(300));
        assert_eq!(explicit_lifetime("验证码 123456","会员有效期30分钟","123456"),None);
        assert_eq!(explicit_lifetime("验证码 123456，会员有效期30分钟","","123456"),None);
        assert_eq!(explicit_lifetime("验证码 123456 有效期为30秒，有效期为5分钟","","123456"),None);
        assert_eq!(explicit_lifetime("验证码 123456 有效期999小时","","123456"),None);
    }
    #[test] fn delay_history_reconnect_and_manual_reveal_never_extend_auto_visibility(){
        let i=item();let mut p=Privacy::default();p.observe(&i,true,0.0,120);
        assert!(!p.hidden(&i,9.0,60));assert!(p.hidden(&i,10.0,60));
        p.observe(&i,true,40.0,140);assert!(p.hidden(&i,40.0,60));
        p.reveal(&i,40.0);assert!(!p.hidden(&i,54.0,60));assert!(p.hidden(&i,55.0,60));
        let mut p=Privacy::default();p.observe(&i,false,0.0,100);assert!(p.hidden(&i,0.0,60));
    }
    #[test] fn fallback_is_configurable_and_masks_formatted_codes(){
        let mut i=item();i.subject="验证码 123456".into();i.summary="no validity stated".into();i.received_unix=None;
        let mut p=Privacy::default();p.observe(&i,true,0.0,100);
        assert!(!p.hidden(&i,14.0,15));assert!(p.hidden(&i,15.0,15));
        assert!(!mask("123456 and 1 2 3-4 5 6","123456").contains('1'));
    }
    #[test] fn masking_covers_normalized_code_variants(){
        for (text,code) in [("验证码 １２３４５６","123456"),("验证码 12\u{200b}3456","123456"),("验证码 123456","123 456"),("验证码 ＡＢＣ－１２３","ABC-123")] {
            assert_eq!(mask(text,code),"验证码 ••••••","归一化提取出的验证码必须遮住原文");
        }
    }
}
