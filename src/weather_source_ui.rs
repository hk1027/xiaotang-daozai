//! 天气来源草稿、连接验证与安全启用。
use crate::{app::{App,WEATHER_PAGE},extras::{settings_card,settings_hint,settings_label},settings_theme::color,qweather::{self,Client,Config,Error,Location}};
use eframe::egui::{self,Color32,RichText};
use std::sync::{Arc,atomic::{AtomicBool,Ordering},mpsc::Receiver};
pub(crate) use qweather::{Provider,normalize_host};
pub(crate) fn updated_at(at:i64)->String {
    if at<=0{return "未取得".into();}
    chrono::DateTime::from_timestamp(at,0).map(|d|d.with_timezone(&chrono::Local).format("%m-%d %H:%M").to_string()).unwrap_or_else(||"未取得".into())
}

/// 来源选项：地域图标、来源名、短标签；整张卡片都可点击。
pub(crate) fn provider_option(ui:&mut egui::Ui,provider:Provider,selected:bool,width:f32)->egui::Response {
    let (rect,response)=ui.allocate_exact_size(egui::vec2(width,66.0),egui::Sense::click());
    let ctx=ui.ctx();let painter=ui.painter();
    let accent=color(ctx,Color32::from_rgb(111,174,255),Color32::from_rgb(48,113,211));
    let base=color(ctx,Color32::from_rgb(37,37,41),Color32::WHITE);
    let fill=if selected{crate::paint::mix(base,accent,0.10)}else if response.hovered(){color(ctx,Color32::from_rgb(45,45,50),Color32::from_rgb(246,248,252))}else{base};
    let border=if selected||response.has_focus(){accent}else{color(ctx,Color32::from_rgb(65,65,71),Color32::from_rgb(221,226,234))};
    painter.rect(rect,9.0,fill,egui::Stroke::new(if selected{1.3}else{1.0},border),egui::StrokeKind::Inside);
    let overseas=provider==Provider::OpenMeteo;
    let title=if overseas{"Open-Meteo"}else{"和风天气"};
    let ink=if overseas{accent}else{color(ctx,Color32::from_rgb(111,209,177),Color32::from_rgb(29,139,105))};
    let icon=egui::Rect::from_min_size(rect.min+egui::vec2(10.0,10.0),egui::vec2(24.0,24.0));
    painter.rect_filled(icon,6.0,crate::paint::mix(fill,ink,0.15));
    let center=icon.center();let stroke=egui::Stroke::new(1.2,ink);
    if overseas {
        painter.circle_stroke(center,7.0,stroke);
        painter.line_segment([center+egui::vec2(-7.0,0.0),center+egui::vec2(7.0,0.0)],stroke);
        painter.line_segment([center+egui::vec2(0.0,-7.0),center+egui::vec2(0.0,7.0)],stroke);
        for sign in [-1.0,1.0]{painter.add(egui::Shape::line(vec![center+egui::vec2(0.0,-7.0),center+egui::vec2(sign*3.0,-3.5),center+egui::vec2(sign*3.0,3.5),center+egui::vec2(0.0,7.0)],stroke));}
    }else {
        let mut points=vec![center+egui::vec2(0.0,9.0)];
        for index in 0..=16 {let angle=std::f32::consts::PI/6.0-index as f32/16.0*std::f32::consts::PI*4.0/3.0;points.push(center+egui::vec2(angle.cos()*6.0,angle.sin()*6.0-2.0));}
        painter.add(egui::Shape::closed_line(points,stroke));painter.circle_filled(center+egui::vec2(0.0,-2.0),1.8,ink);
    }
    let title_size=if width<185.0{11.5}else{13.0};
    painter.text(rect.min+egui::vec2(42.0,20.0),egui::Align2::LEFT_CENTER,title,egui::FontId::proportional(title_size),ui.visuals().text_color());
    let badge=if overseas{"默认"}else if width<210.0{"Key"}else{"自备 Key"};
    let badge_ink=if overseas{ui.visuals().weak_text_color()}else{color(ctx,Color32::from_rgb(235,194,127),Color32::from_rgb(143,93,29))};
    let galley=painter.layout_no_wrap(badge.into(),egui::FontId::proportional(9.0),badge_ink);
    let badge_rect=egui::Rect::from_min_size(egui::pos2(rect.right()-10.0-galley.size().x-10.0,rect.top()+12.0),galley.size()+egui::vec2(10.0,6.0));
    painter.rect_filled(badge_rect,4.0,crate::paint::mix(fill,badge_ink,0.10));painter.galley(badge_rect.min+egui::vec2(5.0,3.0),galley,badge_ink);
    let mut x=rect.left()+42.0;
    for (label,fee) in [(if overseas{"海外"}else{"国内"},false),(if overseas{"免费"}else{"免费额度"},true)]{
        let ink=if fee{color(ctx,Color32::from_rgb(130,211,168),Color32::from_rgb(30,127,79))}else{ui.visuals().weak_text_color()};
        let galley=painter.layout_no_wrap(label.into(),egui::FontId::proportional(10.0),ink);
        let pill=egui::Rect::from_min_size(egui::pos2(x,rect.top()+40.0),galley.size()+egui::vec2(10.0,6.0));
        painter.rect_filled(pill,4.0,crate::paint::mix(fill,ink,if fee{0.10}else{0.06}));painter.galley(pill.min+egui::vec2(5.0,3.0),galley,ink);x=pill.right()+5.0;
    }
    response.widget_info(||egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel,ui.is_enabled(),selected,title));
    response.on_hover_text(if overseas{"海外源，默认免费使用，无需 Key。"}else{"和风天气：国内源·精准。自备 API Host 与 Key；有免费额度，超额可能收费。"})
}

#[derive(Default)]
pub(crate) struct State {
    pub selected:Provider,
    pub displayed:Provider,
    pub host:String,
    pub key:String,
    pub message:Option<(&'static str,bool)>,
    pub tutorial_open:bool,
    pub tutorial_zoom:f32,
    tutorial_textures:Vec<egui::TextureHandle>,
    tutorial_step10:usize,
    pub config:Config,
    pub runtime:qweather::Runtime,
    pub client:Option<Client>,
    tested:Option<Client>,
    testing:Option<Receiver<Result<Client,Error>>>,
    test_error:Option<Error>,
    focus_city:bool,
    draft_gate:Option<Arc<AtomicBool>>,
    search:Option<Receiver<Result<qweather::Search,Error>>>,
    geo_credits:Vec<String>,
    pub query:String,
    pub province:String,
    results:Vec<Location>,
    city_choice:Option<Location>,
    search_input:(String,String),
    search_changed_at:f64,
    search_sent:bool,
    search_error:Option<Error>,
    highlighted:usize,
    pub picked:Option<Location>,
    pub config_open:bool,
}
impl State {
    pub fn load()->Self {
        if cfg!(test){return Self::default();}
        let config=Config::load();let gate=Arc::new(AtomicBool::new(true));
        let client=Client::load(&config.host,&config.credential,gate);
        let mut runtime=qweather::Runtime::default();runtime.reset(&config.host,config.location.clone());
        let active=config.provider;
        Self{selected:active,displayed:active,host:config.host.clone(),picked:config.location.clone(),config,runtime,client,..Default::default()}
    }
    fn invalidate(&mut self){
        if let Some(gate)=self.draft_gate.take(){gate.store(false,Ordering::SeqCst);}
        self.testing=None;self.tested=None;self.test_error=None;self.reset_search();self.message=None;
    }
    pub(crate) fn close(&mut self){self.invalidate();self.key.clear();self.query.clear();self.tutorial_open=false;self.tutorial_textures.clear();}
    fn candidate(&self)->Result<Client,Error>{
        let host=normalize_host(&self.host).map_err(|_|Error::Invalid)?;
        if self.key.is_empty(){if host!=self.config.host{return Err(Error::Invalid);}Client::load(&host,&self.config.credential,Arc::new(AtomicBool::new(true))).ok_or(Error::Invalid)}
        else{Client::new(&host,&self.key,Arc::new(AtomicBool::new(true)))}
    }
    fn test_connection(&mut self){
        self.invalidate();
        match self.candidate(){
            Err(e)=>{self.test_error=Some(e);self.message=Some((e.text(),true));},
            Ok(client)=>{
                self.host=client.host.clone();self.draft_gate=Some(client.gate.clone());
                if !cfg!(test){self.testing=Some(qweather::spawn(move||{client.test()?;Ok(client)}));}
                else {self.message=Some(("测试使用模拟响应，不发送真实请求。",false));}
            },
        }
    }
    pub fn poll(&mut self,enabled:bool){
        if let Some(c)=&mut self.client {
            let active=enabled&&self.displayed==Provider::Qweather;
            if !active&&c.gate.swap(false,Ordering::SeqCst){c.gate=Arc::new(AtomicBool::new(false));}
            else {c.gate.store(active,Ordering::SeqCst);}
        }
        if let Some(g)=&self.draft_gate {g.store(enabled,Ordering::SeqCst);}
        if !enabled {self.invalidate();return;}
        if let Some(result)=self.testing.as_ref().and_then(|rx|rx.try_recv().ok()){
            self.testing=None;
            match result {Ok(client)=>{self.tested=Some(client);self.focus_city=true;self.message=None;},Err(e)=>{self.test_error=Some(e);self.message=Some((e.text(),true));}}
        }
        if let Some(result)=self.search.as_ref().and_then(|rx|rx.try_recv().ok()){
            self.search=None;
            match result {Ok(list)=>{self.results=list.locations;self.geo_credits=list.credits;self.highlighted=0;},Err(e)=>self.search_error=Some(e)}
        }
    }
    fn configured(&self)->bool {self.tested.is_some()||(self.test_error.is_none()&&self.displayed==Provider::Qweather&&self.client.is_some()&&self.key.is_empty()&&self.host==self.config.host)}
    fn reset_search(&mut self){self.search=None;self.results.clear();self.geo_credits.clear();self.city_choice=None;self.search_error=None;self.search_sent=false;self.highlighted=0;}
    fn search_due(&mut self,now:f64)->bool {
        let input=(self.query.trim().to_string(),self.province.trim().to_string());
        if input!=self.search_input {self.reset_search();self.search_input=input;self.search_changed_at=now;}
        let query=&self.search_input.0;
        let enough=if query.is_ascii(){query.chars().count()>=2}else{!query.is_empty()};
        self.configured()&&enough&&!self.search_sent&&self.search.is_none()&&now-self.search_changed_at>=0.45
    }
    fn search(&mut self){
        if !self.configured()||self.search.is_some(){return;}
        let Some(c)=self.tested.as_ref().or(self.client.as_ref()).cloned()else{return;};
        let query=self.query.trim().to_string();if query.is_empty(){return;}
        let adm=self.province.trim().to_string();self.results.clear();self.geo_credits.clear();self.search_error=None;self.search_sent=true;
        if !cfg!(test){self.search=Some(qweather::spawn(move||c.search(&query,&adm)));}
    }
    fn choose_city(&mut self,location:Location){self.highlighted=self.results.iter().position(|l|l==&location).unwrap_or(0);self.query=location.position.label.clone();self.search_input=(self.query.clone(),self.province.trim().into());self.search_sent=true;self.city_choice=Some(location);}
    fn remember_city(&mut self,location:Location)->Result<bool,&'static str>{
        if self.config.saved.contains(&location){return Ok(false);}
        if self.config.saved.len()>=crate::weather::MAX_CITIES{return Err("已保存 5 个城市，请先移除一个再添加。");}
        let mut config=self.config.clone();config.saved.push(location);config.save().map_err(Error::text)?;self.config=config;Ok(true)
    }
    fn connection_status(&self,ui:&mut egui::Ui){
        let (title,detail,dark,light)=if self.testing.is_some(){("正在测试连接","正在检查城市搜索、实况和七日预报…",Color32::from_rgb(29,48,70),Color32::from_rgb(232,241,255))}
        else if let Some(error)=self.test_error {("连接测试失败",error.text(),Color32::from_rgb(69,32,35),Color32::from_rgb(255,235,235))}
        else if self.tested.is_some(){("连接测试通过","城市搜索、实况和七日预报可用。接下来选择城市，保存并启用。",Color32::from_rgb(24,60,44),Color32::from_rgb(228,247,237))}
        else if self.configured(){("QWeather 已启用","凭据已安全保存，可直接添加或切换城市。",Color32::from_rgb(24,60,44),Color32::from_rgb(228,247,237))}
        else{return;};
        let ink=if self.test_error.is_some(){color(ui.ctx(),Color32::from_rgb(255,155,155),Color32::from_rgb(174,46,46))}else if self.testing.is_some(){color(ui.ctx(),Color32::from_rgb(139,188,255),Color32::from_rgb(36,100,181))}else{color(ui.ctx(),Color32::from_rgb(123,225,169),Color32::from_rgb(25,120,76))};
        ui.add_space(7.0);
        egui::Frame::new().fill(color(ui.ctx(),dark,light)).stroke(egui::Stroke::new(1.0,ink.gamma_multiply(0.35))).corner_radius(8.0).inner_margin(10).show(ui,|ui|{
            ui.set_min_width(ui.available_width());ui.label(RichText::new(title).size(13.5).strong().color(ink));ui.add(egui::Label::new(RichText::new(detail).size(11.5)).wrap());
        });
    }
}

impl App {
    fn apply_weather_source(&mut self){
        if !self.module_on(WEATHER_PAGE){return;}
        let source=&mut self.weather_source;
        let mut config=source.config.clone();config.provider=source.selected;
        if source.selected==Provider::Qweather {
            let Some(client)=source.tested.clone()else{return;};
            let Some(location)=source.city_choice.as_ref().or(source.picked.as_ref()).cloned()else{return;};
            config.host=client.host.clone();config.location=Some(location.clone());
            if !config.saved.contains(&location){if config.saved.len()>=crate::weather::MAX_CITIES{source.message=Some(("已保存五个和风城市，请先删除一个。",true));return;}config.saved.push(location);}
            // 按 Host 隔离凭据，新凭据保存失败时不触碰当前来源。
            let old_slot=config.credential.clone();
            config.credential=format!("{}-{}",client.host,chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default());
            if let Err(e)=client.store(&config.credential){source.message=Some((e.text(),true));return;}
            if let Err(e)=config.save(){let _=Client::delete(&config.credential);source.message=Some((e.text(),true));return;}
            if !old_slot.is_empty(){let _=Client::delete(&old_slot);}
            if let Some(old)=&source.client {old.gate.store(false,Ordering::SeqCst);}
            let mut client=client;client.gate=Arc::new(AtomicBool::new(true));source.client=Some(client);
            source.runtime.reset(&config.host,config.location.clone());
            self.weather.switch("",None,chrono::Utc::now().timestamp());self.weather.snapshot=source.runtime.snapshot();
        }else{
            if let Err(e)=config.save(){source.message=Some((e.text(),true));return;}
            if let Some(c)=&source.client{c.gate.store(false,Ordering::SeqCst);}
            self.weather=crate::weather::State::load(&self.settings.weather_city,self.settings.weather_position.as_ref());
        }
        source.config=config;source.displayed=source.selected;source.key.clear();source.invalidate();source.config_open=false;
        source.message=Some(("已应用，天气与城市列表已切换到所选来源。",false));
    }

    pub(crate) fn qweather_select_location(&mut self,location:Location){
        if !self.module_on(WEATHER_PAGE){return;}
        let mut config=self.weather_source.config.clone();config.location=Some(location);
        if config.save().is_err(){self.weather_source.message=Some((Error::Storage.text(),true));return;}
        self.weather_source.config=config;self.qweather_sync_location();
    }

    fn qweather_sync_location(&mut self){
        self.weather_source.picked=self.weather_source.config.location.clone();
        if let Some(c)=&mut self.weather_source.client{c.gate.store(false,Ordering::SeqCst);c.gate=Arc::new(AtomicBool::new(true));}
        self.weather_source.runtime.reset(&self.weather_source.config.host,self.weather_source.config.location.clone());
        self.weather.snapshot=self.weather_source.runtime.snapshot();self.weather.error=None;self.weather.loading=false;
    }

    pub(crate) fn qweather_add_city(&mut self){
        if !self.module_on(WEATHER_PAGE)||!self.weather_source.configured(){return;}
        let Some(location)=self.weather_source.city_choice.clone()else{return;};
        match self.weather_source.remember_city(location.clone()) {
            Err(error)=>self.weather_source.message=Some((error,true)),
            Ok(_)=>{self.weather_source.message=None;if self.weather_source.picked.is_none()&&self.weather_source.displayed!=Provider::Qweather{self.weather_source.picked=Some(location);}self.weather_source.query.clear();self.weather_source.reset_search();},
        }
    }

    pub(crate) fn qweather_remove_city(&mut self,location:&Location){
        if !self.module_on(WEATHER_PAGE){return;}
        let mut config=self.weather_source.config.clone();config.saved.retain(|l|l!=location);
        let active_changed=config.location.as_ref()==Some(location);
        if active_changed{config.location=config.saved.first().cloned();}
        if let Err(error)=config.save(){self.weather_source.message=Some((error.text(),true));return;}
        self.weather_source.config=config;self.weather_source.message=None;
        if self.weather_source.picked.as_ref()==Some(location){self.weather_source.picked=self.weather_source.config.location.clone().or_else(||self.weather_source.config.saved.first().cloned());}
        if active_changed&&self.weather_source.displayed==Provider::Qweather{self.qweather_sync_location();}
        self.weather_source.query.clear();self.weather_source.reset_search();
    }

    pub(crate) fn weather_source_settings(&mut self,ui:&mut egui::Ui){
        settings_label(ui,"天气来源");
        settings_card(ui,|ui|{
            let width=(ui.available_width()-ui.spacing().item_spacing.x)/2.0;
            ui.horizontal(|ui|{
                for provider in [Provider::OpenMeteo,Provider::Qweather]{
                    let selected=self.weather_source.selected==provider;
                    if provider_option(ui,provider,selected,width).clicked()&& !selected{
                        self.weather_source.selected=provider;self.weather_source.message=None;
                        self.weather_source.query.clear();self.weather_source.results.clear();
                        self.weather_source.picked=if provider==Provider::Qweather{self.weather_source.config.location.clone()}else{None};
                        if provider==Provider::Qweather&&self.weather_source.client.is_none(){self.weather_source.config_open=true;}
                    }
                }
            });
            ui.add_space(5.0);
            settings_hint(ui,&format!("当前生效：{}{}",self.weather_source.displayed.name(),if self.weather_source.selected!=self.weather_source.displayed{" · 所选来源尚未应用"}else{""}));
            if self.weather_source.selected==Provider::OpenMeteo {
                settings_hint(ui,"无需 Key。点击应用后使用 Open-Meteo 自己的城市与定位设置。");
                if self.weather_source.displayed!=Provider::OpenMeteo&&ui.button("应用 Open-Meteo").clicked(){self.apply_weather_source();}
                return;
            }
            ui.add_space(4.0);ui.separator();ui.add_space(3.0);
            ui.horizontal_wrapped(|ui|{
                ui.label(RichText::new("和风配置").strong());
                if ui.small_button(if self.weather_source.config_open{"收起"}else{"展开"}).clicked(){self.weather_source.config_open=!self.weather_source.config_open;}
                ui.hyperlink_to("配置网站 ↗","https://console.qweather.com/");
                if ui.link("配置教程 ↗").clicked(){self.weather_source.tutorial_open=true;}
            });
            if !self.weather_source.config_open {self.weather_source.connection_status(ui);}
            if self.weather_source.config_open {
                ui.add_space(5.0);
                ui.label(RichText::new("API Host").size(12.0));
                let host=ui.add(egui::TextEdit::singleline(&mut self.weather_source.host).id_salt("qweather-host").hint_text("专属域名.qweatherapi.com").desired_width(f32::INFINITY));
                ui.label(RichText::new("API Key").size(12.0));
                let stored=self.weather_source.client.as_ref().is_some_and(|c|c.host==self.weather_source.host);
                let key=ui.add(egui::TextEdit::singleline(&mut self.weather_source.key).id_salt("qweather-key").password(true).hint_text(if stored{"已安全保存；填写新 Key 可更换"}else{"粘贴专门为岛仔创建的 API Key"}).desired_width(f32::INFINITY));
                if host.changed()||key.changed(){self.weather_source.invalidate();}
                ui.add_space(5.0);
                ui.horizontal_wrapped(|ui|{
                    if ui.add_enabled(self.weather_source.testing.is_none(),egui::Button::new("测试连接")).clicked(){self.weather_source.test_connection();}
                    if self.weather_source.testing.is_some(){ui.spinner();ui.label("测试中…");ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));}
                    if ui.add_enabled(stored||!self.weather_source.key.is_empty(),egui::Button::new("清除本机 Key")).clicked(){
                        if self.weather_source.displayed==Provider::Qweather {
                            self.weather_source.selected=Provider::OpenMeteo;self.apply_weather_source();self.weather_source.selected=Provider::Qweather;
                            if self.weather_source.displayed!=Provider::OpenMeteo{return;}
                        }
                        let slot=self.weather_source.config.credential.clone();
                        let removed=if slot.is_empty(){Ok(())}else{Client::delete(&slot)};
                        match removed {Err(e)=>self.weather_source.message=Some((e.text(),true)),Ok(())=>{
                            if let Some(c)=self.weather_source.client.take(){c.gate.store(false,Ordering::SeqCst);}
                            self.weather_source.key.clear();self.weather_source.invalidate();self.weather_source.message=Some(("本机 Key 已清除。填写并测试新凭据后可重新启用。",false));
                        }}
                    }
                });
                self.weather_source.connection_status(ui);
                settings_hint(ui,"只启用 GeoAPI 和天气预报两类权限；Key 安全保存在 Windows 凭据管理器。");
                egui::CollapsingHeader::new("免费额度详情").id_salt("qweather-free-details").show(ui,|ui|{
                    ui.label("每自然月前 50,000 次免费");
                    settings_hint(ui,"同一和风开发者账号合计，GeoAPI 与天气预报共享，并非每个 Key 各享一份。超额继续按量计费，不会自动停止。");
                    settings_hint(ui,"岛仔限制本机每自然月最多 5,000 次请求，包含搜索、测试、刷新和失败尝试；不能限制同账号其他设备、应用或项目的用量。");
                    ui.hyperlink_to("官方计费说明","https://dev.qweather.com/docs/finance/pricing/");
                });
            }
            if self.weather_source.test_error.is_none(){if let Some((message,error))=self.weather_source.message {ui.add(egui::Label::new(RichText::new(message).size(11.5).color(if error{color(ui.ctx(),Color32::from_rgb(255,137,137),Color32::from_rgb(183,48,48))}else{ui.visuals().weak_text_color()})).wrap());}}
        });
    }

    fn qweather_city_picker(&mut self,ui:&mut egui::Ui){
        let popup=egui::Id::new("qweather-city-options");
        let input_id=egui::Id::new("qweather-city-input");
        let keys=if ui.is_enabled()&&ui.memory(|m|m.has_focus(input_id)||m.is_popup_open(popup)){
            ui.input_mut(|i|(i.consume_key(egui::Modifiers::NONE,egui::Key::ArrowDown),i.consume_key(egui::Modifiers::NONE,egui::Key::ArrowUp),i.consume_key(egui::Modifiers::NONE,egui::Key::Enter)))
        }else{(false,false,false)};
        let now=ui.ctx().input(|i|i.time);let mut selected=None;
        let before=self.weather_source.province.clone();
        let response=ui.horizontal(|ui|{
            egui::ComboBox::from_id_salt("qweather-province").width(108.0).height(230.0)
                .selected_text(if self.weather_source.province.is_empty(){"不限省份"}else{&self.weather_source.province}).show_ui(ui,|ui|{
                    ui.selectable_value(&mut self.weather_source.province,String::new(),"不限省份");
                    for province in ["北京","天津","河北","山西","内蒙古","辽宁","吉林","黑龙江","上海","江苏","浙江","安徽","福建","江西","山东","河南","湖北","湖南","广东","广西","海南","重庆","四川","贵州","云南","西藏","陕西","甘肃","青海","宁夏","新疆","香港","澳门","台湾"]{
                        ui.selectable_value(&mut self.weather_source.province,province.to_string(),province);
                    }
                });
            let input=ui.add_sized([(ui.available_width()-38.0).max(90.0),32.0],egui::TextEdit::singleline(&mut self.weather_source.query)
                .id(input_id).hint_text("输入城市、县或区名称").margin(egui::vec2(9.0,7.0)));
            let arrow=ui.add_sized([30.0,32.0],egui::Button::new("").corner_radius(7.0)).on_hover_text("展开城市候选");
            let center=arrow.rect.center();ui.painter().add(egui::Shape::line(vec![center+egui::vec2(-4.0,-2.0),center+egui::vec2(0.0,2.0),center+egui::vec2(4.0,-2.0)],egui::Stroke::new(1.5,ui.visuals().text_color())));
            if ui.is_enabled(){
                if self.weather_source.focus_city {input.request_focus();self.weather_source.focus_city=false;}
                if input.changed()||input.clicked()||before!=self.weather_source.province {ui.memory_mut(|m|m.open_popup(popup));}
                if arrow.clicked(){ui.memory_mut(|m|m.toggle_popup(popup));}
                if input.has_focus()||ui.memory(|m|m.is_popup_open(popup)){
                    if keys.0 {
                        if ui.memory(|m|m.is_popup_open(popup)){self.weather_source.highlighted=(self.weather_source.highlighted+1).min(self.weather_source.results.len().saturating_sub(1));}
                        ui.memory_mut(|m|m.open_popup(popup));
                    }
                    if keys.1{self.weather_source.highlighted=self.weather_source.highlighted.saturating_sub(1);}
                }
            }
            input.union(arrow)
        }).inner;
        if ui.is_enabled(){
            if self.weather_source.search_due(now){self.weather_source.search();}
            let term_ready=!self.weather_source.query.trim().is_empty()&&(!self.weather_source.query.is_ascii()||self.weather_source.query.trim().len()>=2);
            if term_ready&&(!self.weather_source.search_sent||self.weather_source.search.is_some()){ui.ctx().request_repaint_after(std::time::Duration::from_millis(60));}
            if keys.2 {if let Some(location)=self.weather_source.results.get(self.weather_source.highlighted){selected=Some(location.clone());}else{self.weather_source.search();ui.memory_mut(|m|m.open_popup(popup));}}
        }
        egui::popup::popup_below_widget(ui,popup,&response,egui::popup::PopupCloseBehavior::CloseOnClickOutside,|ui|{
            ui.spacing_mut().item_spacing.y=2.0;
            if self.weather_source.query.trim().is_empty(){settings_hint(ui,"输入名称或拼音，自动匹配城市、县和区。");}
            else if self.weather_source.query.is_ascii()&&self.weather_source.query.trim().len()<2 {settings_hint(ui,"继续输入至少两个字母。");}
            else if let Some(error)=self.weather_source.search_error {
                settings_hint(ui,error.text());if ui.button("重新搜索").clicked(){self.weather_source.search();}
            }else if self.weather_source.search.is_some()||!self.weather_source.search_sent {ui.horizontal(|ui|{ui.spinner();ui.label("正在匹配城市…");});}
            else if self.weather_source.results.is_empty(){settings_hint(ui,"没有匹配的城市，试试简称或选择省份；乡镇可改搜所属县区。");}
            else {
                let results=self.weather_source.results.clone();
                egui::ScrollArea::vertical().id_salt("qweather-city-results").max_height(220.0).show(ui,|ui|{
                    for (index,location) in results.iter().enumerate(){
                        let chosen=self.weather_source.city_choice.as_ref()==Some(location);
                        let row=crate::weather_ui::city_option(ui,&location.position,chosen,self.weather_source.config.saved.contains(location));
                        if index==self.weather_source.highlighted {ui.painter().rect_stroke(row.rect.shrink(1.0),7.0,egui::Stroke::new(1.0,ui.visuals().selection.stroke.color),egui::StrokeKind::Inside);if keys.0||keys.1{row.scroll_to_me(None);}}
                        if row.clicked(){selected=Some(location.clone());}
                    }
                });
            }
            for credit in &self.weather_source.geo_credits {settings_hint(ui,credit);}
        });
        if let Some(location)=selected {self.weather_source.choose_city(location);ui.memory_mut(|m|m.close_popup());}
    }

    pub(crate) fn qweather_city_settings(&mut self,ui:&mut egui::Ui){
        let active=self.weather_source.displayed==Provider::Qweather;
        let configured=self.weather_source.configured();
        let accent=color(ui.ctx(),Color32::from_rgb(96,164,255),Color32::from_rgb(34,108,220));
        settings_label(ui,"和风城市");
        settings_card(ui,|ui|{
            ui.horizontal(|ui|{ui.label(RichText::new("已保存城市").strong());ui.label(RichText::new(format!("{} / 5",self.weather_source.config.saved.len())).color(ui.visuals().weak_text_color()));});
            let saved=self.weather_source.config.saved.clone();let(mut chosen,mut removed)=(None,None);
            for location in saved {
                let cached=if active{self.weather.snapshot.as_ref().filter(|s|s.position.as_ref()==Some(&location.position))}else{None};
                let (pick,remove)=crate::weather_ui::city_row(ui,self.weather_source.picked.as_ref()==Some(&location),true,&location.position.label,&location.position.region,cached,accent);
                if pick {chosen=Some(location.clone());}if remove {removed=Some(location);}
            }
            if let Some(location)=removed{self.qweather_remove_city(&location);}
            else if let Some(location)=chosen {
                self.weather_source.picked=Some(location.clone());self.weather_source.query.clear();self.weather_source.reset_search();
                if active&&self.weather_source.tested.is_none(){self.qweather_select_location(location);}
            }
            if self.weather_source.config.saved.is_empty(){settings_hint(ui,"还没有保存城市，搜索后点「添加城市」，最多保存 5 个。");}
            else {settings_hint(ui,if active{"点击城市即可切换；只自动更新当前城市。"}else{"选择一个已保存城市，再点「保存并启用」。"});}
            ui.add_space(7.0);ui.separator();ui.add_space(6.0);
            settings_hint(ui,if configured{"和风暂不支持自动定位，请手动选择城市。"}else{"自动定位不可用：先填写 Host / Key 并通过连接测试，再选择城市。"});
            let full=self.weather_source.config.saved.len()>=crate::weather::MAX_CITIES;
            ui.add_enabled_ui(configured,|ui|{self.qweather_city_picker(ui);});
            ui.add_space(5.0);
            let choice=self.weather_source.city_choice.clone();
            let exists=choice.as_ref().is_some_and(|l|self.weather_source.config.saved.contains(l));
            ui.horizontal_wrapped(|ui|{
                if ui.add_enabled(configured&&choice.is_some()&&!exists&&!full,egui::Button::new("添加城市")).clicked(){self.qweather_add_city();}
                settings_hint(ui,if exists{"这个城市已保存"}else if full{"已保存 5 个城市，移除一个后可继续添加。"}else if choice.is_none(){"输入关键词后，从下拉列表选择具体地区。"}else{"添加后可随时在上方列表切换。"});
            });
            let target=self.weather_source.city_choice.as_ref().or(self.weather_source.picked.as_ref());
            let ready=configured&&target.is_some_and(|l|!full||self.weather_source.config.saved.contains(l));
            if let Some(location)=target {settings_hint(ui,&format!("已选择：{} · {}",location.position.label,location.position.region));}
            ui.add_space(7.0);
            if ui.add_enabled(ready,egui::Button::new(if active&&self.weather_source.tested.is_none(){"应用城市"}else{"保存并启用"})).clicked(){
                if active&&self.weather_source.tested.is_none(){if let Some(location)=self.weather_source.city_choice.as_ref().or(self.weather_source.picked.as_ref()).cloned(){
                    match self.weather_source.remember_city(location.clone()){Ok(_)=>{self.qweather_select_location(location);self.weather_source.query.clear();self.weather_source.reset_search();},Err(error)=>self.weather_source.message=Some((error,true))}
                }}else{self.apply_weather_source();}
            }
        });
        if active {
            settings_card(ui,|ui|{
                ui.hyperlink_to("天气数据来自 QWeather","https://www.qweather.com/");
                settings_hint(ui,&format!("本机本月已请求 {} 次 · 仅更新当前城市",qweather::usage()));
                settings_hint(ui,"和风账号每月免费额度 50,000 次，多设备与所有 Key 共享；超额可能收费。");
                let(a,b)=self.weather_source.runtime.stamps();
                settings_hint(ui,&format!("实况更新：{} · 预报更新：{}",updated_at(a),updated_at(b)));
                settings_hint(ui,"实况约 60 分钟、预报约 6 小时刷新。降水概率为日夜较高值。");
                if let Some(e)=self.weather_source.runtime.error {settings_hint(ui,e.text());}
                if let Some(s)=&self.weather.snapshot {for attribution in &s.attributions {ui.label(attribution);}}
            });
        }
    }

    pub(crate) fn weather_tutorial_ui(&mut self,ctx:&egui::Context){
        if !self.weather_source.tutorial_open{self.weather_source.tutorial_textures.clear();return;}
        let id=egui::ViewportId::from_hash_of("daozai-weather-tutorial");
        ctx.show_viewport_immediate(id,egui::ViewportBuilder::default().with_title("和风天气配置教程 · 岛仔")
            .with_icon(crate::theme::app_icon()).with_inner_size([820.0,680.0]).with_min_inner_size([520.0,420.0]),|ctx,_|{
            if ctx.input(|i|i.viewport().close_requested()){self.weather_source.tutorial_open=false;}
            let original=ctx.style();let mut style=(*original).clone();
            style.visuals=if self.settings.settings_theme.dark(crate::settings_theme::system_dark(ctx)){egui::Visuals::dark()}else{egui::Visuals::light()};
            ctx.set_style(style);
            egui::CentralPanel::default().frame(egui::Frame::new().fill(ctx.style().visuals.panel_fill).inner_margin(16)).show(ctx,|ui|{
                ui.horizontal_wrapped(|ui|{
                    ui.label(RichText::new("配置教程 · 步骤 01–10").strong());
                    egui::ComboBox::from_id_salt("weather-tutorial-zoom").width(100.0)
                        .selected_text(if self.weather_source.tutorial_zoom==0.0{"适应宽度".into()}else{format!("{:.0}%",self.weather_source.tutorial_zoom*100.0)})
                        .show_ui(ui,|ui|{for (zoom,title) in [(0.0,"适应宽度"),(1.0,"100%"),(1.5,"150%"),(2.0,"200%")]{ui.selectable_value(&mut self.weather_source.tutorial_zoom,zoom,title);}});
                });
                ui.add_space(8.0);
                settings_hint(ui,"仅需 GeoAPI + 天气预报两类权限，已涵盖搜索、实况、七日预报和日出日落。无需控制台 API。");
                settings_hint(ui,"教程可离线连续滚动查看，支持适应宽度与放大。");
                ui.add_space(8.0);
                if self.weather_source.tutorial_textures.is_empty(){
                    for (index,bytes) in [include_bytes!("../assets/weather/qweather-01-project.png").as_slice(),include_bytes!("../assets/weather/qweather-02-api-key.png").as_slice(),include_bytes!("../assets/weather/qweather-03-daozai.png").as_slice()].into_iter().enumerate(){
                        let image=image::load_from_memory(bytes).expect("内置天气教程有效").to_rgba8();
                        if index==2{self.weather_source.tutorial_step10=self.weather_source.tutorial_textures.len();}
                        // 第 10 步使用下方的岛仔界面及最终操作说明。
                        let height=if index==1{1880.min(image.height())}else{image.height()};
                        // 长图按段上传，保留原始文字清晰度，避免超过纹理高度限制。
                        for y in (0..height).step_by(1024){
                            let tile=image::imageops::crop_imm(&image,0,y,image.width(),1024.min(height-y)).to_image();
                            let pixels=egui::ColorImage::from_rgba_unmultiplied([tile.width()as usize,tile.height()as usize],tile.as_raw());
                            self.weather_source.tutorial_textures.push(ctx.load_texture(format!("weather-tutorial-{index}-{y}"),pixels,egui::TextureOptions::LINEAR));
                        }
                    }
                }
                let fit=ui.available_width()-12.0;
                let width=if self.weather_source.tutorial_zoom==0.0{fit}else{1040.0*self.weather_source.tutorial_zoom};
                egui::ScrollArea::both().id_salt("weather-tutorial").auto_shrink([false,false]).show(ui,|ui|{
                    ui.spacing_mut().item_spacing.y=0.0;
                    for (index,texture) in self.weather_source.tutorial_textures.iter().enumerate(){
                        if index==self.weather_source.tutorial_step10{
                            ui.add_space(14.0);ui.label(RichText::new("10  回岛仔配置并启用").size(19.0).strong());ui.add_space(8.0);
                            ui.label("填写 API Host 和 API Key，点「测试连接」。通过后搜索并选择城市，点「保存并启用」。");
                            ui.label("当前生效变为「和风天气」后，切换完成。配置可收起；城市选择与应用按钮保持可用。");
                            ui.add_space(10.0);
                        }
                        let height=width*texture.size()[1]as f32/texture.size()[0]as f32;
                        ui.add(egui::Image::new((texture.id(),texture.size_vec2())).fit_to_exact_size(egui::vec2(width,height)));
                    }
                });
            });
            ctx.set_style(original);
        });
    }
}

#[cfg(test)]
impl State {
    pub(crate) fn mock_verified(&mut self){
        self.host="example.re.qweatherapi.com".into();
        self.tested=Some(Client::new(&self.host,"fake-test-key",Arc::new(AtomicBool::new(true))).unwrap());
        self.picked=Some(Location{position:crate::weather::Position{lat:30.42,lon:119.99,label:"余杭".into(),region:"浙江省 · 杭州市".into()},timezone:"Asia/Shanghai".into()});
    }
    pub(crate) fn mock_choose_city(&mut self,location:Location){self.choose_city(location);}
    pub(crate) fn mock_search_results(&mut self,locations:Vec<Location>){
        let(tx,rx)=std::sync::mpsc::channel();self.search=Some(rx);self.search_input=(self.query.trim().into(),self.province.trim().into());self.search_sent=true;
        tx.send(Ok(qweather::Search{locations,credits:vec![]})).unwrap();self.poll(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn qweather_city_autocomplete_debounces_deduplicates_and_discards_old_results(){
        let mut state=State::default();state.mock_verified();state.query="杭".into();
        assert!(!state.search_due(1.0));assert!(!state.search_due(1.44));assert!(state.search_due(1.46));
        state.search();assert!(!state.search_due(5.0),"同一个关键词只能自动请求一次");
        let(tx,rx)=std::sync::mpsc::channel();state.search=Some(rx);
        tx.send(Ok(qweather::Search{locations:vec![state.picked.clone().unwrap()],credits:vec![]})).unwrap();
        state.query="宁".into();assert!(!state.search_due(6.0));state.poll(true);assert!(state.results.is_empty(),"旧输入的候选不能覆盖新输入");
        assert!(state.search_due(6.5));state.search();state.province="浙江".into();assert!(!state.search_due(7.0));assert!(state.search_due(7.5));
        state.poll(false);assert!(!state.search_due(100.0),"关闭模块后不能自动搜索");
    }
}
