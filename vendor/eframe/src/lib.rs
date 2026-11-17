//! eframe - the [`egui`] framework crate
//!
//! If you are planning to write an app for web or native,
//! and want to use [`egui`] for everything, then `eframe` is for you!
//!
//! To get started, see the [examples](https://github.com/emilk/egui/tree/master/examples).
//! To learn how to set up `eframe` for web and native, go to <https://github.com/emilk/eframe_template/> and follow the instructions there!
//!
//! In short, you implement [`App`] (especially [`App::update`]) and then
//! call [`crate::run_native`] from your `main.rs`, and/or use `eframe::WebRunner` from your `lib.rs`.
//!
//! ## Compiling for web
//! You need to install the `wasm32` target with `rustup target add wasm32-unknown-unknown`.
//!
//! Build the `.wasm` using `cargo build --target wasm32-unknown-unknown`
//! and then use [`wasm-bindgen`](https://github.com/rustwasm/wasm-bindgen) to generate the JavaScript glue code.
//!
//! See the [`eframe_template` repository](https://github.com/emilk/eframe_template/) for more.
//!
//! ## Simplified usage
//! If your app is only for native, and you don't need advanced features like state persistence,
//! then you can use the simpler function [`run_simple_native`].
//!
//! ## Usage, native:
//! ``` no_run
//! use eframe::egui;
//!
//! fn main() {
//!     let native_options = eframe::NativeOptions::default();
//!     eframe::run_native("My egui App", native_options, Box::new(|cc| Ok(Box::new(MyEguiApp::new(cc)))));
//! }
//!
//! #[derive(Default)]
//! struct MyEguiApp {}
//!
//! impl MyEguiApp {
//!     fn new(cc: &eframe::CreationContext<'_>) -> Self {
//!         // Customize egui here with cc.egui_ctx.set_fonts and cc.egui_ctx.set_visuals.
//!         // Restore app state using cc.storage (requires the "persistence" feature).
//!         // Use the cc.gl (a glow::Context) to create graphics shaders and buffers that you can use
//!         // for e.g. egui::PaintCallback.
//!         Self::default()
//!     }
//! }
//!
//! impl eframe::App for MyEguiApp {
//!    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
//!        egui::CentralPanel::default().show(ctx, |ui| {
//!            ui.heading("Hello World!");
//!        });
//!    }
//! }
//! ```
//!
//! ## Usage, web:
//! ``` no_run
//! # #[cfg(target_arch = "wasm32")]
//! use wasm_bindgen::prelude::*;
//!
//! /// Your handle to the web app from JavaScript.
//! # #[cfg(target_arch = "wasm32")]
//! #[derive(Clone)]
//! #[wasm_bindgen]
//! pub struct WebHandle {
//!     runner: eframe::WebRunner,
//! }
//!
//! # #[cfg(target_arch = "wasm32")]
//! #[wasm_bindgen]
//! impl WebHandle {
//!     /// Installs a panic hook, then returns.
//!     #[allow(clippy::new_without_default)]
//!     #[wasm_bindgen(constructor)]
//!     pub fn new() -> Self {
//!         // Redirect [`log`] message to `console.log` and friends:
//!         eframe::WebLogger::init(log::LevelFilter::Debug).ok();
//!
//!         Self {
//!             runner: eframe::WebRunner::new(),
//!         }
//!     }
//!
//!     /// Call this once from JavaScript to start your app.
//!     #[wasm_bindgen]
//!     pub async fn start(&self, canvas: web_sys::HtmlCanvasElement) -> Result<(), wasm_bindgen::JsValue> {
//!         self.runner
//!             .start(
//!                 canvas,
//!                 eframe::WebOptions::default(),
//!                 Box::new(|cc| Ok(Box::new(MyEguiApp::new(cc))),)
//!             )
//!             .await
//!     }
//!
//!     // The following are optional:
//!
//!     /// Shut down eframe and clean up resources.
//!     #[wasm_bindgen]
//!     pub fn destroy(&self) {
//!         self.runner.destroy();
//!     }
//!
//!     /// Example on how to call into your app from JavaScript.
//!     #[wasm_bindgen]
//!     pub fn example(&self) {
//!         if let Some(app) = self.runner.app_mut::<MyEguiApp>() {
//!             app.example();
//!         }
//!     }
//!
//!     /// The JavaScript can check whether or not your app has crashed:
//!     #[wasm_bindgen]
//!     pub fn has_panicked(&self) -> bool {
//!         self.runner.has_panicked()
//!     }
//!
//!     #[wasm_bindgen]
//!     pub fn panic_message(&self) -> Option<String> {
//!         self.runner.panic_summary().map(|s| s.message())
//!     }
//!
//!     #[wasm_bindgen]
//!     pub fn panic_callstack(&self) -> Option<String> {
//!         self.runner.panic_summary().map(|s| s.callstack())
//!     }
//! }
//! ```
//!
//! ## Feature flags
#![doc = document_features::document_features!()]
//!
//! ## Instrumentation
//! This crate supports using the [profiling](https://crates.io/crates/profiling) crate for instrumentation.
//! You can enable features on the profiling crates in your application to add instrumentation for all
//! crates that support it, including egui. See the profiling crate docs for more information.
//! ```toml
//! [dependencies]
//! profiling = "1.0"
//! [features]
//! profile-with-puffin = ["profiling/profile-with-puffin"]
//! ```
//!

#![warn(missing_docs)] // let's keep eframe well-documented
#![allow(clippy::needless_doctest_main)]

// Re-export all useful libraries:
pub use {egui, egui::emath, egui::epaint};

#[cfg(feature = "glow")]
pub use {egui_glow, glow};

#[cfg(feature = "wgpu")]
pub use {egui_wgpu, wgpu};

mod epi;

// Re-export everything in `epi` so `eframe` users don't have to care about what `epi` is:
pub use epi::*;

pub(crate) mod stopwatch;

// ----------------------------------------------------------------------------
// When compiling for web

#[cfg(target_arch = "wasm32")]
pub use wasm_bindgen;

#[cfg(target_arch = "wasm32")]
pub use web_sys;

#[cfg(target_arch = "wasm32")]
pub mod web;

#[cfg(target_arch = "wasm32")]
pub use web::{WebLogger, WebRunner};

// ----------------------------------------------------------------------------
// When compiling natively

#[cfg(not(target_arch = "wasm32"))]
#[cfg(any(feature = "glow", feature = "wgpu"))]
mod native;

#[cfg(not(target_arch = "wasm32"))]
#[cfg(any(feature = "glow", feature = "wgpu"))]
#[cfg(feature = "persistence")]
pub use native::file_storage::storage_dir;

#[cfg(not(target_arch = "wasm32"))]
pub mod icon_data;

/// This is how you start a native (desktop) app.
///
/// The first argument is name of your app, which is a an identifier
/// used for the save location of persistence (see [`App::save`]).
/// It is also used as the application id on wayland.
/// If you set no title on the viewport, the app id will be used
/// as the title.
///
/// For details about application ID conventions, see the
/// [Desktop Entry Spec](https://specifications.freedesktop.org/desktop-entry-spec/desktop-entry-spec-latest.html#desktop-file-id)
///
/// Call from `fn main` like this:
/// ``` no_run
/// use eframe::egui;
///
/// fn main() -> eframe::Result {
///     let native_options = eframe::NativeOptions::default();
///     eframe::run_native("MyApp", native_options, Box::new(|cc| Ok(Box::new(MyEguiApp::new(cc)))))
/// }
///
/// #[derive(Default)]
/// struct MyEguiApp {}
///
/// impl MyEguiApp {
///     fn new(cc: &eframe::CreationContext<'_>) -> Self {
///         // Customize egui here with cc.egui_ctx.set_fonts and cc.egui_ctx.set_visuals.
///         // Restore app state using cc.storage (requires the "persistence" feature).
///         // Use the cc.gl (a glow::Context) to create graphics shaders and buffers that you can use
///         // for e.g. egui::PaintCallback.
///         Self::default()
///     }
/// }
///
/// impl eframe::App for MyEguiApp {
///    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
///        egui::CentralPanel::default().show(ctx, |ui| {
///            ui.heading("Hello World!");
///        });
///    }
/// }
/// ```
///
/// # Errors
/// This function can fail if we fail to set up a graphics context.
#[cfg(not(target_arch = "wasm32"))]
#[cfg(any(feature = "glow", feature = "wgpu"))]
#[allow(clippy::needless_pass_by_value)]
pub fn run_native(
    app_name: &str,
    mut native_options: NativeOptions,
    app_creator: AppCreator<'_>,
) -> Result {
    #[cfg(not(feature = "__screenshot"))]
    assert!(
        std::env::var("EFRAME_SCREENSHOT_TO").is_err(),
        "EFRAME_SCREENSHOT_TO found without compiling with the '__screenshot' feature"
    );

    if native_options.viewport.title.is_none() {
        native_options.viewport.title = Some(app_name.to_owned());
    }

    let renderer = native_options.renderer;

    #[cfg(all(feature = "glow", feature = "wgpu"))]
    {
        match renderer {
            Renderer::Glow => "glow",
            Renderer::Wgpu => "wgpu",
        };
        log::info!("Both the glow and wgpu renderers are available. Using {renderer}.");
    }

    match renderer {
        #[cfg(feature = "glow")]
        Renderer::Glow => {
            log::debug!("Using the glow renderer");
            native::run::run_glow(app_name, native_options, app_creator)
        }

        #[cfg(feature = "wgpu")]
        Renderer::Wgpu => {
            log::debug!("Using the wgpu renderer");
            native::run::run_wgpu(app_name, native_options, app_creator)
        }
    }
}

// ----------------------------------------------------------------------------

/// The simplest way to get started when writing a native app.
///
/// This does NOT support persistence of custom user data. For that you need to use [`run_native`].
/// However, it DOES support persistence of egui data (window positions and sizes, how far the user has scrolled in a
/// [`ScrollArea`](egui::ScrollArea), etc.) if the persistence feature is enabled.
///
/// # Example
/// ``` no_run
/// fn main() -> eframe::Result {
///     // Our application state:
///     let mut name = "Arthur".to_owned();
///     let mut age = 42;
///
///     let options = eframe::NativeOptions::default();
///     eframe::run_simple_native("My egui App", options, move |ctx, _frame| {
///         egui::CentralPanel::default().show(ctx, |ui| {
///             ui.heading("My egui Application");
///             ui.horizontal(|ui| {
///                 let name_label = ui.label("Your name: ");
///                 ui.text_edit_singleline(&mut name)
///                     .labelled_by(name_label.id);
///             });
///             ui.add(egui::Slider::new(&mut age, 0..=120).text("age"));
///             if ui.button("Increment").clicked() {
///                 age += 1;
///             }
///             ui.label(format!("Hello '{name}', age {age}"));
///         });
///     })
/// }
/// ```
///
/// # Errors
/// This function can fail if we fail to set up a graphics context.
#[cfg(not(target_arch = "wasm32"))]
#[cfg(any(feature = "glow", feature = "wgpu"))]
pub fn run_simple_native(
    app_name: &str,
    native_options: NativeOptions,
    update_fun: impl FnMut(&egui::Context, &mut Frame) + 'static,
) -> Result {
    struct SimpleApp<U> {
        update_fun: U,
    }

    impl<U: FnMut(&egui::Context, &mut Frame) + 'static> App for SimpleApp<U> {
        fn update(&mut self, ctx: &egui::Context, frame: &mut Frame) {
            (self.update_fun)(ctx, frame);
        }
    }

    run_native(
        app_name,
        native_options,
        Box::new(|_cc| Ok(Box::new(SimpleApp { update_fun }))),
    )
}

// ----------------------------------------------------------------------------

/// The different problems that can occur when trying to run `eframe`.
#[derive(Debug)]
pub enum Error {
    /// Something went wrong in user code when creating the app.
    AppCreation(Box<dyn std::error::Error + Send + Sync>),

    /// An error from [`winit`].
    #[cfg(not(target_arch = "wasm32"))]
    Winit(winit::error::OsError),

    /// An error from [`winit::event_loop::EventLoop`].
    #[cfg(not(target_arch = "wasm32"))]
    WinitEventLoop(winit::error::EventLoopError),

    /// An error from [`glutin`] when using [`glow`].
    #[cfg(all(feature = "glow", not(target_arch = "wasm32")))]
    Glutin(glutin::error::Error),

    /// An error from [`glutin`] when using [`glow`].
    #[cfg(all(feature = "glow", not(target_arch = "wasm32")))]
    NoGlutinConfigs(glutin::config::ConfigTemplate, Box<dyn std::error::Error>),

    /// An error from [`glutin`] when using [`glow`].
    #[cfg(feature = "glow")]
    OpenGL(egui_glow::PainterError),

    /// An error from [`wgpu`].
    #[cfg(feature = "wgpu")]
    Wgpu(egui_wgpu::WgpuError),
}

impl std::error::Error for Error {}

#[cfg(not(target_arch = "wasm32"))]
impl From<winit::error::OsError> for Error {
    #[inline]
    fn from(err: winit::error::OsError) -> Self {
        Self::Winit(err)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl From<winit::error::EventLoopError> for Error {
    #[inline]
    fn from(err: winit::error::EventLoopError) -> Self {
        Self::WinitEventLoop(err)
    }
}

#[cfg(all(feature = "glow", not(target_arch = "wasm32")))]
impl From<glutin::error::Error> for Error {
    #[inline]
    fn from(err: glutin::error::Error) -> Self {
        Self::Glutin(err)
    }
}

#[cfg(feature = "glow")]
impl From<egui_glow::PainterError> for Error {
    #[inline]
    fn from(err: egui_glow::PainterError) -> Self {
        Self::OpenGL(err)
    }
}

#[cfg(feature = "wgpu")]
impl From<egui_wgpu::WgpuError> for Error {
    #[inline]
    fn from(err: egui_wgpu::WgpuError) -> Self {
        Self::Wgpu(err)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AppCreation(err) => write!(f, "app creation error: {err}"),

            #[cfg(not(target_arch = "wasm32"))]
            Self::Winit(err) => {
                write!(f, "winit error: {err}")
            }

            #[cfg(not(target_arch = "wasm32"))]
            Self::WinitEventLoop(err) => {
                write!(f, "winit EventLoopError: {err}")
            }

            #[cfg(all(feature = "glow", not(target_arch = "wasm32")))]
            Self::Glutin(err) => {
                write!(f, "glutin error: {err}")
            }

            #[cfg(all(feature = "glow", not(target_arch = "wasm32")))]
            Self::NoGlutinConfigs(template, err) => {
                write!(
                    f,
                    "Found no glutin configs matching the template: {template:?}. Error: {err}"
                )
            }

            #[cfg(feature = "glow")]
            Self::OpenGL(err) => {
                write!(f, "egui_glow: {err}")
            }

            #[cfg(feature = "wgpu")]
            Self::Wgpu(err) => {
                write!(f, "WGPU error: {err}")
            }
        }
    }
}

/// Short for `Result<T, eframe::Error>`.
pub type Result<T = (), E = Error> = std::result::Result<T, E>;

// ----------------------------------------------------------------------------
// 岛仔补丁：只放大/缩小主窗口（灵动岛）的界面，其它窗口（设置、更新、邮件）不受影响。
//
// egui 的 zoom_factor 是全局的，会连带设置窗口一起缩放；这里改为在主窗口的输入里
// 把系统 DPI 乘上倍率，并把指针坐标、画布尺寸换算回同一套逻辑点。这样 egui 按
// 真实物理像素重新栅格化文字与矢量图形，放大后依旧清晰，命中区也保持一致。

#[cfg(not(target_arch = "wasm32"))]
static ROOT_VIEWPORT_SCALE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x3F80_0000); // 1.0

/// 设置主窗口的额外缩放倍率（下一帧生效）。非有限值或越界值会被收回到 0.5..=3.0。
#[cfg(not(target_arch = "wasm32"))]
pub fn set_root_viewport_scale(scale: f32) {
    let scale = if scale.is_finite() { scale.clamp(0.5, 3.0) } else { 1.0 };
    ROOT_VIEWPORT_SCALE.store(scale.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// 当前主窗口的额外缩放倍率。
#[cfg(not(target_arch = "wasm32"))]
pub fn root_viewport_scale() -> f32 {
    f32::from_bits(ROOT_VIEWPORT_SCALE.load(std::sync::atomic::Ordering::Relaxed))
}

/// 把 egui-winit 产生的主窗口输入换算到放大后的逻辑点。
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn scale_root_input(raw: &mut egui::RawInput, scale: f32) {
    if !scale.is_finite() || (scale - 1.0).abs() < 1e-4 {
        return;
    }
    let inv = 1.0 / scale;
    let pos = |p: egui::Pos2| egui::pos2(p.x * inv, p.y * inv);
    let rect = |r: egui::Rect| egui::Rect::from_min_max(pos(r.min), pos(r.max));
    if let Some(r) = raw.screen_rect.as_mut() {
        *r = rect(*r);
    }
    if let Some(info) = raw.viewports.get_mut(&egui::ViewportId::ROOT) {
        if let Some(ppp) = info.native_pixels_per_point.as_mut() {
            *ppp *= scale;
        }
        info.inner_rect = info.inner_rect.map(rect);
        info.outer_rect = info.outer_rect.map(rect);
        info.monitor_size = info.monitor_size.map(|s| s * inv);
    }
    for event in &mut raw.events {
        match event {
            egui::Event::PointerMoved(p) => *p = pos(*p),
            egui::Event::PointerButton { pos: p, .. } => *p = pos(*p),
            egui::Event::Touch { pos: p, .. } => *p = pos(*p),
            egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta, .. } => *delta *= inv,
            _ => {}
        }
    }
}

/// 输入法候选框等输出坐标要换回 egui-winit 的逻辑点。
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn scale_root_output(output: &mut egui::PlatformOutput, scale: f32) {
    if !scale.is_finite() || (scale - 1.0).abs() < 1e-4 {
        return;
    }
    let rect = |r: egui::Rect| egui::Rect::from_min_max(egui::pos2(r.min.x * scale, r.min.y * scale), egui::pos2(r.max.x * scale, r.max.y * scale));
    if let Some(ime) = output.ime.as_mut() {
        ime.rect = rect(ime.rect);
        ime.cursor_rect = rect(ime.cursor_rect);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod root_scale_tests {
    use super::*;

    #[test]
    fn enlarged_root_uses_physical_font_pixels_with_settings_open() {
        let ctx=egui::Context::default();
        let child=egui::ViewportId::from_hash_of("settings-scale-check");
        for dpi in [1.0,1.25,1.5] {
            for scale in [0.8,1.0,1.2,1.4] {
                let mut root=egui::RawInput::default();
                root.viewports.get_mut(&egui::ViewportId::ROOT).unwrap().native_pixels_per_point=Some(dpi);
                scale_root_input(&mut root,scale);
                let root_output=ctx.run(root,|ctx|{
                    assert!((ctx.pixels_per_point()-dpi*scale).abs()<0.0001,"岛体的字形与矢量边缘必须按实际像素生成");
                    ctx.fonts(|fonts|assert!((fonts.pixels_per_point()-dpi*scale).abs()<0.0001));
                });
                assert!((root_output.pixels_per_point-dpi*scale).abs()<0.0001);
                let child_input=egui::RawInput{viewport_id:child,viewports:std::iter::once((child,egui::ViewportInfo{native_pixels_per_point:Some(dpi),..Default::default()})).collect(),..Default::default()};
                let _=ctx.run(child_input,|ctx|assert_eq!(ctx.pixels_per_point(),dpi));
            }
        }
    }

    #[test]
    fn root_input_is_converted_into_scaled_points() {
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 300.0))),
            events: vec![
                egui::Event::PointerMoved(egui::pos2(600.0, 30.0)),
                egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0.0, 24.0), modifiers: Default::default() },
            ],
            ..Default::default()
        };
        raw.viewports.insert(egui::ViewportId::ROOT, egui::ViewportInfo { native_pixels_per_point: Some(1.25), ..Default::default() });
        scale_root_input(&mut raw, 1.5);
        assert_eq!(raw.screen_rect.unwrap().width(), 800.0);
        assert_eq!(raw.viewports[&egui::ViewportId::ROOT].native_pixels_per_point, Some(1.875));
        assert!(matches!(raw.events[0], egui::Event::PointerMoved(p) if p == egui::pos2(400.0, 20.0)));
        assert!(matches!(raw.events[1], egui::Event::MouseWheel { delta, .. } if delta == egui::vec2(0.0, 16.0)));
    }

    #[test]
    fn unit_scale_leaves_input_untouched() {
        let mut raw = egui::RawInput { events: vec![egui::Event::PointerMoved(egui::pos2(3.0, 4.0))], ..Default::default() };
        scale_root_input(&mut raw, 1.0);
        assert!(matches!(raw.events[0], egui::Event::PointerMoved(p) if p == egui::pos2(3.0, 4.0)));
    }
}
