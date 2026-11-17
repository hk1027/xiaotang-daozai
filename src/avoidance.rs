//! 自动避让设置与正在运行的应用选择。
use eframe::egui::{self,Color32,Rect,Vec2};
use crate::{extras::{settings_card,settings_label,setting_row},model::Settings};
use std::sync::mpsc::{self,Receiver};

#[derive(Clone,Debug)]
pub(crate) struct Candidate {pub path:String,pub name:String,pub title:String,pub browser:bool}

pub(crate) fn same_app(a:&str,b:&str)->bool {a.replace('/',"\\").to_lowercase()==b.replace('/',"\\").to_lowercase()}
pub(crate) fn contains_app(apps:&[(String,String)],path:&str)->bool {apps.iter().any(|(p,_)|same_app(p,path))}

#[derive(Default)]
pub(crate) struct Picker {
    pub(crate) open:bool,
    pub(crate) entries:Vec<Candidate>,
    query:String,
    selected:Option<String>,
    rx:Option<Receiver<Result<Vec<Candidate>,String>>>,
    error:String,
    icons:crate::icons::Cache,
}
impl Picker {
    fn refresh(&mut self,ctx:&egui::Context) {
        if self.rx.is_some(){return;}
        let (tx,rx)=mpsc::channel();self.rx=Some(rx);self.error.clear();self.selected=None;
        let ctx=ctx.clone();
        std::thread::spawn(move||{
            #[cfg(windows)] let result=crate::system::avoidance_windows();
            #[cfg(not(windows))] let result=Err("应用选择仅支持 Windows".into());
            let _=tx.send(result);ctx.request_repaint();
        });
    }
    fn icon(&mut self,ui:&egui::Ui,rect:Rect,path:&str,name:&str) {
        if let Some(texture)=self.icons.get(ui.ctx(),std::path::Path::new(path)).and_then(|p|p.image) {
            ui.painter().image(texture.id(),rect,Rect::from_min_max(egui::Pos2::ZERO,egui::pos2(1.0,1.0)),Color32::WHITE);
        }else{
            ui.painter().rect_filled(rect,7.0,ui.visuals().widgets.inactive.bg_fill);
            ui.painter().text(rect.center(),egui::Align2::CENTER_CENTER,name.chars().next().unwrap_or('▦'),crate::theme::font(17.0,crate::theme::W::Semibold),ui.visuals().text_color());
        }
    }
    pub(crate) fn settings(&mut self,ui:&mut egui::Ui,settings:&mut Settings) {
        settings_label(ui,"自动避让");
        settings_card(ui,|ui|{
            setting_row(ui,"自动避让","指定应用在前台并遮住岛的位置时，收成细线",&mut settings.browser_minimal);
            ui.add_enabled_ui(settings.browser_minimal,|ui|{
                ui.separator();
                setting_row(ui,"常用浏览器","自动识别 Chrome、Edge、夸克等浏览器",&mut settings.avoid_browsers);
                ui.separator();
                crate::extras::settings_control_row(ui,"自定义应用",100.0,|ui|{
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{
                        if ui.button("＋ 添加应用").clicked(){self.open=true;self.query.clear();self.refresh(ui.ctx());}
                    }).response
                });
                ui.add_space(6.0);
                let mut remove=None;
                let icon_width=ui.available_width();
                ui.scope(|ui|{
                ui.style_mut().always_scroll_the_only_direction=true;
                egui::ScrollArea::horizontal().id_salt("avoidance-icon-grid").max_height(52.0).min_scrolled_height(0.0).auto_shrink([false,true]).show(ui,|ui|{
                ui.horizontal(|ui|{
                    ui.spacing_mut().item_spacing=Vec2::splat(8.0);
                    for (index,(path,name)) in settings.avoid_apps.iter().enumerate() {
                        let (rect,response)=ui.allocate_exact_size(egui::vec2(52.0,52.0),egui::Sense::hover());
                        ui.painter().rect_filled(rect,9.0,ui.visuals().faint_bg_color);
                        self.icon(ui,Rect::from_center_size(rect.center(),Vec2::splat(32.0)),path,name);
                        response.on_hover_text(format!("{name}\n所有窗口\n{path}"));
                        let center=rect.right_top()+egui::vec2(-8.0,8.0);
                        let remove_button=ui.interact(Rect::from_center_size(center,Vec2::splat(20.0)),ui.id().with(("avoidance-remove",index)),egui::Sense::click());
                        let fill=if remove_button.hovered(){ui.visuals().widgets.hovered.bg_fill}else{ui.visuals().widgets.inactive.bg_fill};
                        ui.painter().circle_filled(center,7.0,fill);
                        ui.painter().text(center,egui::Align2::CENTER_CENTER,"×",crate::theme::font(9.0,crate::theme::W::Regular),ui.visuals().text_color());
                        if remove_button.on_hover_text(format!("移除 {name}")).clicked(){remove=Some(index);}
                    }
                    if settings.avoid_apps.is_empty(){ui.add_sized([icon_width,44.0],egui::Label::new(egui::RichText::new("添加需要自动避让的其他应用").size(11.5).weak()));}
                });
                });
                });
                if let Some(index)=remove{settings.avoid_apps.remove(index);}
                ui.add_space(5.0);
                ui.label(egui::RichText::new("无需全屏或最大化；窗口移开或切换应用后恢复。").size(11.0).weak());
            });
        });
    }
    fn reason(app:&Candidate,settings:&Settings)->Option<&'static str> {
        if settings.avoid_browsers && app.browser {Some("浏览器规则已覆盖")}
        else if contains_app(&settings.avoid_apps,&app.path){Some("已添加")}else{None}
    }
    pub(crate) fn dialog(&mut self,ctx:&egui::Context,settings:&mut Settings) {
        if !self.open{return;}
        if let Some(rx)=&self.rx {
            match rx.try_recv() {
                Ok(result)=>{self.rx=None;match result{Ok(entries)=>self.entries=entries,Err(error)=>self.error=error}},
                Err(mpsc::TryRecvError::Disconnected)=>{self.rx=None;self.error="无法读取应用列表，请刷新重试".into();},
                _=>{},
            }
        }
        let mut close=false;
        let response=egui::Modal::new(egui::Id::new("avoidance-picker"))
            .frame(egui::Frame::popup(&ctx.style()).inner_margin(20).corner_radius(12.0)).show(ctx,|ui|{
            ui.visuals_mut().override_text_color=Some(crate::settings_theme::color(ctx,Color32::from_gray(235),Color32::from_gray(30)));
            ui.set_width(410.0);
            ui.heading("添加应用");
            ui.label(egui::RichText::new("选择正在运行的应用，添加后自动记住。").size(12.0).weak());
            ui.add_space(12.0);
            ui.add(egui::TextEdit::singleline(&mut self.query).hint_text("搜索应用或窗口标题").desired_width(f32::INFINITY));
            ui.add_space(8.0);
            if !self.error.is_empty(){ui.colored_label(ui.visuals().error_fg_color,&self.error);}
            if self.rx.is_some(){ui.horizontal(|ui|{ui.spinner();ui.label("正在读取应用…");});}
            let query=self.query.trim().to_lowercase();
            let entries:Vec<_>=self.entries.iter().filter(|a|a.name.to_lowercase().contains(&query)||a.title.to_lowercase().contains(&query)).cloned().collect();
            egui::ScrollArea::vertical().id_salt("avoidance-apps").max_height((ctx.screen_rect().height()-250.0).clamp(100.0,285.0)).show(ui,|ui|{
                if entries.is_empty() && self.rx.is_none(){ui.label("未找到匹配的应用，请先打开应用后刷新。");}
                for app in &entries {
                    let reason=Self::reason(app,settings);
                    let (rect,response)=ui.allocate_exact_size(egui::vec2(ui.available_width(),58.0),egui::Sense::click());
                    if self.selected.as_ref().is_some_and(|p|same_app(p,&app.path)) {
                        ui.painter().rect_filled(rect,7.0,ui.visuals().selection.bg_fill);
                    }else if reason.is_none() && response.hovered(){ui.painter().rect_filled(rect,7.0,ui.visuals().widgets.hovered.bg_fill);}
                    self.icon(ui,Rect::from_min_size(rect.min+egui::vec2(8.0,13.0),Vec2::splat(32.0)),&app.path,&app.name);
                    let c=crate::paint::Card::new(ui,rect.min,rect);
                    let ink=if reason.is_some(){ui.visuals().weak_text_color()}else{ui.visuals().text_color()};
                    let width=(rect.width()-56.0-if reason.is_some(){112.0}else{0.0}).max(40.0);
                    c.centered_text(50.0,19.0,&c.fit(&app.name,13.0,crate::theme::W::Regular,width),13.0,crate::theme::W::Regular,ink,crate::paint::H::Left);
                    c.centered_text(50.0,39.0,&c.fit(&app.title,11.0,crate::theme::W::Regular,width),11.0,crate::theme::W::Regular,ui.visuals().weak_text_color(),crate::paint::H::Left);
                    if let Some(reason)=reason{c.centered_text(rect.width()-8.0,29.0,reason,10.5,crate::theme::W::Regular,ui.visuals().weak_text_color(),crate::paint::H::Right);}
                    if response.clicked() && reason.is_none(){self.selected=Some(app.path.clone());}
                    response.on_hover_text(format!("{}\n{}",app.title,app.path));
                }
            });
            ui.add_space(10.0);
            ui.horizontal(|ui|{
                ui.label(egui::RichText::new("没有找到？先打开应用，再").size(11.5).weak());
                if ui.add_enabled(self.rx.is_none(),egui::Button::new("刷新列表")).clicked(){self.refresh(ctx);}
            });
            ui.add_space(12.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{
                let selected=self.entries.iter().find(|a|self.selected.as_ref().is_some_and(|p|same_app(p,&a.path)) && Self::reason(a,settings).is_none());
                if ui.add_enabled(selected.is_some(),egui::Button::new("添加").min_size(egui::vec2(72.0,30.0))).clicked(){
                    let app=selected.unwrap();settings.avoid_apps.push((app.path.clone(),app.name.clone()));close=true;
                }
                if ui.add_sized([72.0,30.0],egui::Button::new("取消")).clicked(){close=true;}
            });
        });
        if close || response.should_close(){self.open=false;}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn custom_apps_match_full_path_and_ignore_case_and_slashes() {
        let apps=vec![(r"D:\Apps\Editor.exe".into(),"编辑器".into())];
        assert!(contains_app(&apps,"d:/apps/EDITOR.exe"));
        assert!(!contains_app(&apps,r"C:\Other\Editor.exe"));
    }
    #[test]
    fn picker_disables_duplicates_and_browser_rules_only_when_enabled() {
        let app=Candidate{path:r"D:\Quark\quark.exe".into(),name:"夸克".into(),title:"新标签页".into(),browser:true};
        let mut settings=Settings::default();
        assert_eq!(Picker::reason(&app,&settings),Some("浏览器规则已覆盖"));
        settings.avoid_browsers=false;assert_eq!(Picker::reason(&app,&settings),None);
        settings.avoid_apps.push((app.path.to_uppercase(),app.name.clone()));
        assert_eq!(Picker::reason(&app,&settings),Some("已添加"));
    }
}
