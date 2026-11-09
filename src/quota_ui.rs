//! 同心额度环、账号选择及账号设置。
use crate::{app::{App,Cmd},extras::{setting_row,settings_card,settings_control_row},paint::{Card,H},quota::{self,Accent,Account,Config,Provider,Snapshot},theme::{self,W}};
use eframe::egui::{self,Color32};
use std::collections::{HashMap,HashSet};
use chrono::Datelike;

const CLAUDE_LOCAL_NOTICE:&str="由于 Claude 对网络使用环境要求较高，岛仔不会使用登录凭证向 Claude 服务发送任何请求，只读取本机用量记录与缓存，自动显示其中的重置时间；缺少有效记录时，显示“重置时间未知”。";

#[derive(Default)]
pub(crate) struct State {
    cache_path:Option<std::path::PathBuf>,
    pub config:Config,pub rows:HashMap<String,Result<Snapshot,String>>,pub busy:HashSet<String>,
    pub provider:Provider,pub alias:String,pub source:String,pub note:String,
    pub connect_open:bool,pub connect_target:Option<String>,
    pub login:Option<quota::grok::Login>,
    pub workbuddy_connecting:Option<quota::workbuddy::Connect>,
    pub deepseek_connecting:Option<quota::deepseek::Connect>,pub deepseek_key:String,
    pub commandcode_connecting:Option<quota::commandcode::Connect>,pub commandcode_key:String,
    pub stale:HashMap<String,String>,
    pub gemini_connecting:Option<quota::gemini::Connect>,pub gemini_email:String,pub gemini_cookie:String,
}
impl State {
    fn balance_text(&self,id:&str)->Option<String>{let s=self.rows.get(id)?.as_ref().ok()?;s.wallet.as_ref().map(|w|w.primary().compact()).or_else(||s.credits.as_ref().map(|c|c.compact()))}
    fn balance_loading(&self,id:&str)->bool{self.balance_text(id).is_none()&&(self.busy.contains(id)||!self.rows.contains_key(id))}
    pub fn load()->Self {
        if cfg!(test){return Self::default();}
        Self::restore(Config::load(),crate::model::config_dir().join("ai-quota-cache.json"))
    }

    fn restore(config:Config,path:std::path::PathBuf)->Self {
        let mut state=Self{config,cache_path:Some(path.clone()),..Default::default()};
        let cache:serde_json::Value=std::fs::read(path).ok()
            .and_then(|raw|serde_json::from_slice(&raw).ok()).unwrap_or_default();
        for a in &state.config.accounts {
            let Some(snapshot)=a.cached_snapshot(&cache)else{continue;};
            state.rows.insert(a.id.clone(),Ok(snapshot));
            state.stale.insert(a.id.clone(),"上次记录，等待刷新".into());
        }
        state
    }

    fn save_snapshots(&self){
        let Some(path)=&self.cache_path else{return;};
        let mut entries=serde_json::Map::new();
        for a in &self.config.accounts {
            if let Some(Ok(snapshot))=self.rows.get(&a.id) {
                let mut entry=serde_json::json!({"provider":a.provider.name(),"slot":a.slot,"source":a.source,"snapshot":snapshot});
                if a.provider==Provider::Claude{entry["local_identity_checked"]=a.cache_identity_marker();}
                entries.insert(a.id.clone(),entry);
            }
        }
        if let Ok(raw)=serde_json::to_vec(&entries){let _=crate::model::atomic_write(path,&raw);}
    }

    pub fn receive(&mut self,event:quota::Event){
        self.busy.remove(&event.id);
        if !self.config.accounts.iter().any(|a|a.id==event.id && a.enabled){return;}
        if let Err(error)=&event.result{
            if matches!(self.rows.get(&event.id),Some(Ok(_))){
                self.stale.insert(event.id,error.clone());return;
            }
        }
        let success=event.result.is_ok();
        let old_local_record=self.config.accounts.iter().any(|a|a.id==event.id&&a.provider==Provider::Claude)&&event.result.as_ref().is_ok_and(|s|chrono::Utc::now().timestamp()-s.at>1800);
        self.stale.remove(&event.id);
        if old_local_record{self.stale.insert(event.id.clone(),"本地记录较旧，等待 Claude 更新".into());}
        self.rows.insert(event.id,event.result);
        if success{self.save_snapshots();}
    }
}
fn accent_color(accent:Accent,provider:Provider)->Color32{
    if accent==Accent::Brand&&provider==Provider::DeepSeek{return Color32::from_rgb(77,107,254);}
    let accent=if accent==Accent::Brand{match provider{Provider::Codex=>Accent::Violet,Provider::Claude=>Accent::Orange,Provider::Kimi=>Accent::Blue,Provider::GrokBot=>Accent::Green,Provider::Gemini=>Accent::Cyan,Provider::ZCode=>Accent::Blue,Provider::WorkBuddy=>Accent::Green,Provider::DeepSeek=>Accent::Blue,Provider::CommandCode=>Accent::Pink}}else{accent};
    let (r,g,b)=match accent{Accent::Brand|Accent::Violet=>(142,67,255),Accent::Orange=>(255,116,26),Accent::Blue=>(40,126,255),Accent::Cyan=>(0,213,242),Accent::Pink=>(255,43,132),Accent::Gold=>(255,190,0),Accent::Green=>(52,221,145)};
    Color32::from_rgb(r,g,b)
}
fn accent_colors(config:&Config,a:&Account)->[Color32;2]{
    let main=accent_color(config.colors.get(&a.id).copied().unwrap_or_default(),a.provider);
    [main,crate::paint::mix(main,Color32::WHITE,0.16)]
}
fn color_picker(ui:&mut egui::Ui,provider:Provider,value:&mut Accent){
    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(),30.0),egui::Layout::left_to_right(egui::Align::Center),|ui|{
        ui.label(egui::RichText::new("颜色").size(12.0).color(theme::GRAY));
        if ui.selectable_label(*value==Accent::Brand,egui::RichText::new("默认").size(12.0)).on_hover_text("跟随厂商").clicked(){*value=Accent::Brand;}
        for accent in Accent::ALL.into_iter().skip(1){
            let (rect,response)=ui.allocate_exact_size(egui::vec2(22.0,25.0),egui::Sense::click());
            let on=*value==accent;
            if response.on_hover_text(accent.label()).clicked(){*value=accent;}
            if on{ui.painter().circle_stroke(rect.center(),10.0,egui::Stroke::new(1.5,Color32::WHITE));}
            ui.painter().circle_filled(rect.center(),7.0,accent_color(accent,provider));
        }
        ui.label(egui::RichText::new(value.label()).size(12.0).color(theme::GRAY));
    });
}
pub(crate) fn bar_tooltip(response:egui::Response,text:String)->egui::Response{
    let ctx=&response.ctx;let id=response.id.with("bar-tooltip");
    if response.hovered()&&!ctx.input(|i|i.pointer.any_down()){
        let now=ctx.input(|i|i.time);
        let since=ctx.data_mut(|data|*data.get_temp_mut_or_insert_with(id,||now));
        if now-since>=0.5{
            egui::Area::new(id).order(egui::Order::Tooltip).fixed_pos(response.rect.left_bottom()+egui::vec2(0.0,4.0)).interactable(false).show(ctx,|ui|{
                egui::Frame::popup(ui.style()).show(ui,|ui|{ui.label(&text);});
            });
        }else{ctx.request_repaint_after_secs((0.5-(now-since))as f32);}
    }else{ctx.data_mut(|data|data.remove::<f64>(id));}
    response
}
fn logo_texture(ctx:&egui::Context,provider:Provider,pixels:u32)->Option<egui::TextureHandle>{
    let key=egui::Id::new(("quota-logo",provider.name(),pixels));
    if let Some(texture)=ctx.data_mut(|data|data.get_temp::<egui::TextureHandle>(key)){return Some(texture);}
    // WorkBuddy 图标来自官方 WorkBuddyAI 客户端的 workbuddy-app-icon SVG，保留原始配色。
    let bytes:&[u8]=match provider{Provider::ZCode=>include_bytes!("../assets/ai-icons/zcode.svg"),Provider::Codex=>include_bytes!("../assets/ai-icons/codex.svg"),Provider::Claude=>include_bytes!("../assets/ai-icons/claude.svg"),Provider::Kimi=>include_bytes!("../assets/ai-icons/kimi.svg"),Provider::GrokBot=>include_bytes!("../assets/ai-icons/grokbot.svg"),Provider::Gemini=>include_bytes!("../assets/ai-icons/gemini.svg"),Provider::WorkBuddy=>include_bytes!("../assets/ai-icons/workbuddy.svg"),Provider::DeepSeek=>include_bytes!("../assets/ai-icons/deepseek.svg"),Provider::CommandCode=>include_bytes!("../assets/ai-icons/commandcode.svg")};
    let mut image=egui_extras::image::load_svg_bytes_with_size(bytes,Some(egui::load::SizeHint::Size(pixels,pixels))).ok()?;
    // 白色标识使用矢量覆盖率，避免透明边缘再次变暗。
    for pixel in &mut image.pixels{if matches!(provider,Provider::WorkBuddy|Provider::ZCode){continue;}*pixel=if provider==Provider::DeepSeek{Color32::from_rgba_unmultiplied(77,107,254,pixel.a())}else{Color32::from_white_alpha(pixel.a())};}
    let texture=ctx.load_texture(format!("quota-logo-{}-{pixels}",provider.name()),image,egui::TextureOptions::LINEAR);
    if !provider.balance()&&provider!=Provider::ZCode{crate::skin_paint::mark_monochrome(texture.id());}else{crate::skin_paint::mark_colored(texture.id());}
    ctx.data_mut(|data|data.insert_temp(key,texture.clone()));Some(texture)
}
fn logo(c:&Card,provider:Provider,x:f32,y:f32,size:f32){
    let ctx=c.ui.ctx();let ppp=ctx.pixels_per_point();let center=c.pos(x,y);
    let pixels=(size*c.s*ppp).round().max(1.0) as u32;let half=pixels as f32/2.0;
    let min=egui::pos2((center.x*ppp-half).round()/ppp,(center.y*ppp-half).round()/ppp);
    let rect=egui::Rect::from_min_size(min,egui::Vec2::splat(pixels as f32/ppp));
    if let Some(texture)=logo_texture(ctx,provider,pixels){c.painter.image(texture.id(),rect,egui::Rect::from_min_max(egui::pos2(0.0,0.0),egui::pos2(1.0,1.0)),Color32::WHITE);}
}
fn deepseek_badge_svg(skin:crate::model::Skin)->String{
    use crate::model::Skin;
    let (fill,rim)=match skin{Skin::Classic=>("#e9edff","#cfd7ff"),Skin::Cream=>("#e9edff","#dce2fc"),Skin::GlassLight=>("#f5f7ff","#dfe5fa"),Skin::GlassDark|Skin::Fluid=>("#dce4fa","#b8c9ec")};
    let whale=include_str!("../assets/ai-icons/deepseek.svg").replace("currentColor","#4d6bfe").replace("width=\"1em\"","x=\"12.5\" width=\"75\"").replace("height=\"1em\"","y=\"12.5\" height=\"75\"");
    format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\"><circle cx=\"50\" cy=\"50\" r=\"48\" fill=\"{fill}\" stroke=\"{rim}\" stroke-width=\"3\"/>{whale}</svg>")
}
fn deepseek_badge(c:&Card,skin:crate::model::Skin,x:f32,y:f32,size:f32){
    let ctx=c.ui.ctx();let ppp=ctx.pixels_per_point();let pixels=(size*c.s*ppp).round().max(1.0) as u32;
    let key=egui::Id::new(("deepseek-badge",skin.key(),pixels));
    let texture=ctx.data_mut(|data|data.get_temp::<egui::TextureHandle>(key)).or_else(||{
        let image=egui_extras::image::load_svg_bytes_with_size(deepseek_badge_svg(skin).as_bytes(),Some(egui::load::SizeHint::Size(pixels,pixels))).ok()?;
        let texture=ctx.load_texture(format!("deepseek-badge-{}-{pixels}",skin.key()),image,egui::TextureOptions::LINEAR);
        crate::skin_paint::mark_colored(texture.id());
        ctx.data_mut(|data|data.insert_temp(key,texture.clone()));Some(texture)
    });
    if let Some(texture)=texture{
        let center=c.pos(x,y);let half=pixels as f32/2.0;let min=egui::pos2((center.x*ppp-half).round()/ppp,(center.y*ppp-half).round()/ppp);
        c.painter.image(texture.id(),egui::Rect::from_min_size(min,egui::Vec2::splat(pixels as f32/ppp)),egui::Rect::from_min_max(egui::Pos2::ZERO,egui::pos2(1.0,1.0)),Color32::WHITE);
    }
}
fn ring(c:&Card,a:&Account,snapshot:Option<&Snapshot>,x:f32,y:f32,size:f32,colors:[Color32;2]){
    let windows=snapshot.map(Snapshot::rings).unwrap_or_default();let dual=windows.len()>1;
    let stroke=if dual{if size<35.0{2.2}else{6.5}}else{(size*0.102).clamp(3.0,6.5)};
    let radius=size/2.0-stroke/2.0-1.0;
    let gap=if size<35.0{1.5}else{3.0};
    let start=-std::f32::consts::FRAC_PI_2;
    for i in 0..if dual{2}else{1}{
        let r=radius-i as f32*(stroke+gap);
        if let Some(w)=windows.get(i){
            let col=colors[i];let dark=crate::paint::mix(col,Color32::BLACK,0.8);
            if w.remaining>=100.0{c.ring(x,y,r,stroke,col);}else if w.remaining<=0.0{c.ring(x,y,r,stroke,dark);}else{
                let end=start+std::f32::consts::TAU*w.remaining/100.0;
                c.arc(x,y,r,end,start+std::f32::consts::TAU,stroke,dark);
                c.arc(x,y,r,start,end,stroke,col);
                for angle in [start,end]{c.circle(x+r*angle.cos(),y+r*angle.sin(),stroke/2.0,col);}
            }
        }
    }
    if a.provider==Provider::ZCode && windows.is_empty(){logo(c,a.provider,x,y,if size<35.0{size-3.0}else{size*0.72});}
    else if size<35.0 {logo(c,a.provider,x,y,if dual{10.0}else{12.0});}else if dual {logo(c,a.provider,x,y,24.0);}else if let Some(w)=windows.first(){
        let value=format!("{:.0}",w.remaining);let large=if value.len()>2{22.0}else{28.0};
        let number_width=c.text_w(&value,large,W::Bold);let width=number_width+c.text_w("%",11.0,W::Semibold)+2.0;
        let left=x-width/2.0;c.centered_text(left,y,&value,large,W::Bold,theme::WHITE,H::Left);
        c.centered_text(left+number_width+2.0,y+3.0,"%",11.0,W::Semibold,theme::GRAY,H::Left);
    }else{c.centered_text(x,y,"—",24.0,W::Semibold,theme::DIM,H::Center);}
}
fn reset_text(at:Option<i64>,now:&chrono::DateTime<chrono::Local>)->String {
    let Some(at)=at.and_then(|at|chrono::DateTime::from_timestamp(at,0)) else{return "重置时间未知".into();};
    let at=at.with_timezone(&chrono::Local);
    if at.timestamp()<=now.timestamp(){return "等待刷新".into();}
    let format=if at.date_naive()==now.date_naive(){"今天 %H:%M 重置"}else if at.year()==now.year(){"%-m月%-d日 %H:%M 重置"}else{"%Y/%-m/%-d %H:%M 重置"};
    at.format(format).to_string()
}
fn claude_disclaimer(ui:&mut egui::Ui){
    ui.separator();
    ui.label(egui::RichText::new("免责声明").size(12.0).strong());
    ui.add(egui::Label::new(egui::RichText::new("岛仔为非官方辅助工具，数据仅供参考，以 Claude 官方显示为准；本地读取不构成账号安全保证。").size(12.0).color(crate::settings_theme::color(ui.ctx(),Color32::from_gray(180),Color32::from_gray(75)))).wrap());
}
impl App {
    pub(crate) fn quota_tick(&mut self){
        if let Some(result)=self.quota.workbuddy_connecting.as_ref().and_then(|c|c.poll()){
            self.quota.workbuddy_connecting=None;
            match result{Ok(connected)=>{
                let id=self.quota.config.connect_workbuddy(connected.slot,connected.label);self.quota.stale.remove(&id);self.quota.rows.insert(id,Ok(connected.snapshot));self.quota.save_snapshots();self.quota.connect_target=None;self.quota.config.save();self.quota.note="WorkBuddy 已连接".into();self.pending.push(Cmd::QuotaConnected);
            },Err(error)=>self.quota.note=error}
        }

        if let Some(result)=self.quota.deepseek_connecting.as_ref().and_then(|connect|connect.poll()){
            self.quota.deepseek_connecting=None;
            match result{Ok(connected)=>{let old=self.quota.connect_target.as_ref().and_then(|id|self.quota.config.accounts.iter().find(|a|&a.id==id)).cloned();
                let id=if let Some(target)=self.quota.connect_target.as_deref(){match self.quota.config.reconnect_deepseek(target,connected.slot.clone(),connected.label){Ok(id)=>id,Err(error)=>{self.quota.note=error;return;}}}else{self.quota.config.connect_deepseek(connected.slot.clone(),connected.label)};
                if let Some(old)=old.filter(|a|a.slot.as_deref()!=Some(&connected.slot)){quota::deepseek::forget(&old);}
                self.quota.connect_target=None;self.quota.stale.remove(&id);self.quota.rows.insert(id,Ok(connected.snapshot));self.quota.save_snapshots();self.quota.config.save();self.quota.note="DeepSeek 已连接".into();self.pending.push(Cmd::QuotaConnected);},Err(error)=>self.quota.note=error}
        }
        if let Some(result)=self.quota.commandcode_connecting.as_ref().and_then(|connect|connect.poll()){
            self.quota.commandcode_connecting=None;
            match result{Ok(connected)=>{let old=self.quota.connect_target.as_ref().and_then(|id|self.quota.config.accounts.iter().find(|a|&a.id==id)).cloned();
                let id=if let Some(target)=self.quota.connect_target.as_deref(){match self.quota.config.reconnect_commandcode(target,connected.slot.clone(),connected.label){Ok(id)=>id,Err(error)=>{self.quota.note=error;return;}}}else{self.quota.config.connect_commandcode(connected.slot.clone(),connected.label)};
                if let Some(old)=old.filter(|a|a.slot.as_deref()!=Some(connected.slot.as_str())){quota::commandcode::forget(&old);}
                self.quota.connect_target=None;self.quota.stale.remove(&id);self.quota.rows.insert(id,Ok(connected.snapshot));self.quota.save_snapshots();self.quota.config.save();self.quota.note="Command Code 已连接".into();self.pending.push(Cmd::QuotaConnected);},Err(error)=>self.quota.note=error}
        }
        let result=self.quota.gemini_connecting.as_ref().and_then(|connect|connect.poll());
        if let Some(result)=result{
            self.quota.gemini_connecting=None;
            match result{Ok(connected)=>{let id=self.quota.config.connect_gemini(connected.slot,connected.label);self.quota.rows.insert(id,Ok(connected.snapshot));self.quota.save_snapshots();self.quota.config.save();self.quota.gemini_cookie.clear();self.quota.note="Gemini 已连接".into();self.pending.push(Cmd::QuotaConnected);},Err(error)=>self.quota.note=error}
        }
        let result=self.quota.login.as_ref().and_then(|login|login.poll());
        if let Some(result)=result{
            self.quota.login=None;
            match result{Ok(connected)=>{self.quota.config.connect_grok(connected.slot,connected.label);self.quota.config.save();self.quota.note="Grok Bot 已连接".into();self.pending.push(Cmd::QuotaConnected);},Err(error)=>self.quota.note=error}
        }
    }
    fn quota_login(&mut self,alias:String){
        if self.quota.login.is_some(){return;}
        match quota::grok::Login::start(alias){Ok((login,url))=>{self.quota.login=Some(login);self.quota.note="请在浏览器中完成登录，账号会自动添加".into();self.pending.push(Cmd::OpenUrl(url));},Err(error)=>self.quota.note=error}
    }
    pub fn quota_config(&self)->&Config{&self.quota.config}
    pub fn quota_initialize(&mut self){if !crate::model::config_dir().join("ai-quota.json").exists(){self.quota.config.discover();self.quota.config.save();}}
    pub fn quota_pending(&mut self,accounts:&[Account]){self.quota.busy.extend(accounts.iter().map(|a|a.id.clone()));}
    pub fn quota_event(&mut self,event:quota::Event){self.quota.receive(event);}
    pub fn quota_set_source(&mut self,path:std::path::PathBuf){self.quota.source=path.to_string_lossy().into_owned();}
    pub(crate) fn quota_bar_indices(&self)->Vec<usize>{
        if !self.quota.config.enabled || !self.quota.config.compact || !self.settings.visible_pages[6] || self.settings.minimal || self.t<self.notification_bell_until{return vec![];}
        self.quota.config.pinned_indices()
    }
    pub(crate) fn quota_balance_layout(&self,ctx:&egui::Context)->Vec<(usize,f32)>{
        self.quota_bar_indices().iter().enumerate().filter_map(|(position,index)|{
            let account=&self.quota.config.accounts[*index];if !account.provider.balance(){return None;}
            let extra=if let Some(text)=self.quota.balance_text(&account.id){ctx.fonts(|fonts|fonts.layout_no_wrap(text,theme::font(theme::size::PILL_CLOCK,W::Bold),theme::WHITE).size().x).ceil()+6.0}else if self.quota.balance_loading(&account.id){22.0}else{0.0};Some((position,extra))
        }).collect()
    }
    pub(crate) fn draw_quota_bar(&mut self,c:&Card,cx:f32,layout:&crate::bar_layout::Layout){
        let indices=self.quota_bar_indices();let mut chosen=None;
        for (position,i)in indices.iter().enumerate(){
            let alpha=layout.pet_clearance(crate::bar_layout::quota_slot(position));
            let faded=c.faded(alpha);let faded=if alpha<0.8{faded.ghost()}else{faded};let c=&faded;
            let a=&self.quota.config.accounts[*i];let x=cx+layout.x[crate::bar_layout::quota_slot(position)].unwrap();
            let snapshot=self.quota.rows.get(&a.id).and_then(|r|r.as_ref().ok());
            if a.provider.balance(){
                let wide=true;let extra=if wide{layout.widths[crate::bar_layout::quota_slot(position)]-27.0}else{0.0};let icon_x=x-extra/2.0;
                if a.provider==Provider::WorkBuddy{logo(c,a.provider,icon_x,15.0,22.0);}else{deepseek_badge(c,self.settings.skin,icon_x,15.0,24.0);}
                let loading=self.quota.balance_loading(&a.id);let stale=self.quota.stale.contains_key(&a.id);
                if stale||(!loading&&snapshot.is_none()){
                    c.circle(icon_x+9.0,24.0,3.8,Color32::BLACK);
                    c.circle(icon_x+9.0,24.0,2.5,Color32::from_rgb(240,165,84));
                }
                if wide{
                    if let Some(text)=self.quota.balance_text(&a.id){
                        let wallet=snapshot.and_then(|s|s.wallet.as_ref());
                        let color=if stale{theme::GRAY}else if wallet.is_some_and(|w|!w.available||w.primary().total<=0.0){Color32::from_rgb(255,166,77)}else{theme::WHITE};
                        c.text(icon_x+18.0,21.0,&text,theme::size::PILL_CLOCK,W::Bold,color,H::Left);
                    }else if loading{
                        let active=(self.t*3.0) as usize%3;
                        for dot in 0..3{c.circle(icon_x+20.0+dot as f32*5.0,15.0,1.65,theme::GRAY.gamma_multiply(if dot==active{1.0}else{0.35}));}
                        c.ui.ctx().request_repaint_after(std::time::Duration::from_millis(160));
                    }
                }
                if c.hit(x-(27.0+extra)/2.0,1.0,27.0+extra,28.0,("quota-pin",&a.id)).clicked(){chosen=Some(a.id.clone());}continue;
            }
            ring(c,a,snapshot,x,15.0,27.0,accent_colors(&self.quota.config,a));
            let mut same=self.quota.config.accounts.iter().filter(|other|other.provider==a.provider && other.enabled);
            if same.clone().count()>1{let n=same.position(|other|other.id==a.id).unwrap_or(0)+1;c.circle(x+10.0,24.0,4.5,Color32::from_gray(225));c.text(x+10.0,26.8,&n.to_string(),6.5,W::Bold,Color32::BLACK,H::Center);}
            let summary=snapshot.map(|s|s.rings().iter().map(|w|format!("{}剩余 {:.0}%",w.label,w.remaining)).collect::<Vec<_>>().join(" · ")).unwrap_or_else(||if self.quota.busy.contains(&a.id){"正在更新".into()}else{"额度暂不可用".into()});
            if bar_tooltip(c.hit(x-14.0,1.0,28.0,28.0,("quota-pin",&a.id)),format!("{} · {}\n{}",a.provider.name(),a.alias,summary)).clicked(){chosen=Some(a.id.clone());}
        }
        if let Some(id)=chosen{self.quota.config.selected=id;self.quota.config.save();self.show_page(6);}
    }
    pub(crate) fn page_quota(&mut self,c:&Card){
        if c.hit(303.0,8.0,32.0,21.0,"quota-refresh").on_hover_text("刷新额度").clicked(){self.pending.push(Cmd::QuotaRefresh);}
        c.text(319.0,23.0,if self.quota.busy.is_empty(){"刷新"}else{"更新中"},10.5,W::Regular,theme::GRAY,H::Center);
        let Some(selected)=self.quota.config.selected_index() else{
            c.text(43.0,61.0,"还没有添加 AI 账号",13.0,W::Bold,theme::WHITE,H::Left);
            c.text(43.0,80.0,"连接 AI 账号，查看剩余额度",10.5,W::Regular,theme::GRAY,H::Left);
            let(r,b)=crate::paint::capsule(c,43.0,92.0,94.0,24.0,"add-quota-account",theme::BTN,theme::BTN_HOVER);b.text(90.0,108.0,"添加账号",10.0,W::Semibold,theme::WHITE,H::Center);if r.clicked(){self.settings_tab=11;self.open_settings();}return;
        };
        let indices=self.quota.config.ordered_indices(false);
        let pos=indices.iter().position(|i|*i==selected).unwrap_or(0);let start=pos/3*3;let mut picked=None;
        if indices.len()>3{for (x,offset,label)in [(42.0,-1isize,"‹"),(357.0,1,"›")]{if c.hit(x-7.0,30.0,18.0,19.0,("quota-account-next",offset)).clicked(){picked=Some(indices[(pos as isize+offset).rem_euclid(indices.len() as isize) as usize]);}c.text(x+2.0,44.0,label,13.0,W::Regular,theme::GRAY,H::Center);}}
        for (column,index)in indices.iter().skip(start).take(3).enumerate(){let a=&self.quota.config.accounts[*index];let x=59.0+column as f32*99.0;let on=*index==selected;let colors=accent_colors(&self.quota.config,a);let color=colors[0];let(r,b)=crate::paint::capsule(c,x,30.0,94.0,21.0,&format!("quota-account-{}",a.id),if on{Color32::from_rgb(color.r()/5,color.g()/5,color.b()/5)}else{Color32::BLACK},theme::BTN_HOVER);let text=b.fit(&a.alias,10.5,W::Regular,84.0);b.text(x+47.0,45.0,&text,10.5,W::Regular,if on{colors[1]}else{theme::GRAY},H::Center);if r.on_hover_text(format!("{} · {}",a.provider.name(),a.alias)).clicked(){picked=Some(*index);}}
        if let Some(i)=picked{self.quota.config.selected=self.quota.config.accounts[i].id.clone();self.quota.config.save();}
        let a=&self.quota.config.accounts[selected];let result=self.quota.rows.get(&a.id);let snapshot=result.and_then(|r|r.as_ref().ok());
        let colors=accent_colors(&self.quota.config,a);
        if let Some(s)=snapshot.filter(|s|s.at>0){
            if let Some(at)=chrono::DateTime::from_timestamp(s.at,0){
                let at=at.with_timezone(&chrono::Local);let time=at.format(if at.date_naive()==chrono::Local::now().date_naive(){"%H:%M"}else{"%m/%d %H:%M"}).to_string();
                let busy=self.quota.busy.contains(&a.id);let stale=self.quota.stale.get(&a.id);
                let status=if busy{format!("{} · {time}",if a.provider==Provider::Claude{"读取中…"}else{"正在更新…"})}
                    else if stale.is_some(){format!("上次记录 · {time}")}
                    else if a.provider==Provider::Claude{format!("本地记录 · {time}")}
                    else if let Some(wallet)=&s.wallet{let b=wallet.primary();if wallet.balances.len()>1{let other=wallet.balances.iter().find(|other|other.currency!=b.currency).unwrap();format!("另有 {} {} · {time}",other.currency,other.full(other.total))}else{format!("{} · {time} 更新",b.currency)}}
                    else{format!("{time} 更新")};
                let x=if a.provider.balance(){125.0}else{157.0};let width=357.0-x;let y=154.0;
                c.centered_text(x,y,&c.fit(&status,10.0,W::Regular,width),10.0,W::Regular,if stale.is_some()&&!busy{Color32::from_rgb(255,166,77)}else{theme::DIM},H::Left);
                c.hit(x,y-6.0,width,8.0,("quota-update-time",&a.id)).on_hover_text(if let Some(reason)=stale{format!("{status}\n{reason}")}else{status});
            }
        }
        if a.provider==Provider::WorkBuddy{
            logo(c,a.provider,85.0,105.0,44.0);
            if let Some(credits)=snapshot.and_then(|s|s.credits.as_ref()){
                c.text(125.0,81.0,"可用积分",10.5,W::Regular,theme::GRAY,H::Left);
                c.text(125.0,107.0,&credits.full(),23.0,W::Semibold,theme::WHITE,H::Left);
                let detail=if let Some(reason)=self.quota.stale.get(&a.id){reason.clone()}else{credits.packages.iter().map(|(label,n)|format!("{label} {n}")).collect::<Vec<_>>().join(" · ")};c.text(125.0,131.0,&c.fit(&detail,10.0,W::Regular,225.0),10.0,W::Regular,theme::GRAY,H::Left);
            }else{let text=result.and_then(|r|r.as_ref().err()).map(String::as_str).unwrap_or("正在查询积分…");for(i,line)in c.wrap(text,11.0,W::Regular,226.0,3).iter().enumerate(){c.text(125.0,92.0+i as f32*17.0,line,11.0,W::Regular,theme::GRAY,H::Left);}}
            return;
        }
        if a.provider==Provider::DeepSeek{
            deepseek_badge(c,self.settings.skin,85.0,105.0,50.0);
            if let Some(s)=snapshot{
                if let Some(wallet)=&s.wallet{
                    let b=wallet.primary();
                    c.text(125.0,81.0,if wallet.available{"可用余额"}else{"余额不可用"},10.5,W::Regular,theme::GRAY,H::Left);
                    let amount=b.full(b.total);let size=(23.0*145.0/c.text_w(&amount,23.0,W::Semibold).max(145.0)).min(23.0);
                    c.text(125.0,109.0,&amount,size,W::Semibold,if !wallet.available{Color32::from_rgb(255,166,77)}else{theme::WHITE},H::Left);
                    for (y,label,value) in [(90.0,"充值",b.topped_up),(118.0,"赠送",b.granted)]{
                        c.text(281.0,y,label,10.0,W::Regular,theme::GRAY,H::Left);
                        let text=b.full(value);let size=10.0*54.0/c.text_w(&text,10.0,W::Semibold).max(54.0);c.text(357.0,y,&text,size,W::Semibold,theme::WHITE,H::Right);
                    }
                }
            }else{let text=if self.quota.busy.contains(&a.id){"正在更新余额…"}else{result.and_then(|r|r.as_ref().err()).map(String::as_str).unwrap_or("等待更新余额")};for (i,line) in c.wrap(text,11.0,W::Regular,226.0,3).iter().enumerate(){c.text(125.0,92.0+i as f32*17.0,line,11.0,W::Regular,theme::GRAY,H::Left);}}
            return;
        }
        ring(c,a,snapshot,103.0,105.0,70.0,colors);
        let raw_plan=snapshot.map(|s|s.plan.as_str()).unwrap_or_default();let plan=match raw_plan{"pro"|"prolite"|"pro_lite"=>"Pro","plus"=>"Plus","free"=>"Free","team"|"business"=>"Business","enterprise"=>"Enterprise",other=>other};let label=if a.provider==Provider::Claude{"Claude · 本地记录".into()}else if plan.is_empty(){a.provider.name().to_owned()}else{format!("{} · {}",a.provider.name(),plan)};
        c.text(157.0,64.0,&c.fit(&label,12.0,W::Semibold,145.0),12.0,W::Semibold,theme::WHITE,H::Left);
        if let Some(s)=snapshot{
            let windows=s.rings();let now=chrono::Local::now();
            if a.provider==Provider::CommandCode&&s.windows.len()>2{
                c.text(355.0,64.0,"剩余",10.5,W::Regular,theme::GRAY,H::Right);
                for (i,w)in s.windows.iter().take(3).enumerate(){
                    let y=78.0+i as f32*24.0;
                    c.circle(159.0,y-4.0,2.5,colors[i%2]);
                    c.text(168.0,y,&w.label,11.0,W::Semibold,theme::WHITE,H::Left);
                    let reset=reset_text(w.reset,&now);
                    c.text(168.0,y+14.0,&c.fit(&reset,10.0,W::Regular,175.0),10.0,W::Regular,theme::GRAY,H::Left);
                    c.centered_text(355.0,y+2.0,&format!("{:.0}%",w.remaining),17.0,W::Semibold,theme::WHITE,H::Right);
                }
            }else if windows.is_empty() && a.provider==Provider::ZCode{
                c.text(157.0,104.0,"官网未分配体验额度",11.0,W::Regular,theme::GRAY,H::Left);
            }else if windows.len()==1{
                let w=windows[0];c.text(157.0,103.0,&format!("{}剩余",w.label),12.0,W::Semibold,theme::WHITE,H::Left);
                c.text(157.0,124.0,&reset_text(w.reset,&now),11.0,W::Regular,theme::GRAY,H::Left);
            }else{
                c.text(355.0,64.0,"剩余",10.5,W::Regular,theme::GRAY,H::Right);
                for (i,w)in windows.iter().enumerate(){
                    let y=86.0+i as f32*35.0;c.circle(159.0,y-4.0,2.5,colors[i]);
                    c.text(168.0,y,&w.label,11.5,W::Semibold,theme::WHITE,H::Left);
                    let reset=reset_text(w.reset,&now);
                    c.text(168.0,y+17.0,&c.fit(&reset,11.0,W::Regular,185.0),11.0,W::Regular,theme::GRAY,H::Left);
                    c.centered_text(355.0,y+4.0,&format!("{:.0}%",w.remaining),22.0,W::Semibold,theme::WHITE,H::Right);
                }
            }
        }else{let message=if self.quota.busy.contains(&a.id){"正在更新额度…"}else{result.and_then(|r|r.as_ref().err()).map(String::as_str).unwrap_or("等待更新额度")};for(i,line)in c.wrap(message,11.0,W::Regular,197.0,2).iter().enumerate(){c.text(157.0,103.0+i as f32*17.0,line,11.0,W::Regular,theme::GRAY,H::Left);}}
    }
    fn quota_open_connection(&mut self,id:Option<String>){
        if self.quota.workbuddy_connecting.is_some()||self.quota.deepseek_connecting.is_some()||self.quota.commandcode_connecting.is_some()||self.quota.gemini_connecting.is_some()||self.quota.login.is_some(){self.quota.connect_open=true;return;}
        self.quota.note.clear();self.quota.deepseek_key.clear();self.quota.commandcode_key.clear();self.quota.gemini_cookie.clear();self.quota.source.clear();self.quota.alias.clear();
        if let Some(a)=id.as_ref().and_then(|id|self.quota.config.accounts.iter().find(|a|&a.id==id)){
            self.quota.provider=a.provider;self.quota.alias=a.alias.clone();self.quota.source=a.source.to_string_lossy().into();
        }
        self.quota.connect_target=id;self.quota.connect_open=true;
    }
    fn quota_remove_account(&mut self,id:&str){
        if let Some(a)=self.quota.config.accounts.iter().find(|a|a.id==id){match a.provider{Provider::GrokBot=>quota::grok::forget(a),Provider::Gemini=>quota::gemini::forget(a),Provider::DeepSeek=>quota::deepseek::forget(a),Provider::WorkBuddy=>quota::workbuddy::forget(a),Provider::CommandCode=>quota::commandcode::forget(a),_=>{}}}
        self.quota.config.accounts.retain(|a|a.id!=id);self.quota.config.colors.remove(id);self.quota.rows.remove(id);self.quota.stale.remove(id);self.quota.busy.remove(id);self.quota.save_snapshots();
        if self.quota.config.selected==id{self.quota.config.selected=self.quota.config.accounts.first().map(|a|a.id.clone()).unwrap_or_default();}
    }
    pub(crate) fn quota_settings(&mut self,ui:&mut egui::Ui){
        let before=self.quota.config.clone();
        settings_card(ui,|ui|{
            setting_row(ui,"开启 AI 额度","每 3 分钟自动更新",&mut self.quota.config.enabled);
            ui.separator();
            setting_row(ui,"收起时显示常驻账号","WorkBuddy 积分在前，DeepSeek 余额在最后",&mut self.quota.config.compact);
        });
        ui.add_space(10.0);
        ui.horizontal_wrapped(|ui|{
            if ui.button("＋ 添加账号").clicked(){self.quota_open_connection(None);}
            if ui.button("识别本机账号").clicked(){self.quota.note=self.quota.config.discover().message();}
            if ui.add_enabled(self.quota.busy.is_empty(),egui::Button::new("刷新额度")).clicked(){self.pending.push(Cmd::QuotaRefresh);}
        });ui.add_space(8.0);
        let accounts=self.quota.config.accounts.clone();let mut remove=None;let mut connect=None;let mut reorder=None;
        for mut a in accounts{
            let fill=crate::settings_theme::color(ui.ctx(),Color32::from_rgb(35,35,38),Color32::WHITE);
            let row=egui::Frame::new().fill(fill).corner_radius(10).inner_margin(egui::Margin::symmetric(16,12)).show(ui,|ui|{
                ui.set_width((ui.available_width()-1.0).max(0.0));ui.spacing_mut().item_spacing.y=6.0;
                ui.push_id(a.id.clone(),|ui|{
                    ui.horizontal(|ui|{
                        ui.add(egui::Label::new("::").sense(egui::Sense::drag())).on_hover_text("拖动调整账号顺序").dnd_set_drag_payload(a.id.clone());
                        let (rect,_)=ui.allocate_exact_size(egui::vec2(26.0,28.0),egui::Sense::hover());let c=Card::new(ui,rect.min,ui.clip_rect());
                        if a.provider==Provider::DeepSeek{deepseek_badge(&c,if ui.visuals().dark_mode{crate::model::Skin::Classic}else{crate::model::Skin::GlassLight},13.0,14.0,26.0);}else if a.provider==Provider::ZCode{logo(&c,a.provider,13.0,14.0,26.0);}else{c.circle(13.0,14.0,13.0,crate::paint::mix(accent_colors(&self.quota.config,&a)[0],Color32::BLACK,0.75));logo(&c,a.provider,13.0,14.0,20.0);}
                        ui.label(egui::RichText::new(a.provider.name()).strong());
                        // 从右往左排：按钮先靠右放好，名称输入框占用剩余宽度，按钮再宽也不会顶出卡片。
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{
                            if ui.add_enabled(self.quota.workbuddy_connecting.is_none()&&self.quota.deepseek_connecting.is_none()&&self.quota.gemini_connecting.is_none()&&self.quota.login.is_none(),egui::Button::new("移除")).clicked(){remove=Some(a.id.clone());}
                            if ui.selectable_label(self.quota.config.selected==a.id,"当前").clicked(){self.quota.config.selected=a.id.clone();}
                            ui.add_sized([ui.available_width().max(55.0),26.0],egui::TextEdit::singleline(&mut a.alias).hint_text("账号名称")).on_hover_text("直接修改账号名称");
                        });
                    });
                    let result=self.quota.rows.get(&a.id);let snapshot=result.and_then(|r|r.as_ref().ok());let error=self.quota.stale.get(&a.id).or_else(||result.and_then(|r|r.as_ref().err()));
                    if let Some(s)=snapshot{
                        if let Some(credits)=&s.credits{ui.horizontal_wrapped(|ui|{ui.label(egui::RichText::new("可用积分").size(11.0).color(ui.visuals().weak_text_color()));ui.label(egui::RichText::new(credits.full()).size(20.0).strong());ui.label(egui::RichText::new(credits.packages.iter().map(|(label,n)|format!("{label} {n}")).collect::<Vec<_>>().join(" · ")).size(11.0).color(ui.visuals().weak_text_color()));});}else if let Some(wallet)=&s.wallet{
                            ui.horizontal_wrapped(|ui|{
                                ui.label(egui::RichText::new(if wallet.available{"可用余额"}else{"余额不可用"}).size(11.0).color(ui.visuals().weak_text_color()));
                                ui.label(egui::RichText::new(wallet.primary().full(wallet.primary().total)).size(20.0).strong());
                                ui.label(egui::RichText::new(format!("充值 {} · 赠送 {}",wallet.primary().full(wallet.primary().topped_up),wallet.primary().full(wallet.primary().granted))).size(11.0).color(ui.visuals().weak_text_color()));
                            });
                        }else if a.provider==Provider::ZCode && s.windows.is_empty(){
                            ui.label("已连接 · 暂无体验额度");
                        }else{
                            let windows=s.rings();let count=windows.len().max(1);let width=(ui.available_width()-12.0*(count-1) as f32)/count as f32;
                            ui.horizontal_top(|ui|{ui.spacing_mut().item_spacing.x=12.0;for window in windows{ui.allocate_ui_with_layout(egui::vec2(width,30.0),egui::Layout::top_down(egui::Align::Min),|ui|{
                                ui.horizontal(|ui|{ui.label(egui::RichText::new(format!("{}  剩余 {:.0}%",window.label,window.remaining)).size(11.0));});
                                ui.add(egui::ProgressBar::new(window.remaining/100.0).desired_width(width).desired_height(3.0).fill(accent_colors(&self.quota.config,&a)[0]));
                            });}});
                        }
                    }
                    if !a.enabled{ui.label(egui::RichText::new("已暂停监控").size(11.0).color(ui.visuals().weak_text_color()));}
                    else if let Some(error)=error{ui.add(egui::Label::new(egui::RichText::new(error).size(11.0).color(crate::settings_theme::color(ui.ctx(),Color32::from_rgb(231,175,108),Color32::from_rgb(151,94,28)))).wrap());}
                    else if snapshot.is_none(){ui.label(egui::RichText::new(if self.quota.busy.contains(&a.id){"正在查询额度…"}else{"等待更新"}).size(11.0).color(ui.visuals().weak_text_color()));}
                    ui.horizontal_wrapped(|ui|{
                        ui.checkbox(&mut a.enabled,"监控");ui.checkbox(&mut a.pinned,"常驻岛上");
                        if ui.add_enabled(!self.quota.busy.contains(&a.id),egui::Button::new(if error.is_some(){"重试"}else{"刷新"})).clicked(){self.pending.push(Cmd::QuotaRefresh);}
                        if ui.button("重新连接").clicked(){connect=Some(a.id.clone());}
                        if let Some(s)=snapshot{let time=chrono::DateTime::from_timestamp(s.at,0).map(|at|at.with_timezone(&chrono::Local).format(if a.provider==Provider::Claude{"%m-%d %H:%M 本地记录"}else{"%H:%M 更新"}).to_string()).unwrap_or_default();ui.label(egui::RichText::new(time).size(10.0).color(ui.visuals().weak_text_color()));}
                    });
                    if a.provider.balance(){ui.label(egui::RichText::new(if a.provider==Provider::DeepSeek{"品牌蓝 · 圆底随皮肤"}else{"官方图标"}).size(11.0).color(ui.visuals().weak_text_color()));}else{
                        let mut color=self.quota.config.colors.get(&a.id).copied().unwrap_or_default();color_picker(ui,a.provider,&mut color);
                        if color==Accent::Brand{self.quota.config.colors.remove(&a.id);}else{self.quota.config.colors.insert(a.id.clone(),color);}
                    }
                    if a.provider==Provider::Claude{
                        ui.separator();ui.label(egui::RichText::new("本地读取说明").size(12.0).strong());
                        ui.add(egui::Label::new(egui::RichText::new(CLAUDE_LOCAL_NOTICE).size(12.0).color(crate::settings_theme::color(ui.ctx(),Color32::from_gray(180),Color32::from_gray(75)))).wrap());
                        claude_disclaimer(ui);
                    }
                });
            });
            if row.response.dnd_hover_payload::<String>().is_some(){ui.painter().rect_stroke(row.response.rect,10,egui::Stroke::new(2.0,ui.visuals().selection.bg_fill),egui::StrokeKind::Inside);}
            if let Some(from)=row.response.dnd_release_payload::<String>(){reorder=Some(((*from).clone(),a.id.clone()));}
            if let Some(account)=self.quota.config.accounts.iter_mut().find(|account|account.id==a.id){*account=a;}
            ui.add_space(8.0);
        }
        if let Some((from,to))=reorder{self.quota.config.move_account(&from,&to);}
        if let Some(id)=remove{self.quota_remove_account(&id);}
        if let Some(id)=connect{self.quota_open_connection(Some(id));}
        if !self.quota.connect_open&&!self.quota.note.is_empty(){ui.add_space(8.0);ui.label(egui::RichText::new(&self.quota.note).size(12.0).color(ui.visuals().weak_text_color()));}
        self.quota_connect_window(ui.ctx());
        if before!=self.quota.config{self.quota.config.save();}
    }
    fn quota_connect_window(&mut self,ctx:&egui::Context){
        if !self.quota.connect_open{return;}let mut open=true;
        egui::Window::new(egui::RichText::new(if self.quota.connect_target.is_some(){"重新连接账号"}else{"添加账号"}).size(16.0).color(crate::settings_theme::color(ctx,Color32::from_gray(235),Color32::from_gray(35)))).id(egui::Id::new("quota-connect-window")).open(&mut open).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER,egui::Vec2::ZERO).default_width(410.0).show(ctx,|ui|{
            ui.set_max_width(410.0);ui.style_mut().wrap_mode=Some(egui::TextWrapMode::Wrap);ui.spacing_mut().item_spacing.y=9.0;ui.visuals_mut().override_text_color=Some(crate::settings_theme::color(ctx,Color32::from_gray(225),Color32::from_gray(35)));
            egui::ScrollArea::vertical().max_height(460.0).show(ui,|ui|{
            ui.add_enabled_ui(self.quota.connect_target.is_none()&&self.quota.workbuddy_connecting.is_none()&&self.quota.deepseek_connecting.is_none()&&self.quota.commandcode_connecting.is_none()&&self.quota.gemini_connecting.is_none()&&self.quota.login.is_none(),|ui|{settings_control_row(ui,"服务",56.0,|ui|{egui::ComboBox::from_id_salt("quota-provider").selected_text(self.quota.provider.name()).show_ui(ui,|ui|{for p in Provider::ALL{ui.selectable_value(&mut self.quota.provider,p,p.name());}}).response});});
            settings_control_row(ui,"备注",56.0,|ui|{ui.add_sized([ui.available_width(),27.0],egui::TextEdit::singleline(&mut self.quota.alias).vertical_align(egui::Align::Center).hint_text("例如：个人、工作、备用"))});
            if self.quota.provider==Provider::WorkBuddy{
                ui.label("连接官网个人账号，查看剩余积分。");
                ui.horizontal(|ui|{
                    if ui.button("打开官网用量页").clicked(){self.pending.push(Cmd::OpenUrl("https://www.codebuddy.cn/profile/usage".into()));}
                    if ui.add_enabled(self.quota.workbuddy_connecting.is_none(),egui::Button::new("登录并连接")).clicked(){
                        let expected=self.quota.connect_target.as_ref().and_then(|id|self.quota.config.accounts.iter().find(|a|&a.id==id)).and_then(|a|a.slot.clone());self.quota.note.clear();self.quota.workbuddy_connecting=Some(quota::workbuddy::Connect::start(self.quota.alias.clone(),expected));
                    }
                });
                ui.label(egui::RichText::new("自动识别本机登录；需要授权时打开默认浏览器。登录状态加密保存在本机。").size(12.0).color(ui.visuals().weak_text_color()));
                if self.quota.workbuddy_connecting.is_some(){ui.horizontal(|ui|{ui.spinner();ui.label("请在默认浏览器完成官网登录…");if ui.button("取消").clicked(){self.quota.workbuddy_connecting=None;self.quota.note="已取消 WorkBuddy 连接".into();}});ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));}
            }else if self.quota.provider==Provider::DeepSeek{
                ui.label("查询 API 账户余额，密钥仅保存在本机。");
                ui.add_sized([ui.available_width(),27.0],egui::TextEdit::singleline(&mut self.quota.deepseek_key).password(true).hint_text("粘贴 API Key"));
                if ui.add_enabled(self.quota.deepseek_connecting.is_none()&&!self.quota.deepseek_key.trim().is_empty(),egui::Button::new("验证并连接")).clicked(){self.quota.note.clear();self.quota.deepseek_connecting=Some(quota::deepseek::Connect::start(self.quota.alias.clone(),std::mem::take(&mut self.quota.deepseek_key)));}
                if self.quota.deepseek_connecting.is_some(){ui.spinner();ui.label("正在连接 DeepSeek…");ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));}
            }else if self.quota.provider==Provider::CommandCode{
                ui.label("读取订阅的 5 小时与每周滚动窗口，密钥仅保存在本机。");
                ui.add_sized([ui.available_width(),27.0],egui::TextEdit::singleline(&mut self.quota.commandcode_key).password(true).hint_text("粘贴 Command Code API Key"));
                if ui.add_enabled(self.quota.commandcode_connecting.is_none()&&!self.quota.commandcode_key.trim().is_empty(),egui::Button::new("验证并连接")).clicked(){self.quota.note.clear();self.quota.commandcode_connecting=Some(quota::commandcode::Connect::start(self.quota.alias.clone(),std::mem::take(&mut self.quota.commandcode_key)));}
                if self.quota.commandcode_connecting.is_some(){ui.spinner();ui.label("正在连接 Command Code…");ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));}
                ui.label(egui::RichText::new("在 commandcode.ai/studio 创建 API Key；与 CLI 使用的是同一把。").size(12.0).color(ui.visuals().weak_text_color()));
            }else if self.quota.provider==Provider::Gemini{
                ui.label(egui::RichText::new("读取 Gemini 网页的 5 小时与每周额度。").size(12.0).color(crate::settings_theme::color(ui.ctx(),Color32::from_gray(180),Color32::from_gray(75))));
                ui.horizontal(|ui|{if ui.button("打开 Gemini 用量").clicked(){self.pending.push(Cmd::OpenUrl("https://gemini.google.com/usage".into()));}if ui.add_enabled(self.quota.gemini_connecting.is_none(),egui::Button::new("连接当前浏览器")).clicked(){self.quota.gemini_connecting=Some(quota::gemini::Connect::start(self.quota.alias.clone(),self.quota.gemini_email.clone(),None,self.quota.connect_target.as_ref().and_then(|id|self.quota.config.accounts.iter().find(|a|&a.id==id)).and_then(|a|a.slot.clone())));}});
                ui.label(egui::RichText::new("浏览器连接需要 Kimi 浏览器扩展；连接后可独立刷新。").size(12.0).color(crate::settings_theme::color(ui.ctx(),Color32::from_gray(180),Color32::from_gray(75))));
                settings_control_row(ui,"Google 邮箱",80.0,|ui|{ui.add_sized([ui.available_width(),27.0],egui::TextEdit::singleline(&mut self.quota.gemini_email).vertical_align(egui::Align::Center).hint_text("多账号时填写要连接的邮箱"))});
                ui.collapsing("使用 Cookie 连接",|ui|{
                    ui.label("在 Gemini 用量页按 F12，打开网络，刷新后找到 executeQuery 请求，复制 Cookie 请求头。");
                    ui.add_sized([ui.available_width(),27.0],egui::TextEdit::singleline(&mut self.quota.gemini_cookie).vertical_align(egui::Align::Center).password(true).hint_text("粘贴 Cookie 请求头"));
                    if ui.add_enabled(self.quota.gemini_connecting.is_none() && !self.quota.gemini_cookie.trim().is_empty(),egui::Button::new("验证并连接")).clicked(){self.quota.gemini_connecting=Some(quota::gemini::Connect::start(self.quota.alias.clone(),self.quota.gemini_email.clone(),Some(self.quota.gemini_cookie.clone()),self.quota.connect_target.as_ref().and_then(|id|self.quota.config.accounts.iter().find(|a|&a.id==id)).and_then(|a|a.slot.clone())));}
                });
            }else if self.quota.provider==Provider::GrokBot{
                ui.horizontal(|ui|{
                    if ui.add_enabled(self.quota.login.is_none(),egui::Button::new("浏览器登录")).clicked(){self.quota_login(self.quota.alias.clone());}
                    if ui.button("从本机导入").clicked(){self.quota.note=match quota::grok::native_path(){Some(path)=>match self.quota.config.import_grok(&path,&self.quota.alias){Ok(n)=>format!("新增 {n} 个 Grok Bot 账号"),Err(e)=>e},None=>"请先安装 Grok Bot 并登录，或使用浏览器连接".into()};}
                });
                ui.collapsing("导入其他登录文件",|ui|{self.quota_file_form(ui);});
                ui.label(egui::RichText::new("使用 Grok Bot 对应的 Cursor 账号登录，浏览器授权支持自动续期。").size(12.0).color(crate::settings_theme::color(ui.ctx(),Color32::from_gray(180),Color32::from_gray(75))));
            }else if self.quota.provider==Provider::ZCode{
                if ui.button("连接本机 ZCode").clicked(){self.quota.note=match quota::zcode::path().ok_or_else(||"未找到 ZCode 登录目录".to_owned()).and_then(|p|quota::zcode::add(&mut self.quota.config,&p,&self.quota.alias)){Ok(())=>"ZCode 已连接".into(),Err(e)=>e};}
            }else if self.quota.provider==Provider::Claude{
                ui.add(egui::Label::new(CLAUDE_LOCAL_NOTICE).wrap());
                ui.horizontal(|ui|{
                    if ui.button("读取桌面版本地记录").clicked(){self.quota.note=match self.quota.config.connect_claude_desktop(&self.quota.alias){Ok(_)=>{self.pending.push(Cmd::QuotaConnected);"已连接，数据随 Claude 桌面版使用更新".into()},Err(e)=>e};}
                    if ui.button("启用 Code 本地采集").clicked(){self.quota.note=match self.quota.config.connect_claude_code(&self.quota.alias){Ok(_)=>{self.pending.push(Cmd::QuotaConnected);"本地采集已启用，使用 Claude Code 后更新".into()},Err(e)=>e};}
                });
                for text in ["桌面版自动从本地用量缓存读取重置时间；缓存不可用时显示未知，额度仍以本地记录为准。","Code 本地采集会配置官方状态栏输出，已有状态栏不会被覆盖。仅使用网页版时，暂无可读取的本机记录。"]{ui.add(egui::Label::new(egui::RichText::new(text).size(12.0).color(ui.visuals().weak_text_color())).wrap());}
                ui.collapsing("选择本地用量 JSON",|ui|{self.quota_file_form(ui);});
                claude_disclaimer(ui);
            }else{
                self.quota_file_form(ui);
                ui.label(egui::RichText::new("各账号使用各自的登录文件；移除只取消监控。登录失效时，请在对应工具中重新登录。").size(12.0).color(crate::settings_theme::color(ui.ctx(),Color32::from_gray(180),Color32::from_gray(75))));
            }
            if !self.quota.note.is_empty(){
                ui.add_space(8.0);
                let color=crate::settings_theme::color(ui.ctx(),Color32::from_rgb(112,190,255),Color32::from_rgb(24,94,160));
                egui::Frame::new().fill(color.gamma_multiply(0.12)).stroke(egui::Stroke::new(1.0,color.gamma_multiply(0.4))).corner_radius(8).inner_margin(10.0).show(ui,|ui|{
                    ui.set_min_width((ui.available_width()-1.0).max(0.0));
                    ui.label(egui::RichText::new(format!("●  {}",self.quota.note)).size(13.0).strong().color(color));
                });
            }

                if self.quota.login.is_some(){ui.horizontal(|ui|{ui.spinner();ui.label("等待浏览器登录…");if ui.button("取消登录").clicked(){self.quota.login=None;self.quota.note="登录已取消".into();}});ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));}
                if self.quota.gemini_connecting.is_some(){ui.horizontal(|ui|{ui.spinner();ui.label("正在连接 Gemini…");});ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));}
            });
        });self.quota.connect_open=open;
        if !open{self.quota.deepseek_key.clear();self.quota.commandcode_key.clear();self.quota.gemini_cookie.clear();}
    }
    fn quota_file_form(&mut self,ui:&mut egui::Ui){
        let local=self.quota.provider==Provider::Claude;
        ui.label(if local{"本地用量 JSON 文件"}else{"登录 JSON 文件"});ui.horizontal(|ui|{ui.add_sized([(ui.available_width()-65.0).max(100.0),27.0],egui::TextEdit::singleline(&mut self.quota.source).vertical_align(egui::Align::Center).hint_text(if local{"选择 Claude 本地用量记录"}else{"选择该账号的登录文件"}));if ui.button("选择…").clicked(){self.pending.push(Cmd::QuotaChooseFile);}});
        if ui.button("添加账号").clicked(){match self.quota.config.add(self.quota.provider,self.quota.alias.clone(),self.quota.source.trim().into()){Ok(())=>{self.quota.alias.clear();self.quota.source.clear();self.quota.note="账号已添加".into();},Err(e)=>self.quota.note=e}}
    }
    #[cfg(feature="record")]
    pub fn quota_demo(&mut self,mode:&str){
        use serde_json::json;
        self.quota=State::default();let reset=chrono::Utc::now().timestamp()+8200;
        for(i,(provider,alias))in [(Provider::Codex,"个人"),(Provider::Codex,"工作"),(Provider::Claude,"Claude")].into_iter().enumerate(){let id=format!("demo-{i}");self.quota.config.accounts.push(Account{id:id.clone(),provider,alias:alias.into(),source:format!("demo-{i}.json").into(),slot:None,enabled:true,pinned:mode=="multi" || i==0});let payload=if provider==Provider::Codex{json!({"plan_type":if i==0{"pro"}else{"plus"},"rate_limit":{"primary_window":if mode=="single" && i==0{serde_json::Value::Null}else{json!({"used_percent":28+i*15,"limit_window_seconds":18000,"reset_at":reset})},"secondary_window":{"used_percent":56,"limit_window_seconds":604800,"reset_at":reset+200000}}})}else{json!({"five_hour":{"utilization":45,"resets_at":"2030-01-01T12:00:00Z"},"seven_day":{"utilization":27}})};self.quota.rows.insert(id,quota::parse(provider,&payload));}
        self.quota.config.selected="demo-0".into();self.show_page(6);if self.settings_open{self.settings_tab=11;}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn claude_local_quota_displays_only_recorded_reset_times(){
        let now=chrono::Local::now();let id="local-fixture".to_string();
        let account=Account{id:id.clone(),provider:Provider::Claude,alias:"离线账号".into(),source:"fixture/config.json".into(),slot:Some("fixture-slot".into()),enabled:true,pinned:true};
        let mut state=State::default();state.config.accounts.push(account.clone());
        let window=quota::Window{label:"5 小时".into(),seconds:Some(18000),remaining:37.5,reset:None};
        let snapshot=Snapshot{at:now.timestamp(),windows:vec![window.clone()],..Default::default()};
        state.receive(quota::Event{id:id.clone(),result:Ok(snapshot.clone())});
        assert_eq!(reset_text(window.reset,&now),"重置时间未知");
        assert_eq!(state.rows[&id].as_ref().unwrap().windows[0],window);
        let later=now+chrono::Duration::minutes(11);
        assert_eq!(reset_text(window.reset,&later),"重置时间未知");
        assert_eq!(state.rows[&id].as_ref().unwrap().windows[0].remaining,37.5,"缺少重置时间不能影响额度百分比");
        let actual=quota::Window{reset:Some(now.timestamp()+1200),remaining:20.0,..window.clone()};
        assert_ne!(reset_text(actual.reset,&now),"重置时间未知");
        state.receive(quota::Event{id:id.clone(),result:Ok(Snapshot{windows:vec![actual.clone()],..snapshot})});
        assert_eq!(state.rows[&id].as_ref().unwrap().windows[0],actual,"本地更新仍应正常生效");
    }
    #[test]
    fn claude_local_records_do_not_restore_old_online_snapshots_or_claim_a_new_update(){
        let path=std::env::temp_dir().join(format!("daozai-claude-cache-{}.json",std::process::id()));
        let mut config=Config::default();let id="claude-local".to_string();config.accounts.push(Account{id:id.clone(),provider:Provider::Claude,alias:"Claude".into(),source:"desktop/config.json".into(),slot:Some("fixture".into()),enabled:true,pinned:true});
        let at=chrono::Utc::now().timestamp()-3600;let snapshot=Snapshot{at,windows:vec![quota::Window{label:"5 小时".into(),seconds:Some(18000),remaining:72.0,reset:None}],..Default::default()};
        std::fs::write(&path,serde_json::json!({&id:{"provider":"Claude","slot":"fixture","snapshot":snapshot}}).to_string()).unwrap();
        let mut state=State::restore(config.clone(),path.clone());assert!(state.rows.is_empty(),"迁移后不能把旧联网缓存显示为本地数据");
        state.receive(quota::Event{id:id.clone(),result:Ok(snapshot)});assert!(state.stale[&id].contains("本地记录较旧"));
        let restored=State::restore(config,path.clone());assert_eq!(restored.rows[&id].as_ref().unwrap().at,at,"读取缓存不能刷新官方记录时间");std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn claude_verified_history_survives_client_exit_and_island_restart(){
        let path=std::env::temp_dir().join(format!("daozai-claude-exit-{}.json",std::process::id()));let mut config=Config::default();let id="claude-closed".to_string();config.accounts.push(Account{id:id.clone(),provider:Provider::Claude,alias:"Claude".into(),source:"desktop/config.json".into(),slot:Some("verified-owner".into()),enabled:true,pinned:true});
        let at=chrono::Utc::now().timestamp()-60;let snapshot=Snapshot{at,windows:vec![quota::Window{label:"本周".into(),seconds:Some(604800),remaining:16.0,reset:Some(at+86400)}],..Default::default()};
        let mut state=State::restore(config.clone(),path.clone());state.receive(quota::Event{id:id.clone(),result:Ok(snapshot.clone())});state.receive(quota::Event{id:id.clone(),result:Err("客户端已关闭，等待新记录".into())});assert_eq!(state.rows[&id].as_ref().unwrap().windows,snapshot.windows);assert!(state.stale.contains_key(&id));
        let mut restarted=State::restore(config,path.clone());assert_eq!(restarted.rows[&id].as_ref().unwrap().windows,snapshot.windows);restarted.receive(quota::Event{id:id.clone(),result:Err("等待新记录".into())});assert_eq!(restarted.rows[&id].as_ref().unwrap().windows,snapshot.windows);assert_eq!(restarted.rows[&id].as_ref().unwrap().at,at);std::fs::remove_file(path).unwrap();
    }
    #[test] fn side_amounts_stay_short(){
        for (value,symbol,expected) in [(4.18,"¥","¥4.1"),(999.9,"¥","¥999"),(9999.0,"¥","¥9.99k"),(28560.0,"","2.85万"),(123456.7,"¥","¥12.3万"),
            (1234567.8,"¥","¥123万"),(99999999.0,"¥","¥0.99亿"),(12345678901.0,"","123亿"),(-12.5,"¥","-¥12.5"),(0.0,"","0"),(0.04,"¥","¥0")] {
            assert_eq!(side_amount(value,symbol),expected,"{value}");
        }
        assert!(side_amount(f64::NAN,"¥").contains("--"));
    }
    #[test] fn deepseek_missing_balance_distinguishes_loading_and_failure(){
        let mut state=State::default();assert!(state.balance_loading("test"));assert!(state.balance_text("test").is_none());
        state.rows.insert("test".into(),Err("无法连接".into()));assert!(!state.balance_loading("test"));assert!(state.balance_text("test").is_none());
        state.busy.insert("test".into());assert!(state.balance_loading("test"));
    }
    #[test] fn zcode_badge_has_clear_corners_and_a_visible_edge(){
        let icon=egui_extras::image::load_svg_bytes_with_size(include_bytes!("../assets/ai-icons/zcode.svg"),Some(egui::load::SizeHint::Size(26,26))).unwrap();
        for (x,y) in [(3,3),(22,3),(3,22),(22,22)] {assert_eq!(icon[(x,y)].a(),0);}
        assert!(icon[(13,1)].a()>180);
        assert!(icon.pixels.iter().filter(|p|p.r()>230&&p.a()>240).count()>35);
    }
    #[test] fn deepseek_badge_keeps_brand_blue_in_every_skin(){
        use crate::model::Skin;
        for skin in [Skin::Classic,Skin::Cream,Skin::GlassLight,Skin::GlassDark]{
            let image=egui_extras::image::load_svg_bytes_with_size(deepseek_badge_svg(skin).as_bytes(),Some(egui::load::SizeHint::Size(100,100))).unwrap();
            assert!(image.pixels.iter().any(|p|*p==Color32::from_rgb(77,107,254)));
            let id=egui::TextureId::User(0xDEE5);crate::skin_paint::mark_monochrome(id);crate::skin_paint::mark_colored(id);
            let mut mesh=egui::Mesh::with_texture(id);mesh.add_rect_with_uv(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(24.0,24.0)),egui::Rect::from_min_max(egui::Pos2::ZERO,egui::pos2(1.0,1.0)),Color32::WHITE);
            let mut shape=egui::Shape::mesh(mesh);crate::skin_paint::recolor(&mut shape,&crate::skin_paint::CREAM);
            if let egui::Shape::Mesh(mesh)=shape{assert!(mesh.vertices.iter().all(|v|v.color==Color32::WHITE));}else{panic!("徽标应保留纹理");}
        }
    }
    #[test]
    #[ignore="只读验证本机已启用账号冷启动时可恢复额度"]
    fn quota_live_cold_start(){
        let state=State::restore(Config::load(),crate::model::config_dir().join("ai-quota-cache.json"));
        let enabled:Vec<_>=state.config.accounts.iter().filter(|a|a.enabled).collect();
        assert!(!enabled.is_empty());
        for a in enabled {
            assert!(matches!(state.rows.get(&a.id),Some(Ok(_))),"{} 缺少上次记录",a.provider.name());
            assert!(state.stale.contains_key(&a.id));
            println!("{} 已从磁盘恢复",a.provider.name());
        }
    }
    #[test] fn restart_restores_last_balance_only_for_the_bound_account(){
        let path=std::env::temp_dir().join(format!("daozai-quota-restart-{}.json",std::process::id()));
        let mut config=Config::default();
        let id=config.connect_workbuddy("fixture".into(),"测试账号".into());
        let mut state=State::restore(config.clone(),path.clone());
        let snapshot=quota::workbuddy::parse(&serde_json::json!({"code":0,"data":{"Packages":[{"CycleRemainCapacity":2000}]}})).unwrap();
        state.receive(quota::Event{id:id.clone(),result:Ok(snapshot)});
        let mut restored=State::restore(config.clone(),path.clone());
        assert_eq!(restored.balance_text(&id).as_deref(),Some("2k"));
        assert!(restored.stale.contains_key(&id));
        restored.receive(quota::Event{id:id.clone(),result:Err("浏览器未启动".into())});
        assert_eq!(restored.balance_text(&id).as_deref(),Some("2k"));
        config.accounts[0].slot=Some("不同账号".into());
        assert!(State::restore(config,path.clone()).rows.is_empty());
        std::fs::remove_file(path).unwrap();
    }
    #[test] fn deepseek_refresh_failure_keeps_balance_until_success(){
        let mut state=State::default();let id=state.config.connect_deepseek("test".into(),"DeepSeek".into());
        let snapshot=quota::deepseek::parse(&serde_json::json!({"is_available":true,"balance_infos":[{"currency":"CNY","total_balance":"12800.00","granted_balance":"800","topped_up_balance":"12000"}]})).unwrap();
        state.receive(quota::Event{id:id.clone(),result:Ok(snapshot.clone())});state.receive(quota::Event{id:id.clone(),result:Err("连接失败".into())});
        assert_eq!(state.rows[&id].as_ref().unwrap().wallet.as_ref().unwrap().primary().total,12800.0);assert!(state.stale.contains_key(&id));
        state.receive(quota::Event{id:id.clone(),result:Ok(snapshot)});assert!(!state.stale.contains_key(&id));
    }

    use chrono::TimeZone;
    #[test]
    fn quota_logo_matches_screen_pixels_at_each_display_scale(){
        for scale in [1.0,1.25,1.5,1.75,2.0] {
            let ctx=egui::Context::default();egui_extras::install_image_loaders(&ctx);
            for provider in Provider::ALL {for size in [10.0,12.0,20.0,24.0] {
                let raw=egui::RawInput{screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(120.0,80.0))),viewports:std::iter::once((egui::ViewportId::ROOT,egui::ViewportInfo{native_pixels_per_point:Some(scale),..Default::default()})).collect(),..Default::default()};
                let output=ctx.run(raw,|ctx|{egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx,|ui|{
                    let c=Card::new(ui,egui::pos2(0.37,0.21),ui.max_rect());logo(&c,provider,32.25,32.5,size);
                });});
                let mesh=output.shapes.iter().find_map(|s|match &s.shape{egui::Shape::Mesh(mesh)=>Some(mesh),_=>None}).expect("应绘制厂商图标");
                let image=&output.textures_delta.set.iter().find(|(id,_)|*id==mesh.texture_id).expect("不同尺寸应各自生成纹理").1.image;
                let pixels=(size*scale).round() as usize;
                assert_eq!(image.size(),[pixels,pixels],"图标应按屏幕像素生成，厂商={}，缩放={scale}",provider.name());
                for v in &mesh.vertices {
                    assert!((v.pos.x*scale-(v.pos.x*scale).round()).abs()<0.001 && (v.pos.y*scale-(v.pos.y*scale).round()).abs()<0.001,"图标边界应对齐物理像素");
                }
                if let egui::ImageData::Color(image)=image {for pixel in &image.pixels {let expected=if matches!(provider,Provider::WorkBuddy|Provider::ZCode){*pixel}else if provider==Provider::DeepSeek{Color32::from_rgba_unmultiplied(77,107,254,pixel.a())}else{Color32::from_white_alpha(pixel.a())};assert_eq!(*pixel,expected,"标识颜色与透明边缘应保持一致");}}
            }}
        }
    }
    #[test]
    fn quota_reset_uses_local_calendar_dates_and_clock_times(){
        let now=chrono::Local.with_ymd_and_hms(2026,10,31,22,0,0).unwrap();
        for (parts,expected) in [
            ((2026,10,31,23,30),"今天 23:30 重置"),
            ((2026,11,1,9,0),"11月1日 09:00 重置"),
            ((2026,11,3,9,0),"11月3日 09:00 重置"),
            ((2027,1,1,9,0),"2027/1/1 09:00 重置"),
        ] {
            let (year,month,day,hour,minute)=parts;
            let at=chrono::Local.with_ymd_and_hms(year,month,day,hour,minute,0).unwrap().timestamp();
            assert_eq!(reset_text(Some(at),&now),expected);
        }
    }
    #[test]
    fn quota_reset_does_not_invent_missing_or_expired_times(){
        let now=chrono::Local.with_ymd_and_hms(2026,10,31,22,0,0).unwrap();
        assert_eq!(reset_text(None,&now),"重置时间未知");
        assert_eq!(reset_text(Some(i64::MAX),&now),"重置时间未知");
        assert_eq!(reset_text(Some(now.timestamp()),&now),"等待刷新");
        assert_eq!(reset_text(Some(now.timestamp()-1),&now),"等待刷新");
    }
}

// ---------------------------------------------------------------- 左右停靠：竖向额度
// 侧边停靠时额度竖排在胶囊里：环形额度一个圆环一行；余额类（DeepSeek、WorkBuddy）图标在上、金额在下。
pub(crate) const SIDE_QUOTA_RING_H:f32=30.0;

/// 侧边竖排的金额：4.1、999、9.99k、99.9万、999万、0.12亿……截断不进位，避免显示得比实际多。
pub(crate) fn side_amount(value:f64,symbol:&str)->String{
    if !value.is_finite(){return format!("{symbol}--");}
    let n=value.abs();
    let (mut v,mut unit)=if n>=100_000_000.0{(n/100_000_000.0,"亿")}else if n>=10_000.0{(n/10_000.0,"万")}else if n>=1000.0{(n/1000.0,"k")}else{(n,"")};
    if unit=="万" && v>=1000.0 {v=n/100_000_000.0;unit="亿";}
    let digits=if unit.is_empty(){if v<100.0{1}else{0}}else if v<10.0{2}else if v<100.0{1}else{0};
    let factor=10f64.powi(digits);
    let text=format!("{:.*}",digits as usize,(v*factor).trunc()/factor);
    let text=if text.contains('.'){text.trim_end_matches('0').trim_end_matches('.').to_string()}else{text};
    format!("{}{symbol}{text}{unit}",if value<0.0 && text!="0"{"-"}else{""})
}
pub(crate) const SIDE_QUOTA_BALANCE_H:f32=38.0;
impl App {
    pub(crate) fn quota_side_items(&self)->Vec<(usize,f32)>{
        self.quota_bar_indices().into_iter().map(|i|{
            let balance=self.quota.config.accounts[i].provider.balance();
            (i,if balance{SIDE_QUOTA_BALANCE_H}else{SIDE_QUOTA_RING_H})
        }).collect()
    }

    pub(crate) fn draw_quota_side(&mut self,c:&Card,cx:f32,mid:f32,height:f32,i:usize){
        let Some(a)=self.quota.config.accounts.get(i).cloned() else{return};
        let snapshot=self.quota.rows.get(&a.id).and_then(|r|r.as_ref().ok()).cloned();
        let summary;
        if a.provider.balance(){
            let icon_y=mid-7.0;
            if a.provider==Provider::WorkBuddy{logo(c,a.provider,cx,icon_y,20.0);}else{deepseek_badge(c,self.settings.skin,cx,icon_y,22.0);}
            let loading=self.quota.balance_loading(&a.id);let stale=self.quota.stale.contains_key(&a.id);
            if stale||(!loading&&snapshot.is_none()){
                c.circle(cx+8.0,icon_y+8.0,3.6,Color32::BLACK);
                c.circle(cx+8.0,icon_y+8.0,2.4,Color32::from_rgb(240,165,84));
            }
            if let Some(text)=self.quota.balance_text(&a.id){
                let wallet=snapshot.as_ref().and_then(|s|s.wallet.as_ref());
                let color=if stale{theme::GRAY}else if wallet.is_some_and(|w|!w.available||w.primary().total<=0.0){Color32::from_rgb(255,166,77)}else{theme::WHITE};
                // 竖向胶囊只有 34 点宽：用更短的写法（最多约 4 位有效字符），放不下再缩小字号。
                let short=snapshot.as_ref().and_then(|s|s.wallet.as_ref().map(|w|side_amount(w.primary().total,w.primary().symbol()))
                    .or_else(||s.credits.as_ref().map(|c|side_amount(c.remaining,"")))).unwrap_or_else(||text.clone());
                let size=[9.5,8.5,7.5].into_iter().find(|&size|c.text_w(&short,size,W::Bold)<=34.0).unwrap_or(7.5);
                let shown=c.fit(&short,size,W::Bold,34.0);
                c.text(cx,mid+15.0,&shown,size,W::Bold,color,H::Center);
                summary=format!("余额 {text}");
            }else if loading{
                let active=(self.t*3.0) as usize%3;
                for dot in 0..3{c.circle(cx-5.0+dot as f32*5.0,mid+11.0,1.5,theme::GRAY.gamma_multiply(if dot==active{1.0}else{0.35}));}
                c.ui.ctx().request_repaint_after(std::time::Duration::from_millis(160));
                summary="正在更新".into();
            }else{summary="余额暂不可用".into();}
        }else{
            ring(c,&a,snapshot.as_ref(),cx,mid,26.0,accent_colors(&self.quota.config,&a));
            let mut same=self.quota.config.accounts.iter().filter(|other|other.provider==a.provider && other.enabled);
            if same.clone().count()>1{let n=same.position(|other|other.id==a.id).unwrap_or(0)+1;c.circle(cx+10.0,mid+9.0,4.5,Color32::from_gray(225));c.text(cx+10.0,mid+11.8,&n.to_string(),6.5,W::Bold,Color32::BLACK,H::Center);}
            summary=snapshot.as_ref().map(|s|s.rings().iter().map(|w|format!("{}剩余 {:.0}%",w.label,w.remaining)).collect::<Vec<_>>().join(" · ")).unwrap_or_else(||if self.quota.busy.contains(&a.id){"正在更新".into()}else{"额度暂不可用".into()});
        }
        let hit=c.hit(cx-17.0,mid-height/2.0,34.0,height,("quota-side",&a.id));
        if bar_tooltip(hit,format!("{} · {}\n{}",a.provider.name(),a.alias,summary)).clicked(){
            self.quota.config.selected=a.id.clone();self.quota.config.save();self.show_page(6);
        }
    }
}
