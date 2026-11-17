//! 消息客户端的统一身份、通知策略和账号归属记录。
use crate::{model::Settings,paint::Card};
use eframe::egui;
use std::collections::HashMap;

#[cfg(windows)]
pub mod windows;
#[cfg(windows)]
mod qq;
#[cfg(windows)]
pub(crate) mod desktop;
#[cfg(windows)]
pub use windows::{Runtime,ToastAccounts,TaskbarEvent};

#[derive(Clone,Copy,Debug,PartialEq,Eq,Hash)]
pub enum Messenger {Wechat,Qq,WeCom,Feishu,DingTalk}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Capture {Taskbar,SystemToast}

/// 只注册已经实现的客户端；新增后端在这里声明识别信息和行为边界。
pub struct Adapter {
    pub client:Messenger,
    pub name:&'static str,
    pub capture:Capture,
    pub notification_id:&'static str,
    pub aliases:&'static [&'static str],
    pub display_names:&'static [&'static str],
    pub process_names:&'static [&'static str],
    pub publishers:&'static [&'static str],
    pub keep_unidentified:bool,
    pub dismiss_unread:bool,
    pub icon_scale:f32,
    pub collapsed_icon_scale:f32,
    pub(crate) enabled:fn(&Settings)->bool,
    pub(crate) tray_enabled:fn(&Settings)->bool,
    pub(crate) paint_icon:fn(&Card<'_>,f32,f32),
    #[cfg(windows)]
    pub(crate) native:&'static windows::NativeAdapter,
}

const WECHAT:Adapter=Adapter{
    client:Messenger::Wechat,name:"微信",capture:Capture::Taskbar,notification_id:"WeChat",
    aliases:&["Weixin","Tencent.WeChat","Tencent.Weixin"],display_names:&["微信","WeChat","Weixin"],process_names:&["Weixin.exe","WeChat.exe"],publishers:&["tencent"],
    keep_unidentified:false,dismiss_unread:false,icon_scale:1.0,collapsed_icon_scale:0.85,
    enabled:|settings|settings.wechat_notifications,tray_enabled:|settings|settings.wechat_tray_open,paint_icon:wechat_icon,
    #[cfg(windows)] native:&crate::wechat::ADAPTER,
};
const QQ:Adapter=Adapter{
    client:Messenger::Qq,name:"QQ",capture:Capture::SystemToast,notification_id:"QQ",
    aliases:&[],display_names:&["QQ"],process_names:&["QQ.exe"],publishers:&["tencent"],
    keep_unidentified:true,dismiss_unread:true,icon_scale:1.12,collapsed_icon_scale:0.92,
    enabled:|settings|settings.read_notifications,tray_enabled:|_|true,paint_icon:qq_icon,
    #[cfg(windows)] native:&qq::ADAPTER,
};

const WECOM:Adapter=Adapter{
    client:Messenger::WeCom,name:"企业微信",capture:Capture::Taskbar,notification_id:"WXWork",
    aliases:&["WeCom","WeWork","Tencent.WeWork"],display_names:&["企业微信","WXWork"],process_names:&["WXWork.exe"],publishers:&["tencent"],
    keep_unidentified:true,dismiss_unread:true,icon_scale:1.0,collapsed_icon_scale:0.85,
    enabled:|settings|settings.wecom_notifications,tray_enabled:|_|true,paint_icon:|c,x,y|crate::calendar_ui::source_icon(c,crate::calendar_ui::Source::Calendar(Some(crate::calendar::Provider::WeCom)),x-10.0,y-10.0,20.0),
    #[cfg(windows)] native:&desktop::WECOM,
};

const FEISHU:Adapter=Adapter{
    client:Messenger::Feishu,name:"飞书",capture:Capture::Taskbar,notification_id:"Feishu",
    aliases:&["com.electron.Feishu"],display_names:&["飞书","Feishu"],process_names:&["Feishu.exe"],publishers:&["beijing feishu technology","bytedance"],
    keep_unidentified:true,dismiss_unread:true,icon_scale:1.0,collapsed_icon_scale:0.85,
    enabled:|settings|settings.feishu_notifications,tray_enabled:|_|true,paint_icon:|c,x,y|crate::calendar_ui::source_icon(c,crate::calendar_ui::Source::Calendar(Some(crate::calendar::Provider::Feishu)),x-10.0,y-10.0,20.0),
    #[cfg(windows)] native:&desktop::FEISHU,
};

const DINGTALK:Adapter=Adapter{
    client:Messenger::DingTalk,name:"钉钉",capture:Capture::Taskbar,notification_id:"DingTalk",
    aliases:&[],display_names:&["钉钉","DingTalk"],process_names:&["DingTalk.exe"],publishers:&["alibaba","dingtalk"],
    keep_unidentified:true,dismiss_unread:true,icon_scale:1.0,collapsed_icon_scale:0.85,
    enabled:|settings|settings.dingtalk_notifications,tray_enabled:|_|true,paint_icon:|c,x,y|crate::calendar_ui::source_icon(c,crate::calendar_ui::Source::Calendar(Some(crate::calendar::Provider::DingTalk)),x-10.0,y-10.0,20.0),
    #[cfg(windows)] native:&desktop::DINGTALK,
};

impl Messenger {
    pub const ALL:[Self;5]=[Self::Wechat,Self::Qq,Self::WeCom,Self::Feishu,Self::DingTalk];
    pub const OFFICE:[Self;3]=[Self::WeCom,Self::Feishu,Self::DingTalk];
    pub fn adapter(self)->&'static Adapter{match self{Self::Wechat=>&WECHAT,Self::Qq=>&QQ,Self::WeCom=>&WECOM,Self::Feishu=>&FEISHU,Self::DingTalk=>&DINGTALK}}
    pub fn name(self)->&'static str{self.adapter().name}
    pub fn from_notification_id(id:&str)->Option<Self>{
        Self::ALL.into_iter().find(|client|{
            let adapter=client.adapter();let id=id.trim();
            adapter.notification_id.eq_ignore_ascii_case(id) || adapter.aliases.iter().any(|alias|alias.eq_ignore_ascii_case(id))
        })
    }
    pub fn enabled(self,settings:&Settings)->bool{settings.notifications && (self.adapter().enabled)(settings)}
}

pub fn taskbar_client(app:&str,id:&str)->Option<Messenger>{
    if let Some(client)=Messenger::from_notification_id(id){return (client.adapter().capture==Capture::Taskbar).then_some(client);}
    Messenger::ALL.into_iter().find(|client|{
        let adapter=client.adapter();adapter.capture==Capture::Taskbar
            && adapter.display_names.iter().any(|name|name.eq_ignore_ascii_case(app.trim()))
    })
}

pub fn toast_suppression(settings:&Settings)->u8{
    Messenger::ALL.into_iter().filter(|client|client.adapter().capture==Capture::Taskbar && (*client==Messenger::Wechat || (client.adapter().enabled)(settings)))
        .fold(0,|mask,client|mask|(1<<(client as u8)))
}
pub fn suppresses_toast(app:&str,id:&str,mask:u8)->bool{
    taskbar_client(app,id).is_some_and(|client|mask&(1<<(client as u8))!=0)
}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum NotificationFilter {All,Only(Messenger)}
impl NotificationFilter {
    pub fn for_settings(settings:&Settings)->Self{if settings.qq_only{Self::Only(Messenger::Qq)}else{Self::All}}
    pub fn allows(self,id:&str)->bool{match self{Self::All=>true,Self::Only(client)=>Messenger::from_notification_id(id)==Some(client)}}
}

/// 来源和原始窗口必须一起保留，未知账号不能在点击时绑定到剩余客户端。
#[derive(Clone,Copy,Debug,PartialEq,Eq,Hash)]
pub struct Unread {pub source:Messenger,pub pid:u32,pub window:isize}
impl Unread {
    pub fn of(source:Messenger)->Self{Self{source,pid:0,window:0}}
    pub fn unidentified(self)->bool{self.pid==0 && self.source.adapter().keep_unidentified}
    pub fn viewed_by(self,source:Messenger,pid:u32)->bool{
        self.source==source && (pid!=0 && self.pid==pid || !source.adapter().keep_unidentified && (pid==0 || self.pid==0))
    }
}

/// 进程创建时间用来区分进程号复用，不等同于客户端内部的登录账号标识。
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub struct Target {pub unread:Unread,pub(crate) started:u64}

#[derive(Debug,PartialEq,Eq)]
pub enum OpenError {Exited,Unidentified,Unavailable,Busy,Failed(String)}
impl OpenError {
    pub fn message(&self,client:Messenger)->String{match self{
        Self::Exited=>format!("{} 账号已退出",client.name()),
        Self::Unidentified=>format!("暂时无法确认此 {} 消息的账号",client.name()),
        Self::Unavailable=>format!("{} 窗口暂时无法打开",client.name()),
        Self::Busy=>"正在打开消息客户端，请稍后".into(),
        Self::Failed(message)=>message.clone(),
    }}
}

pub(crate) struct AccountHistory {
    client:Messenger,
    previous:Option<(i64,Option<Vec<Target>>)>,
    current:Option<(i64,Option<Vec<Target>>)>,
    single_since:Option<i64>,
    targets:HashMap<u32,(i64,Option<Target>)>,
}
impl AccountHistory {
    pub(crate) fn new(client:Messenger)->Self{Self{client,previous:None,current:None,single_since:None,targets:HashMap::new()}}
    pub(crate) fn observe(&mut self,at:i64,accounts:Option<Vec<Target>>){
        let accounts=accounts.filter(|accounts|accounts.iter().all(|target|target.unread.source==self.client));
        // 连续单账号区间可覆盖延迟到达的通知，多开、采样失败或长时间暂停后重新计时。
        self.single_since=match(self.current.as_ref(),accounts.as_ref()){
            (Some((last,Some(previous))),Some(current)) if at>*last && at.checked_sub(*last).is_some_and(|gap|gap<=30_000_000)=>{
                match(previous.as_slice(),current.as_slice()){([a],[b]) if a==b=>Some(self.single_since.unwrap_or(*last)),_=>None}
            },
            _=>None,
        };
        self.previous=self.current.take();self.current=Some((at,accounts));
    }
    pub(crate) fn target(&mut self,id:u32,created:i64,ready:impl FnOnce(Target)->bool)->Option<Target>{
        // 原地更新通知时可以降为未识别，但不能改绑到另一个账号。
        if let Some(&(at,target))=self.targets.get(&id){if at==created{
            let target=target.filter(|target|self.stable_account().is_some_and(|(_,_,current)|current==*target)).filter(|target|ready(*target));
            self.targets.insert(id,(created,target));return target;
        }}
        let target=(||{
            let (from,to,target)=self.stable_account()?;
            if created<=from || created>to{return None;}
            Some(target)
        })().filter(|target|ready(*target));
        self.targets.insert(id,(created,target));target
    }
    fn stable_account(&self)->Option<(i64,i64,Target)>{
        let (from,Some(previous))=self.previous.as_ref()? else{return None;};
        let (to,Some(current))=self.current.as_ref()? else{return None;};
        if to.checked_sub(*from)? > 30_000_000 || to<=from{return None;}
        match(previous.as_slice(),current.as_slice()){([a],[b]) if a==b=>Some((self.single_since?,*to,*a)),_=>None}
    }
    pub(crate) fn retain(&mut self,exists:impl Fn(u32,i64)->bool){self.targets.retain(|id,(created,_)|exists(*id,*created));}
}

fn wechat_icon(c:&Card,x:f32,y:f32){
    use egui::Color32;
    let ink=Color32::from_gray(24);let back=Color32::from_rgb(70,203,123);let front=Color32::from_rgb(145,235,165);
    c.fill(x-10.,y-7.,14.,11.,5.5,back);c.poly(&[(x-8.,y+1.),(x-8.,y+6.),(x-3.,y+2.)],back);
    for dx in [-6.,-1.]{c.circle(x+dx,y-3.,0.9,ink);}
    c.fill(x-2.,y-2.,13.,10.,5.,front);c.poly(&[(x+5.,y+6.),(x+9.,y+10.),(x+8.,y+5.)],front);
    for dx in [2.,7.]{c.circle(x+dx,y+1.,0.8,ink);}
}
fn qq_icon(c:&Card,x:f32,y:f32){
    let rect=c.rect(x-9.5,y-9.5,19.0,19.0);let icon=egui::Image::new(egui::include_image!("../assets/qq-client.png"));
    if let Ok(egui::load::TexturePoll::Ready{texture})=icon.load_for_size(c.ui.ctx(),rect.size()){
        c.painter.image(texture.id,rect,egui::Rect::from_min_max(egui::Pos2::ZERO,egui::pos2(1.0,1.0)),egui::Color32::WHITE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn qq_logo_has_no_pale_fringe_around_the_black_head(){
        let image=image::load_from_memory(include_bytes!("../assets/qq-client.png")).unwrap().into_rgba8();
        for y in image.height()/5..image.height()/3{
            let edge=(0..image.width()/2).find_map(|x|{let pixel=image.get_pixel(x,y);(pixel[3]>=128).then_some(pixel)}).unwrap();
            assert!(edge[0]<32 && edge[1]<32 && edge[2]<32,"QQ 黑色头部的外缘不能带浅色描边：{edge:?}");
        }
    }
    #[test]
    fn registered_adapters_have_distinct_identities_and_matching_backends(){
        for client in Messenger::ALL{
            let adapter=client.adapter();assert_eq!(adapter.client,client);
            assert_eq!(Messenger::from_notification_id(adapter.notification_id),Some(client));
            assert!(!adapter.process_names.is_empty());
            #[cfg(windows)]{
                assert_eq!(adapter.native.taskbar.is_some(),adapter.capture==Capture::Taskbar);
            }
        }
        assert_eq!(Messenger::from_notification_id(" qq "),Some(Messenger::Qq));
        assert_eq!(Messenger::from_notification_id("Tencent.Weixin"),Some(Messenger::Wechat));
        for id in ["QQMusic","企业微信助手","FeishuMeeting","DingTalkHelper","unknown"]{assert_eq!(Messenger::from_notification_id(id),None,"未接入的应用不能误认为现有客户端");}
    }
    #[test]
    fn taskbar_clients_do_not_duplicate_system_toasts_or_hide_other_apps(){
        assert_eq!(taskbar_client("微信","unknown"),Some(Messenger::Wechat));
        assert_eq!(taskbar_client("unknown","Tencent.WeChat"),Some(Messenger::Wechat));
        assert_eq!(taskbar_client("企业微信","WeCom"),Some(Messenger::WeCom));
        assert_eq!(taskbar_client("飞书","com.electron.Feishu"),Some(Messenger::Feishu));
        assert_eq!(taskbar_client("微信","com.electron.Feishu"),Some(Messenger::Feishu));
        assert_eq!(taskbar_client("微信","QQ"),None,"可信应用标识优先于显示名称");
        assert!(NotificationFilter::Only(Messenger::Qq).allows("qq"));
        assert!(!NotificationFilter::Only(Messenger::Qq).allows("WeChat"));
        assert!(NotificationFilter::All.allows("DingTalk"));
    }
    #[test]
    fn viewing_a_client_never_acknowledges_another_client_or_unknown_toast_account(){
        let wx=Unread{source:Messenger::Wechat,pid:100,window:11};
        let qq=Unread{source:Messenger::Qq,pid:100,window:11};
        assert!(!wx.viewed_by(Messenger::Qq,100));assert!(!qq.viewed_by(Messenger::Qq,200));
        assert!(qq.viewed_by(Messenger::Qq,100));
        assert!(!Unread::of(Messenger::Qq).viewed_by(Messenger::Qq,100));
        assert!(Unread::of(Messenger::Wechat).viewed_by(Messenger::Wechat,100));
    }
    #[test]
    fn account_history_rejects_foreign_client_samples_and_keeps_notification_namespaces_separate(){
        let target=|source|Target{unread:Unread{source,pid:100,window:11},started:1};
        let mut wx=AccountHistory::new(Messenger::Wechat);let mut qq=AccountHistory::new(Messenger::Qq);
        for at in [10,20]{wx.observe(at,Some(vec![target(Messenger::Wechat)]));qq.observe(at,Some(vec![target(Messenger::Qq)]));}
        assert_eq!(wx.target(1,15,|_|true),Some(target(Messenger::Wechat)));
        assert_eq!(qq.target(1,15,|_|true),Some(target(Messenger::Qq)));
        qq.observe(30,Some(vec![target(Messenger::Wechat)]));
        assert_eq!(qq.target(2,25,|_|true),None);
        assert_eq!(qq.target(1,15,|_|true),None);
        assert_eq!(wx.target(1,15,|_|true),Some(target(Messenger::Wechat)));
    }
    #[test]
    fn single_account_notifications_can_arrive_after_the_next_account_sample(){
        let a=Target{unread:Unread{source:Messenger::Qq,pid:100,window:11},started:1};
        let b=Target{unread:Unread{pid:200,window:22,..a.unread},started:2};
        let mut history=AccountHistory::new(Messenger::Qq);
        for at in [10_000_000,20_000_000,30_000_000]{history.observe(at,Some(vec![a]));}
        assert_eq!(history.target(1,15_000_000,|_|true),Some(a),"连续单账号期间的延迟通知也应能绑定");
        history.observe(40_000_000,Some(vec![a,b]));
        history.observe(50_000_000,Some(vec![b]));history.observe(60_000_000,Some(vec![b]));
        assert_eq!(history.target(2,35_000_000,|_|true),None,"单账号区间重置后，旧通知不能绑定剩余账号");
        assert_eq!(history.target(3,45_000_000,|_|true),None,"从多账号变成单账号的交界期间仍不能猜测");
        assert_eq!(history.target(4,55_000_000,|_|true),Some(b));
    }
    #[test]
    fn single_account_intervals_restart_after_missing_samples_or_long_pauses(){
        let a=Target{unread:Unread{source:Messenger::Qq,pid:100,window:11},started:1};
        for interrupted in [None,Some(Vec::new()),Some(vec![Target{started:2,..a}])]{
            let mut history=AccountHistory::new(Messenger::Qq);
            for at in [10_000_000,20_000_000,30_000_000]{history.observe(at,Some(vec![a]));}
            history.observe(40_000_000,interrupted);
            history.observe(50_000_000,Some(vec![a]));history.observe(60_000_000,Some(vec![a]));
            assert_eq!(history.target(1,35_000_000,|_|true),None,"中断之前的通知不能借用恢复后的单账号区间");
            assert_eq!(history.target(2,55_000_000,|_|true),Some(a));
            history.observe(100_000_000,Some(vec![a]));history.observe(110_000_000,Some(vec![a]));
            assert_eq!(history.target(3,85_000_000,|_|true),None,"长时间暂停期间也不能猜账号");
            assert_eq!(history.target(4,105_000_000,|_|true),Some(a));
        }
    }
    #[test]
    fn errors_use_state_instead_of_translated_error_text(){
        assert_eq!(OpenError::Exited.message(Messenger::Qq),"QQ 账号已退出");
        assert_eq!(OpenError::Exited.message(Messenger::Wechat),"微信 账号已退出");
        assert_ne!(OpenError::Failed("账号已退出".into()),OpenError::Exited);
        assert!(!OpenError::Unavailable.message(Messenger::Qq).contains("已退出"));
    }
}
