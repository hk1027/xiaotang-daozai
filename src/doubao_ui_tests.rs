use super::*;

fn fixture(h: &mut H) {
    use crate::quota::{Account, Provider};
    let account = Account { id:"doubao-preview".into(),provider:Provider::Doubao,alias:"豆包".into(),
        source:Default::default(),slot:Some("0".repeat(64)),enabled:true,pinned:true };
    h.app.quota.config.accounts=vec![account.clone()];h.app.quota.config.selected=account.id.clone();
    let snapshot=crate::quota::parse(Provider::Doubao,&serde_json::json!({"code":0,"data":{
        "current_subscription":{"display":{"short_name":"豆包 Pro"}},
        "window_limit_section":{"window_limit_groups":[{"window_limits":[
            {"window_type":1,"used_percent":28,"end_time":1900008000000i64},
            {"window_type":2,"used_percent":56,"end_time":1900604800000i64}
        ]}]},
        "incentive_quota_section":{"window_limit_groups":[{"window_limits":[{"window_type":3,"used_percent":25,"end_time":1900008000000i64}]}]}
    }})).unwrap();
    h.app.quota.rows.insert(account.id,Ok(snapshot));
}

fn settings_frame(h: &mut H, events: Vec<egui::Event>) {
    h.t+=1.0/60.0;
    let output=h.ctx.run(egui::RawInput{time:Some(h.t),screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::new(780.0,900.0))),events,..Default::default()},
        |ctx|{egui::CentralPanel::default().show(ctx,|ui|h.app.quota_settings(ui));});
    h.shapes=output.shapes;
}

fn has_text(h: &H, expected: &str) -> bool {
    h.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(text) if text.galley.text().contains(expected)))
}

#[test]
fn doubao_activity_is_visible_on_the_island_and_in_account_details() {
    let _guard=ENV_LOCK.lock().unwrap_or_else(|error|error.into_inner());let mut h=H::new();fixture(&mut h);
    h.app.show_page(6);h.hover(H::local(260.0,60.0),1.0);
    for label in ["5 小时","本周","活动额度","75%"] { assert!(has_text(&h,label),"额度面板缺少 {label}"); }
    for _ in 0..5 { settings_frame(&mut h,vec![]); }
    assert!(has_text(&h,"活动额度：剩余 75.0%"),"账号明细应保留活动额度");
    assert!(has_text(&h,"5 小时：剩余 72.0%") && has_text(&h,"本周：剩余 44.0%"));
}

#[test]
fn doubao_closing_either_window_cancels_validation_and_clears_the_cookie() {
    let _guard=ENV_LOCK.lock().unwrap_or_else(|error|error.into_inner());
    for (close_settings,ready) in [(false,false),(false,true),(true,false),(true,true)] {
        let mut h=H::new();h.app.open_settings();h.app.quota.connect_open=true;h.app.quota.provider=crate::quota::Provider::Doubao;
        h.app.quota.doubao_cookie="synthetic-secret".into();
        let (connection,cancel)=if ready{crate::quota::doubao::Connect::ready_test()}else{crate::quota::doubao::Connect::pending_test()};h.app.quota.doubao_connecting=Some(connection);
        if close_settings { h.app.settings_open=false;h.app.quota_tick(); } else { h.app.quota_close_connection(); }
        assert!(cancel.load(std::sync::atomic::Ordering::Acquire));
        assert!(h.app.quota.doubao_connecting.is_none() && h.app.quota.doubao_cookie.is_empty());
        assert!(h.app.quota.config.accounts.is_empty(),"关闭窗口不能新增账号");
    }
}

#[test]
fn doubao_connection_offers_official_login_without_an_extension() {
    let _guard=ENV_LOCK.lock().unwrap_or_else(|error|error.into_inner());let mut h=H::new();
    h.app.open_settings();h.app.quota.provider=crate::quota::Provider::Doubao;h.app.quota.connect_open=true;
    for _ in 0..5 { settings_frame(&mut h,vec![]); }
    for label in ["添加账号","浏览器登录","从本机导入","使用 Cookie 连接"] { assert!(has_text(&h,label),"连接窗口缺少 {label}"); }
    assert!(!has_text(&h,"Kimi"),"豆包登录不能依赖浏览器扩展");
    assert!(!has_text(&h,"登录 JSON"),"豆包不应走登录文件导入");
}

#[test]
fn doubao_settings_viewport_close_cancels_pending_validation() {
    let _guard=ENV_LOCK.lock().unwrap_or_else(|error|error.into_inner());let mut h=H::new();
    h.app.settings_tab=11;h.app.open_settings();h.ctx.set_embed_viewports(false);
    h.app.quota.provider=crate::quota::Provider::Doubao;h.app.quota.connect_open=true;h.app.quota.doubao_cookie="synthetic-secret".into();
    let (connection,cancel)=crate::quota::doubao::Connect::pending_test();h.app.quota.doubao_connecting=Some(connection);
    egui::Context::set_immediate_viewport_renderer(move|ctx,mut viewport|{
        let id=viewport.ids.this;let _=ctx.run(egui::RawInput{viewport_id:id,screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(690.0,700.0))),
            viewports:std::iter::once((id,egui::ViewportInfo{events:vec![egui::ViewportEvent::Close],..Default::default()})).collect(),..Default::default()},
            |ctx|(viewport.viewport_ui_cb)(ctx));
    });
    let _=h.ctx.run(Default::default(),|ctx|h.app.settings_ui(ctx,false));
    assert!(cancel.load(std::sync::atomic::Ordering::Acquire));
    assert!(!h.app.settings_open && !h.app.quota.connect_open && h.app.quota.doubao_cookie.is_empty());
}

#[test]
fn doubao_quota_gallery() {
    let Some(dest)=std::env::var_os("DAOZAI_DOUBAO_PREVIEW_DIR").map(std::path::PathBuf::from) else{return;};
    let _guard=ENV_LOCK.lock().unwrap_or_else(|error|error.into_inner());std::fs::create_dir_all(&dest).unwrap();
    for light in [false,true] {
        let name=if light{"light"}else{"dark"};
        let mut h=H::new();fixture(&mut h);h.app.settings.pet=false;
        h.app.settings.skin=if light{crate::model::Skin::GlassLight}else{crate::model::Skin::Classic};
        h.app.show_page(6);let mut raster=feature_raster();
        shoot_feature(&mut h,&mut raster,&dest,&format!("doubao-island-{name}"),60,Some(H::local(260.0,60.0)),"2026-11-11");
        let mut resident=H::new();fixture(&mut resident);resident.app.settings.pet=false;resident.app.settings.show_battery=false;
        resident.app.settings.skin=if light{crate::model::Skin::GlassLight}else{crate::model::Skin::Classic};
        let path=dest.join(format!("doubao-resident-{name}.png"));
        shoot_feature(&mut resident,&mut raster,&dest,&format!("doubao-resident-{name}"),90,None,"2026-11-11");
        let image=image::open(&path).unwrap().to_rgba8();let width=((resident.app.current_width()*2.0).ceil()as u32+32).min(image.width());
        image::imageops::crop_imm(&image,(image.width()-width)/2,0,width,96).to_image().save(path).unwrap();
        for connect in [false,true] {
            let mut h=H::new();if !connect{fixture(&mut h);}
            h.app.settings.settings_theme=if light{crate::settings_theme::SettingsTheme::Light}else{crate::settings_theme::SettingsTheme::Dark};
            h.app.settings_tab=11;h.app.open_settings();h.ctx.set_embed_viewports(false);
            h.app.quota.provider=crate::quota::Provider::Doubao;h.app.quota.connect_open=connect;
            let size=Vec2::new(780.0,850.0);let ppp=1.25;
            let output=std::sync::Arc::new(Mutex::new(None));let capture=output.clone();
            egui::Context::set_immediate_viewport_renderer(move|ctx,mut viewport|{
                let id=viewport.ids.this;*capture.lock().unwrap()=Some(ctx.run(egui::RawInput{viewport_id:id,
                    screen_rect:Some(Rect::from_min_size(Pos2::ZERO,size)),viewports:std::iter::once((id,egui::ViewportInfo{
                        native_pixels_per_point:Some(ppp),focused:Some(true),..Default::default()})).collect(),..Default::default()},
                    |ctx|(viewport.viewport_ui_cb)(ctx)));
            });
            let mut raster=crate::preview_raster::Raster::new((size.x*ppp)as usize,(size.y*ppp)as usize);
            for frame in 0..5 {
                let root=h.ctx.run(Default::default(),|ctx|h.app.settings_ui(ctx,false));raster.update_textures(&root.textures_delta);
                let child=output.lock().unwrap().take().unwrap();raster.update_textures(&child.textures_delta);
                if frame==4 {
                    raster.buf.fill(if light{[0.95,0.95,0.96,1.0]}else{[0.1,0.1,0.11,1.0]});
                    raster.draw(&h.ctx.tessellate(child.shapes,child.pixels_per_point),child.pixels_per_point);
                    let mut rgba=Vec::new();raster.to_rgba8(&mut rgba);
                    image::save_buffer(dest.join(format!("doubao-{}-{name}.png",if connect{"connect"}else{"settings"})),
                        &rgba,(size.x*ppp)as u32,(size.y*ppp)as u32,image::ColorType::Rgba8).unwrap();
                }
                raster.free_textures(&root.textures_delta);raster.free_textures(&child.textures_delta);
            }
        }
    }
}
