//! 自动检查正式版更新，下载与安装由用户确认。
use eframe::egui;
use std::{io::{Read, Write}, path::PathBuf, sync::{mpsc::{self, Receiver}, Arc, atomic::{AtomicBool, Ordering}}, time::{Duration, Instant}};

const REPOSITORY: &str = "https://github.com/Renly1994/xiaotang-daozai";
const API: &str = "https://api.github.com/repos/Renly1994/xiaotang-daozai/releases/latest";
const RELEASES: &str = "https://github.com/Renly1994/xiaotang-daozai/releases/latest";
const BACKUP_DOWNLOAD: &str = "https://pan.quark.cn/s/0a39b4e1f332";
const STARTUP_DELAY: Duration = Duration::from_secs(10);
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 3600);
const RETRY_INTERVAL: Duration = Duration::from_secs(3600);

#[derive(Clone, Debug)]
struct Release { version: String, notes: String, page: String, package: Option<Package> }
#[derive(Clone, Debug)]
struct Package { url: String, size: u64, hash: String }
enum Event { Checked(Result<Option<Release>, String>), Progress(f32), Ready(PathBuf), Installed, Failed(String) }

#[derive(Default)]
pub(crate) struct State {
    rx: Option<Receiver<Event>>,
    result: Option<Result<Option<Release>, String>>,
    cancel: Option<Arc<AtomicBool>>,
    progress: f32,
    message: String,
    dialog_open: bool,
    focus_requested: bool,
    logo: Option<egui::TextureHandle>,
    next_check: Option<Instant>,
    automatic_check: bool,
    notified_version: Option<String>,
    history_path: Option<PathBuf>,
}

impl Drop for State {
    fn drop(&mut self) { if let Some(cancel) = &self.cancel { cancel.store(true, Ordering::Relaxed); } }
}

impl State {
    pub(crate) fn load(path: PathBuf) -> Self {
        let mut state = State::default();
        if let Some((next, version)) = std::fs::read(&path).ok()
            .and_then(|bytes| serde_json::from_slice::<(i64, Option<String>)>(&bytes).ok()) {
            let seconds = next.saturating_sub(chrono::Utc::now().timestamp()).clamp(0, CHECK_INTERVAL.as_secs() as i64);
            state.next_check = Some(Instant::now() + STARTUP_DELAY.max(Duration::from_secs(seconds as u64)));
            state.notified_version = version;
        }
        state.history_path = Some(path);
        state
    }

    fn remember_check(&self, delay: Duration) {
        let Some(path) = &self.history_path else { return };
        let next = chrono::Utc::now().timestamp() + delay.as_secs() as i64;
        if let Some(parent) = path.parent() {
            if std::fs::create_dir_all(parent).is_err() { return; }
        }
        if let Ok(bytes) = serde_json::to_vec(&(next, &self.notified_version)) {
            let _ = crate::model::atomic_write(path, &bytes);
        }
    }

    fn should_check(&mut self, now: Instant) -> bool {
        now >= *self.next_check.get_or_insert(now + STARTUP_DELAY) && self.rx.is_none() && !self.dialog_open
    }

    fn begin_check(&mut self, ctx: &egui::Context, automatic: bool, load: impl FnOnce() -> Result<Option<Release>, String> + Send + 'static) -> bool {
        if self.rx.is_some() { return false; }
        let (tx, rx) = mpsc::channel(); self.rx = Some(rx); self.automatic_check = automatic;
        self.next_check = Some(Instant::now() + CHECK_INTERVAL);
        self.message.clear(); self.result = None; self.dialog_open = false; self.focus_requested = false;
        let ctx = ctx.clone();
        std::thread::spawn(move || { let _ = tx.send(Event::Checked(load())); ctx.request_repaint(); });
        true
    }

    pub(crate) fn poll(&mut self, ctx: &egui::Context) {
        loop {
            let event = match self.rx.as_ref().map(|rx| rx.try_recv()) {
                Some(Ok(event)) => event,
                Some(Err(mpsc::TryRecvError::Disconnected)) => {
                    self.rx = None; self.cancel = None;
                    self.automatic_check = false; self.next_check = Some(Instant::now() + RETRY_INTERVAL);
                    self.message = "检查或下载已中断，请重试。".into(); break;
                }
                _ => break,
            };
            match event {
                Event::Checked(result) => {
                    let automatic = std::mem::take(&mut self.automatic_check);
                    let delay = if result.is_err() { RETRY_INTERVAL } else { CHECK_INTERVAL };
                    self.next_check = Some(Instant::now() + delay);
                    self.dialog_open = matches!(&result, Ok(Some(release)) if !automatic || self.notified_version.as_deref() != Some(release.version.as_str()));
                    self.focus_requested = self.dialog_open;
                    if self.dialog_open { if let Ok(Some(release)) = &result { self.notified_version = Some(release.version.clone()); } }
                    self.remember_check(delay);
                    self.result = Some(result); self.rx = None;
                }
                Event::Progress(value) => self.progress = value,
                Event::Failed(message) => { self.message = message; self.rx = None; self.cancel = None; }
                Event::Ready(work) => {
                    if self.cancel.as_ref().is_some_and(|cancel| cancel.load(Ordering::Relaxed)) {
                        self.rx = None; self.cancel = None;
                        self.message = "下载已取消".into();
                        let _ = std::fs::remove_dir_all(work);
                        continue;
                    }
                    let (tx, rx) = mpsc::channel(); self.rx = Some(rx); self.cancel = None;
                    self.message = "正在准备重启…".into();
                    let ctx = ctx.clone();
                    std::thread::spawn(move || {
                        let event = match launch_installer(&work) { Ok(()) => Event::Installed, Err(error) => Event::Failed(error) };
                        let _ = tx.send(event); ctx.request_repaint();
                    });
                }
                Event::Installed => {
                    self.rx = None;
                    ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
                }
            }
        }
        // 复用主界面的低频轮询；只在实际检查时启动线程，不增加常驻计时线程。
        if !cfg!(test) && self.should_check(Instant::now()) { self.begin_check(ctx, true, check); }
        if self.rx.is_some() && !self.automatic_check { ctx.request_repaint_after(Duration::from_millis(100)); }
    }

    pub(crate) fn ui(&mut self, ui: &mut egui::Ui) {
        if ui.add_enabled(self.rx.is_none(), egui::Button::new("检查更新")).clicked() {
            self.begin_check(ui.ctx(), false, check);
        }
        match &self.result {
            Some(Ok(Some(release))) => {
                if ui.button(format!("发现新版本 {} · 查看更新", release.version)).clicked() {
                    self.dialog_open = true; self.focus_requested = true;
                }
            }
            Some(Ok(None)) => { ui.label("当前已是最新版本"); }
            Some(Err(message)) => { ui.label(message); ui.hyperlink_to("打开发布页", RELEASES); }
            None if self.rx.is_some() => { ui.label("正在检查…"); }
            _ => {}
        }
        ui.add_space(5.0);
    }

    fn dismiss(&mut self) {
        if let Some(cancel) = &self.cancel { cancel.store(true, Ordering::Relaxed); }
        self.dialog_open = false;
    }

    pub(crate) fn dialog_ui(&mut self, ctx: &egui::Context) {
        use crate::theme::{self, W};
        if !self.dialog_open { return; }
        let Some(Ok(Some(release))) = self.result.clone() else { return; };
        let focus = std::mem::take(&mut self.focus_requested);
        let logo = self.logo.get_or_insert_with(|| {
            let icon = theme::app_icon();
            ctx.load_texture("update-logo", egui::ColorImage::from_rgba_unmultiplied([icon.width as usize,icon.height as usize],&icon.rgba),egui::TextureOptions::LINEAR)
        }).id();
        let installing = self.rx.is_some() && self.cancel.is_none();
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("daozai-update"),
            egui::ViewportBuilder::default().with_title("岛仔更新").with_icon(theme::app_icon())
                .with_inner_size([520.0, 416.0]).with_resizable(false).with_decorations(false)
                .with_transparent(true).with_active(true), |ctx, _| {
                if focus {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                if ctx.input(|input| input.viewport().close_requested()) {
                    if installing { ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose); }
                    else { self.dismiss(); return; }
                }
                egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx, |ui| {
                    let window = ui.max_rect().shrink(8.0);
                    #[cfg(windows)]
                    clip_update_window(window, ctx.pixels_per_point());
                    ui.painter().rect_filled(window, 22.0, egui::Color32::from_rgb(25,25,28));
                    ui.painter().rect_stroke(window, 22.0, egui::Stroke::new(1.0, egui::Color32::from_rgb(55,55,61)), egui::StrokeKind::Inside);
                    let close_rect = egui::Rect::from_min_size(window.right_top()+egui::vec2(-42.0,12.0),egui::vec2(28.0,28.0));
                    let close = ui.interact(close_rect, ui.id().with("update-close"), egui::Sense::click()).on_hover_text("关闭");
                    if close.hovered() { ui.painter().rect_filled(close_rect, 8.0, theme::BTN_HOVER); }
                    let center = close_rect.center();
                    for direction in [-1.0,1.0] {
                        ui.painter().line_segment([center+egui::vec2(-4.0,-4.0*direction),center+egui::vec2(4.0,4.0*direction)],egui::Stroke::new(1.5,theme::GRAY));
                    }
                    if close.clicked() && !installing { self.dismiss(); }
                    let drag_rect = egui::Rect::from_min_max(window.min,egui::pos2(close_rect.left()-8.0,window.top()+42.0));
                    if ui.interact(drag_rect,ui.id().with("update-drag"),egui::Sense::drag()).drag_started() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }
                    ui.scope_builder(egui::UiBuilder::new().max_rect(window.shrink2(egui::vec2(24.0,24.0))), |ui| {
                        let style = ui.style_mut();
                        style.visuals.override_text_color = Some(theme::WHITE);
                        style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(10);
                        style.visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(10);
                        style.visuals.widgets.active.corner_radius = egui::CornerRadius::same(10);
                        style.spacing.item_spacing = egui::vec2(10.0,8.0);
                        style.text_styles.insert(egui::TextStyle::Body,theme::font(13.0,W::Regular));
                        style.text_styles.insert(egui::TextStyle::Button,theme::font(13.0,W::Semibold));
                        ui.horizontal(|ui| {
                            ui.image((logo,egui::vec2(52.0,52.0)));
                            ui.add_space(4.0);
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new("岛仔有新版本了").font(theme::font(21.0,W::Semibold)));
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(option_env!("DAOZAI_BUILD_LABEL").unwrap_or(env!("CARGO_PKG_VERSION"))).color(theme::DIM));
                                    ui.label(egui::RichText::new("→").color(theme::DIM));
                                    egui::Frame::new().fill(theme::alpha_blue(0.12)).corner_radius(6.0)
                                        .inner_margin(egui::Margin::symmetric(8,3)).show(ui, |ui| {
                                            ui.label(egui::RichText::new(&release.version).color(theme::BLUE).font(theme::font(12.0,W::Semibold)));
                                        });
                                });
                            });
                        });
                        ui.add_space(12.0);
                        egui::Frame::new().fill(egui::Color32::from_rgb(35,35,39)).corner_radius(12.0)
                            .inner_margin(16.0).show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                ui.label(egui::RichText::new("本次更新").color(theme::GRAY).font(theme::font(11.5,W::Semibold)));
                                egui::ScrollArea::vertical().id_salt("update-notes").max_height(96.0).min_scrolled_height(96.0)
                                    .auto_shrink([false,false]).show(ui, |ui| {
                                        if release.notes.trim().is_empty() { ui.label("新版本已准备好。"); }
                                        else { for line in release.notes.lines() {
                                            let line = line.trim();
                                            if line.is_empty() { ui.add_space(3.0); continue; }
                                            let heading = line.starts_with('#');
                                            let text = if heading { line.trim_start_matches('#').trim().to_owned() }
                                                else if let Some(item) = line.strip_prefix("- ").or_else(||line.strip_prefix("* ")) { format!("•  {item}") }
                                                else { line.to_owned() };
                                            ui.label(egui::RichText::new(text).font(theme::font(13.0,if heading{W::Semibold}else{W::Regular})).color(egui::Color32::from_gray(225)));
                                        } }
                                    });
                            });
                        ui.add_space(4.0);
                        ui.label(egui::RichText::new(if self.message.is_empty(){"更新后自动重启，设置和记录会保留。"}else{&self.message}).color(theme::DIM).size(11.5));
                        if self.cancel.is_some() {
                            ui.add(egui::ProgressBar::new(self.progress).fill(theme::BLUE).desired_height(8.0));
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(if self.progress==0.0{"正在连接下载服务器…".into()}else if self.progress<1.0{format!("正在下载更新 · {:.0}%",self.progress*100.0)}else{"正在校验安装包…".into()}).color(theme::GRAY));
                                if ui.button("取消下载").clicked() { self.cancel.as_ref().unwrap().store(true,Ordering::Relaxed); }
                            });
                        } else {
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                if ui.add_enabled(!installing,egui::Button::new("稍后").fill(theme::BTN).min_size(egui::vec2(110.0,40.0))).clicked() { self.dismiss(); }
                                let install = egui::Button::new(egui::RichText::new(if installing{"正在准备重启…"}else{"下载并安装"}).color(theme::WHITE))
                                    .fill(theme::BLUE).min_size(egui::vec2(ui.available_width(),40.0));
                                if ui.add_enabled(release.package.is_some() && self.rx.is_none(),install).clicked() {
                                    let (tx,rx)=mpsc::channel(); self.rx=Some(rx);
                                    let cancel=Arc::new(AtomicBool::new(false)); self.cancel=Some(cancel.clone());
                                    self.progress=0.0; self.message.clear();
                                    let ctx=ui.ctx().clone(); let release=release.clone();
                                    std::thread::spawn(move || {
                                        let event=match download(&release,&tx,&cancel){Ok(work)=>Event::Ready(work),Err(error)=>Event::Failed(error)};
                                        let _=tx.send(event); ctx.request_repaint();
                                    });
                                }
                            });
                        }
                        ui.add_space(2.0);
                        ui.horizontal(|ui| {
                            ui.hyperlink_to(egui::RichText::new("网盘下载").size(11.5).color(theme::BLUE),BACKUP_DOWNLOAD);
                            ui.label(egui::RichText::new("·").color(theme::DIM));
                            ui.hyperlink_to(egui::RichText::new("浏览器下载").size(11.5).color(theme::GRAY),&release.page);
                            ui.label(egui::RichText::new("·").color(theme::DIM));
                            ui.hyperlink_to(egui::RichText::new("查看发布说明").size(11.5).color(theme::GRAY),&release.page);
                        });
                        if release.package.is_none() { ui.label(egui::RichText::new("此版本请通过浏览器下载。").size(11.5).color(theme::DIM)); }
                    });
                    if self.rx.is_some() { ctx.request_repaint_after(Duration::from_millis(100)); }
                });
            });
    }

}

/// 只统计岛仔自己的文件（KB）。共享安装文件夹里的其他软件不算在内。
/// 诊断脚本已内嵌在程序里（`--collect-diagnostics`），安装目录不再附带单独的诊断文件。
fn own_files_kb(dir: &std::path::Path) -> u32 {
    let bytes: u64 = ["daozai.exe", "Uninstall.exe", "LICENSE", "THIRD_PARTY_NOTICES.txt"].iter()
        .filter_map(|name| std::fs::metadata(dir.join(name)).ok()).filter(|meta| meta.is_file()).map(|meta| meta.len()).sum();
    bytes.div_ceil(1024).min(u64::from(u32::MAX)) as u32
}

/// 启动时为安装版登记真实大小：旧版安装器没有写 `EstimatedSize`，
/// 软件管家会统计整个安装文件夹；装进共享文件夹时会显示几十 GB。只在登记的安装位置就是本程序所在目录时写入。
#[cfg(windows)]
pub fn record_install_size() {
    use windows::{core::w, Win32::System::Registry::{RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_DWORD, RRF_RT_REG_SZ}};
    let key = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\Daozai");
    let Some(dir) = std::env::current_exe().ok().and_then(|exe| exe.parent().map(PathBuf::from)) else { return };
    let mut buffer = [0u16; 1024];
    let mut size = (buffer.len() * 2) as u32;
    if unsafe { RegGetValueW(HKEY_CURRENT_USER, key, w!("InstallLocation"), RRF_RT_REG_SZ, None, Some(buffer.as_mut_ptr().cast()), Some(&mut size)) }.is_err() { return; }
    let location = String::from_utf16_lossy(&buffer[..buffer.iter().position(|c| *c == 0).unwrap_or(0)]);
    let same = |a: &std::path::Path, b: &std::path::Path| a.canonicalize().ok().zip(b.canonicalize().ok()).is_some_and(|(a, b)| a == b);
    if location.trim().is_empty() || !same(std::path::Path::new(location.trim()), &dir) { return; }
    let kb = own_files_kb(&dir);
    if kb == 0 { return; }
    let _ = unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, key, w!("EstimatedSize"), REG_DWORD.0, Some((&kb as *const u32).cast()), 4) };
}

#[cfg(windows)]
fn clip_update_window(rect: egui::Rect, scale: f32) {
    use windows::Win32::{Foundation::{BOOL, HWND, LPARAM}, Graphics::Gdi::*,
        System::Threading::GetCurrentThreadId, UI::WindowsAndMessaging::*};
    unsafe extern "system" fn visit(hwnd: HWND, data: LPARAM) -> BOOL {
        let mut title = [0u16; 64];
        let len = GetWindowTextW(hwnd, &mut title) as usize;
        if String::from_utf16_lossy(&title[..len]) != "岛仔更新" { return BOOL(1); }
        let &(rect, scale) = &*(data.0 as *const (egui::Rect, f32));
        let region = update_window_region(rect, scale);
        let old = CreateRectRgn(0, 0, 0, 0);
        GetWindowRgn(hwnd, old);
        if EqualRgn(old, region).as_bool() || SetWindowRgn(hwnd, region, true) == 0 {
            let _ = DeleteObject(region);
        }
        let _ = DeleteObject(old);
        BOOL(0)
    }
    let data = (rect, scale);
    unsafe { let _ = EnumThreadWindows(GetCurrentThreadId(), Some(visit), LPARAM(&data as *const _ as isize)); }
}

#[cfg(windows)]
unsafe fn update_window_region(rect: egui::Rect, scale: f32) -> windows::Win32::Graphics::Gdi::HRGN {
    windows::Win32::Graphics::Gdi::CreateRoundRectRgn(
        (rect.left()*scale).round() as i32, (rect.top()*scale).round() as i32,
        (rect.right()*scale).round() as i32 + 1, (rect.bottom()*scale).round() as i32 + 1,
        (44.0*scale).round() as i32, (44.0*scale).round() as i32,
    )
}

fn update_agent(proxy: Option<&str>, timeout: Duration) -> Result<ureq::Agent, String> {
    // 整体 timeout 会覆盖读取超时，分别限制连接和无数据等待。
    let mut builder = ureq::AgentBuilder::new().try_proxy_from_env(false)
        .timeout_connect(Duration::from_secs(20)).timeout_read(timeout).timeout_write(timeout);
    if let Some(proxy) = proxy {
        builder = builder.proxy(ureq::Proxy::new(proxy).map_err(|_| "代理地址无效，请检查代理设置后重试")?);
    }
    Ok(builder.build())
}

fn http_agent() -> Result<ureq::Agent, String> {
    let proxy = ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy", "HTTP_PROXY", "http_proxy"]
        .iter().find_map(|key| std::env::var(key).ok().filter(|value| !value.trim().is_empty()))
        .or_else(crate::quota::system_proxy);
    update_agent(proxy.as_deref(), Duration::from_secs(15))
}

fn check() -> Result<Option<Release>, String> {
    let response = http_agent()?
        .get(API).set("User-Agent", concat!("Daozai/", env!("CARGO_PKG_VERSION")))
        .set("Accept", "application/vnd.github+json").call().map_err(|error| match error {
            ureq::Error::Status(403 | 429, _) => "检查过于频繁，请稍后重试".to_owned(),
            ureq::Error::Status(404, _) => "暂未找到正式版本，请查看发布页".to_owned(),
            _ => "无法连接更新服务，请检查网络或代理后重试".to_owned(),
        })?;
    let value: serde_json::Value = serde_json::from_reader(response.into_reader())
        .map_err(|_| "更新信息格式异常，请稍后重试".to_owned())?;
    release_info(&value, env!("CARGO_PKG_VERSION"))
}

fn release_info(value: &serde_json::Value, current: &str) -> Result<Option<Release>, String> {
    let Some(tag) = parse_release(value, current)? else { return Ok(None); };
    let name = format!("Daozai-v{}-windows-x64.zip", tag.trim_start_matches('v'));
    let expected = format!("{REPOSITORY}/releases/download/{tag}/{name}");
    let package = value["assets"].as_array().and_then(|assets| assets.iter().find(|asset| asset["name"] == name))
        .and_then(|asset| {
            let hash = asset["digest"].as_str()?.strip_prefix("sha256:")?;
            let size = asset["size"].as_u64()?;
            (asset["browser_download_url"].as_str()? == expected && size > 0 && size <= 200 * 1024 * 1024
                && hash.len() == 64 && hash.bytes().all(|c| c.is_ascii_hexdigit()))
                .then(|| Package { url: expected, size, hash: hash.to_ascii_lowercase() })
        });
    Ok(Some(Release { page: format!("{REPOSITORY}/releases/tag/{tag}"), version: tag,
        notes: value["body"].as_str().unwrap_or("").to_owned(), package }))
}

fn download(release: &Release, tx: &mpsc::Sender<Event>, cancel: &AtomicBool) -> Result<PathBuf, String> {
    let package = release.package.as_ref().ok_or("请通过浏览器下载此版本")?;
    let root = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).ok_or("无法找到更新缓存目录")?.join("Daozai/Updates");
    let work = root.join(format!("{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos()));
    std::fs::create_dir_all(&work).map_err(|_| "无法创建更新缓存")?;
    let result = (|| {
        let response = http_agent()?
            .get(&package.url).call().map_err(|_| "连接下载服务器失败，请检查网络或代理，或使用网盘下载")?;
        if !response.get_url().starts_with("https://") { return Err("更新下载未使用安全连接".into()); }
        let mut input = response.into_reader();
        let mut output = std::fs::File::create(work.join("update.zip")).map_err(|_| "无法保存更新包")?;
        transfer(&mut input, &mut output, package, cancel, |progress| { let _ = tx.send(Event::Progress(progress)); })?;
        std::fs::write(work.join("apply.ps1"), concat!("\u{feff}", include_str!("../installer/update.ps1"))).map_err(|_| "无法准备更新程序")?;
        std::fs::write(work.join("version.txt"), release.version.trim_start_matches('v')).map_err(|_| "无法保存更新信息")?;
        std::fs::write(work.join("hash.txt"), &package.hash).map_err(|_| "无法保存校验信息")?;
        if cancel.load(Ordering::Relaxed) { return Err("下载已取消".into()); }
        Ok(work.clone())
    })();
    if result.is_err() { let _ = std::fs::remove_dir_all(&work); }
    result
}

fn transfer(input: &mut impl Read, output: &mut impl Write, package: &Package, cancel: &AtomicBool, mut progress: impl FnMut(f32)) -> Result<(), String> {
    let mut hash = ring::digest::Context::new(&ring::digest::SHA256);
    let mut buffer = [0u8; 65536]; let mut total = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) { return Err("下载已取消".into()); }
        let length = input.read(&mut buffer).map_err(|_| "下载中断或等待超时，请重试或使用网盘下载")?;
        if length == 0 { break; }
        total += length as u64;
        if total > package.size { return Err("更新包大小不符，请重新下载".into()); }
        output.write_all(&buffer[..length]).map_err(|_| "无法保存更新包，请检查磁盘空间")?;
        hash.update(&buffer[..length]); progress(total as f32 / package.size as f32);
    }
    let digest: String = hash.finish().as_ref().iter().map(|b| format!("{b:02x}")).collect();
    if total != package.size || digest != package.hash { return Err("更新包不完整或校验失败，请重新下载".into()); }
    Ok(())
}

#[cfg(windows)]
fn launch_installer(work: &std::path::Path) -> Result<(), String> {
    let target = std::env::current_exe().map_err(|_| "无法找到当前程序")?;
    let shell = std::env::var_os("SystemRoot").map(PathBuf::from).ok_or("无法找到 Windows 目录")?
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let mut child = crate::child_process::spawn(std::process::Command::new(shell).args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(work.join("apply.ps1")).arg("-WorkPath").arg(work).arg("-TargetExe").arg(target)
        .arg("-ParentProcessId").arg(std::process::id().to_string()))
        .map_err(|_| "无法启动更新程序，请通过浏览器下载安装")?;
    // 脚本先确认缓存和安装目录可用，再允许主程序退出。
    for _ in 0..100 {
        if work.join("ready").exists() { return Ok(()); }
        if child.try_wait().ok().flatten().is_some() { break; }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    Err(std::fs::read_to_string(work.join("error.txt")).unwrap_or_else(|_| "无法准备安装，请通过浏览器下载后安装".into()))
}
#[cfg(not(windows))]
fn launch_installer(_: &std::path::Path) -> Result<(), String> { Err("仅支持 Windows 应用内更新".into()) }

fn version(value: &str) -> Option<[u64; 3]> {
    let mut parts = value.strip_prefix('v').unwrap_or(value).split('.');
    let mut numbers = [0; 3];
    for number in &mut numbers {
        let part = parts.next()?;
        if part.is_empty() || !part.bytes().all(|c| c.is_ascii_digit()) { return None; }
        *number = part.parse().ok()?;
    }
    parts.next().is_none().then_some(numbers)
}

fn parse_release(value: &serde_json::Value, current: &str) -> Result<Option<String>, String> {
    let invalid = || "更新信息格式异常，请稍后重试".to_owned();
    if value["draft"].as_bool() != Some(false) || value["prerelease"].as_bool() != Some(false) {
        return Err(invalid());
    }
    let tag = value["tag_name"].as_str().ok_or_else(invalid)?;
    let latest = version(tag).ok_or_else(invalid)?;
    let current = version(current).ok_or_else(invalid)?;
    Ok((latest > current).then(|| tag.to_owned()))
}

#[cfg(test)]
mod install_size_tests {
    #[test]
    fn install_size_counts_only_daozai_files() {
        let dir = std::env::temp_dir().join(format!("daozai-size-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Other App")).unwrap();
        std::fs::write(dir.join("daozai.exe"), vec![0u8; 3000]).unwrap();
        std::fs::write(dir.join("LICENSE"), vec![0u8; 100]).unwrap();
        std::fs::write(dir.join("Other App").join("big.bin"), vec![0u8; 5 * 1024 * 1024]).unwrap();
        std::fs::write(dir.join("other.exe"), vec![0u8; 2 * 1024 * 1024]).unwrap();
        assert_eq!(super::own_files_kb(&dir), 4, "共享文件夹里其他软件的文件不计入");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn automatic_check_waits_for_startup_and_idle_state() {
        let mut state = State::default();
        let start = Instant::now();
        assert!(!state.should_check(start));
        let due = start + Duration::from_secs(10);
        assert!(!state.should_check(due - Duration::from_millis(1)));
        assert!(state.should_check(due));
        let (_tx, rx) = mpsc::channel(); state.rx = Some(rx);
        assert!(!state.should_check(due));
        state.rx = None; state.dialog_open = true;
        assert!(!state.should_check(due));
        state.dismiss();
        assert!(state.should_check(due));
    }

    #[test]
    fn automatic_check_reschedules_success_and_silent_failure() {
        for (result, delay) in [(Ok(None), Duration::from_secs(24 * 3600)), (Err("网络失败".into()), Duration::from_secs(3600))] {
            let mut state = State::default(); state.automatic_check = true;
            let (tx, rx) = mpsc::channel(); state.rx = Some(rx);
            tx.send(Event::Checked(result)).unwrap();
            let before = Instant::now();
            state.poll(&egui::Context::default());
            let after = Instant::now();
            let next = state.next_check.unwrap();
            assert!(next >= before + delay && next <= after + delay);
            assert!(!state.dialog_open); assert!(!state.focus_requested);
            assert!(!state.automatic_check); assert!(state.rx.is_none());
            assert!(!state.should_check(next - Duration::from_millis(1)));
            assert!(state.should_check(next));
        }
    }

    #[test]
    fn automatic_notice_is_once_per_version_and_manual_check_can_reopen_it() {
        let path=std::env::temp_dir().join(format!("daozai-update-check-{}.json",std::process::id()));
        let _=std::fs::remove_file(&path);
        for (automatic, version, expected) in [(true, "v0.6.65", true), (true, "v0.6.65", false),
            (true, "v0.6.66", true), (false, "v0.6.66", true)] {
            let mut state=State::load(path.clone());
            if path.exists() { assert!(!state.should_check(Instant::now()+Duration::from_secs(3600))); }
            state.automatic_check = automatic;
            let (tx, rx) = mpsc::channel(); state.rx = Some(rx);
            tx.send(Event::Checked(Ok(Some(Release { version: version.into(), notes: String::new(), page: RELEASES.into(),
                package: Some(Package { url: String::new(), size: 1, hash: String::new() }) })))).unwrap();
            state.poll(&egui::Context::default());
            assert_eq!(state.dialog_open, expected); assert_eq!(state.focus_requested, expected);
            assert!(state.rx.is_none()); assert!(state.cancel.is_none()); assert_eq!(state.progress, 0.0);
            assert!(matches!(&state.result, Some(Ok(Some(release))) if release.version == version));
            state.dismiss();
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn background_checks_do_not_overlap_active_work() {
        let mut state = State::default();
        let ctx = egui::Context::default();
        assert!(state.begin_check(&ctx, true, || Ok(None)));
        assert!(!state.begin_check(&ctx, false, || panic!("已有任务时不应发起检查")));
        assert!(matches!(state.rx.as_ref().unwrap().recv_timeout(Duration::from_secs(3)), Ok(Event::Checked(Ok(None)))));
    }

    #[test]
    fn update_requests_use_configured_proxy() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let proxy = format!("http://{}", listener.local_addr().unwrap());
        let worker = std::thread::spawn(move || {
            let start = std::time::Instant::now();
            loop {
                if let Ok((mut stream, _)) = listener.accept() {
                    stream.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
                    let mut request = [0; 4096];
                    let n = stream.read(&mut request).unwrap();
                    assert!(String::from_utf8_lossy(&request[..n]).starts_with("GET http://127.0.0.1:1/update.zip"));
                    stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok").unwrap();
                    return;
                }
                if start.elapsed() > Duration::from_secs(3) { return; }
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        let result = update_agent(Some(&proxy), Duration::from_millis(200)).unwrap()
            .get("http://127.0.0.1:1/update.zip").call();
        worker.join().unwrap();
        assert_eq!(result.unwrap().into_string().unwrap(), "ok");
    }

    #[test]
    fn stalled_update_response_times_out() {
        for (scheme, body) in [("http", false), ("https", false), ("http", true)] {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("{scheme}://{}", listener.local_addr().unwrap());
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            if body {
                stream.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
                stream.read(&mut [0; 4096]).unwrap();
                stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n").unwrap();
            }
            std::thread::sleep(Duration::from_millis(1200));
        });
        let start = std::time::Instant::now();
        let result = update_agent(None, Duration::from_millis(100)).unwrap().get(&url).call();
        if body { assert!(result.unwrap().into_string().is_err()); }
        else { assert!(result.is_err()); }
        let elapsed = start.elapsed();
        worker.join().unwrap();
        assert!(elapsed < Duration::from_secs(1), "等待响应超时未生效：{elapsed:?}");
        }
    }
    fn release(tag: &str) -> serde_json::Value {
        serde_json::json!({"tag_name":tag,"draft":false,"prerelease":false})
    }
    #[test]
    fn compares_versions_numerically_and_never_downgrades() {
        for tag in ["v0.6.40", "0.10.0", "v1.0.0"] {
            assert_eq!(parse_release(&release(tag), "0.6.39"), Ok(Some(tag.into())));
        }
        for tag in ["v0.6.39", "v0.6.9", "v0.5.99"] {
            assert_eq!(parse_release(&release(tag), "0.6.39"), Ok(None));
        }
    }
    #[test]
    fn rejects_invalid_and_unpublished_releases() {
        for tag in ["", "latest", "v1.0", "v1.0.0.1", "v1.0.0-beta", "v+1.0.0"] {
            assert!(parse_release(&release(tag), "0.6.39").is_err());
        }
        for field in ["draft", "prerelease"] {
            let mut value = release("v1.0.0"); value[field] = true.into();
            assert!(parse_release(&value, "0.6.39").is_err());
        }
        assert!(parse_release(&serde_json::json!({}), "0.6.39").is_err());
    }
    #[test]
    fn only_matching_verified_windows_package_can_install() {
        let mut value = release("v0.6.40");
        value["body"] = "更新说明".into();
        value["assets"] = serde_json::json!([{"name":"Daozai-v0.6.40-windows-x64.zip",
            "browser_download_url":format!("{REPOSITORY}/releases/download/v0.6.40/Daozai-v0.6.40-windows-x64.zip"),
            "size":123,"digest":format!("sha256:{}", "a".repeat(64))}]);
        let parsed = release_info(&value, "0.6.39").unwrap().unwrap();
        assert!(parsed.package.is_some()); assert_eq!(parsed.notes, "更新说明");
        for (field, bad) in [("name", serde_json::json!("source.zip")), ("browser_download_url", serde_json::json!("https://example.com/download.zip")),
            ("digest", serde_json::Value::Null), ("digest", serde_json::json!("sha256:bad")), ("size", serde_json::json!(0))] {
            let mut invalid = value.clone(); invalid["assets"][0][field] = bad;
            assert!(release_info(&invalid, "0.6.39").unwrap().unwrap().package.is_none());
        }
    }
    #[test]
    fn download_rejects_truncation_corruption_excess_and_cancellation() {
        let data = b"verified update";
        let package = Package { url: String::new(), size: data.len() as u64,
            hash: ring::digest::digest(&ring::digest::SHA256, data).as_ref().iter().map(|b|format!("{b:02x}")).collect() };
        let cancel = AtomicBool::new(false); let mut output = Vec::new(); let mut progress = 0.0;
        transfer(&mut &data[..], &mut output, &package, &cancel, |value| progress = value).unwrap();
        assert_eq!(output, data); assert_eq!(progress, 1.0);
        for bad in [&data[..3], &b"corrupted data!"[..], &b"verified update excess"[..]] {
            assert!(transfer(&mut &bad[..], &mut Vec::new(), &package, &cancel, |_|{}).is_err());
        }
        cancel.store(true, Ordering::Relaxed);
        assert!(transfer(&mut &data[..], &mut Vec::new(), &package, &cancel, |_|{}).unwrap_err().contains("取消"));
    }
    #[test]
    #[ignore = "需要连接 GitHub"]
    fn live_update_check() { assert!(check().is_ok()); }

    #[test]
    fn background_result_and_disconnect_allow_retry() {
        let ctx = egui::Context::default();
        for disconnected in [false, true] {
            let (tx, rx) = mpsc::channel();
            if !disconnected { tx.send(Event::Checked(Ok(None))).unwrap(); }
            drop(tx);
            let mut state = State::default(); state.rx = Some(rx);
            state.automatic_check = true;
            let before = Instant::now();
            let _ = ctx.run(Default::default(), |ctx| {
                state.poll(ctx);
                egui::CentralPanel::default().show(ctx, |ui| state.ui(ui));
            });
            assert!(state.rx.is_none());
            assert!(!state.automatic_check);
            let delay = if disconnected { Duration::from_secs(3600) } else { Duration::from_secs(24 * 3600) };
            let next = state.next_check.unwrap();
            assert!(next >= before + delay && next <= Instant::now() + delay);
            assert_eq!(!state.message.is_empty(), disconnected);
        }
    }

    #[test]
    fn cancellation_before_install_handoff_keeps_the_app_open() {
        let mut state = State::default();
        let (tx, rx) = mpsc::channel(); state.rx = Some(rx);
        state.cancel = Some(Arc::new(AtomicBool::new(true)));
        let work = std::env::temp_dir().join(format!("daozai-cancel-test-{}", std::process::id()));
        std::fs::create_dir_all(&work).unwrap();
        tx.send(Event::Ready(work.clone())).unwrap();
        state.poll(&egui::Context::default());
        assert_eq!(state.message, "下载已取消");
        assert!(state.rx.is_none()); assert!(!work.exists());
    }

    #[test]
    fn only_a_new_release_opens_the_update_dialog() {
        for result in [Ok(None), Err("网络失败".into()), Ok(Some(Release {
            version: "v0.6.40".into(), notes: String::new(), page: RELEASES.into(), package: None,
        }))] {
            let expected = matches!(&result, Ok(Some(_)));
            let mut state = State::default();
            let (tx, rx) = mpsc::channel(); state.rx = Some(rx);
            tx.send(Event::Checked(result)).unwrap();
            state.poll(&egui::Context::default());
            assert_eq!(state.dialog_open, expected);
            assert_eq!(state.focus_requested, expected);
        }
    }

    #[test]
    fn dismissing_update_dialog_cancels_download_and_keeps_release_available() {
        let mut state = State::default(); state.dialog_open = true;
        state.result = Some(Ok(Some(Release { version: "v0.6.40".into(), notes: String::new(), page: RELEASES.into(), package: None })));
        let cancel = Arc::new(AtomicBool::new(false)); state.cancel = Some(cancel.clone());
        state.dismiss();
        assert!(!state.dialog_open); assert!(cancel.load(Ordering::Relaxed));
        assert!(matches!(state.result, Some(Ok(Some(_)))));
    }

    #[test]
    #[ignore = "按需导出发现新版时的设置界面，使用独立数据目录"]
    fn available_update_preview() {
        let dest = std::path::PathBuf::from(std::env::var_os("DAOZAI_UPDATE_PREVIEW_DIR").expect("需指定预览目录"));
        std::fs::create_dir_all(&dest).unwrap();
        let old = std::env::var_os("DAOZAI_DATA_DIR");
        std::env::set_var("DAOZAI_DATA_DIR", dest.join("data"));
        let ctx = egui::Context::default();
        let mut app = crate::app::App::new(&ctx, Default::default());
        app.updates.result = Some(Ok(Some(Release { version: "v0.6.63".into(),
            notes: "- 更新下载支持 Windows 系统代理\n- 连接失败及时提示，避免长时间停在 0%\n- 新增网盘下载入口".into(), page: RELEASES.into(),
            package: Some(Package { url: String::new(), size: 100, hash: String::new() }) })));
        app.updates.dialog_open = true;
        app.settings_tab = 9;
        app.settings_open = false;
        ctx.set_embed_viewports(false);
        let output = std::sync::Arc::new(std::sync::Mutex::new(None));
        let capture = output.clone();
        egui::Context::set_immediate_viewport_renderer(move |ctx, mut viewport| {
            let id = viewport.ids.this;
            assert_eq!(id, egui::ViewportId::from_hash_of("daozai-update"));
            let raw = egui::RawInput {
                viewport_id: id,
                viewports: std::iter::once((id, egui::ViewportInfo { native_pixels_per_point: Some(1.0), ..Default::default() })).collect(),
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(520.0, 416.0))),
                ..Default::default()
            };
            *capture.lock().unwrap() = Some(ctx.run(raw, |ctx| (viewport.viewport_ui_cb)(ctx)));
        });
        let mut raster = crate::preview_raster::Raster::new(520, 416);
        for (name, progress, message) in [
            ("update-dialog", None, ""),
            ("update-connecting", Some(0.0), ""),
            ("update-downloading", Some(0.42), ""),
            ("update-failed", None, "连接下载服务器失败，请检查网络或代理，或使用网盘下载"),
        ] {
        app.updates.cancel = progress.map(|_| Arc::new(AtomicBool::new(false)));
        app.updates.progress = progress.unwrap_or_default();
        app.updates.message = message.into();
        for frame in 0..5 {
            let root = ctx.run(Default::default(), |ctx| app.settings_ui(ctx, false));
            raster.update_textures(&root.textures_delta);
            let child = output.lock().unwrap().take().unwrap();
            raster.update_textures(&child.textures_delta);
            if frame == 4 {
                raster.buf.fill([0.0, 0.0, 0.0, 0.0]);
                raster.draw(&ctx.tessellate(child.shapes, child.pixels_per_point), child.pixels_per_point);
                let mut rgba = Vec::new();
                raster.to_rgba8(&mut rgba);
                image::save_buffer(dest.join(format!("{name}.png")), &rgba, 520, 416, image::ColorType::Rgba8).unwrap();
            }
            raster.free_textures(&root.textures_delta);
            raster.free_textures(&child.textures_delta);
        }
        }
        if let Some(old) = old { std::env::set_var("DAOZAI_DATA_DIR", old); }
        else { std::env::remove_var("DAOZAI_DATA_DIR"); }
    }
}

#[cfg(all(test, windows))]
#[test]
fn update_window_region_excludes_black_corners() {
    use windows::Win32::Graphics::Gdi::*;
    for scale in [1.0, 1.25, 1.5, 2.0] {
        let rect = egui::Rect::from_min_max(egui::pos2(8.0,8.0), egui::pos2(512.0,408.0));
        unsafe {
            let region = update_window_region(rect, scale);
            for (x,y) in [(9.0,9.0),(511.0,9.0),(9.0,407.0),(511.0,407.0),(1.0,208.0)] {
                assert!(!PtInRegion(region,(x*scale) as i32,(y*scale) as i32).as_bool());
            }
            for (x,y) in [(260.0,208.0),(260.0,9.0),(9.0,208.0)] {
                assert!(PtInRegion(region,(x*scale) as i32,(y*scale) as i32).as_bool());
            }
            let _ = DeleteObject(region);
        }
    }
}
