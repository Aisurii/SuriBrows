//! Boucle d'événements Winit et gestion du cycle de vie du navigateur.
//!
//! ## Pattern "Two-Phase App"
//!
//! Winit 0.30 impose que les fenêtres soient créées à l'intérieur du callback
//! `resumed()`, et non dans `main()`. Mais Servo a besoin d'un handle de fenêtre
//! pour créer son `WindowRenderingContext`. On résout ce problème avec un enum
//! à deux états :
//!
//! ```text
//! App::Initial(Waker)  →  [resumed() appelé]  →  App::Running(Rc<AppState>)
//! ```
//!
//! ## Flux de communication Winit ↔ Servo
//!
//! ```text
//! Threads Servo (script, layout, réseau)
//!         │
//!         │ wake()  ← EventLoopWaker trait
//!         ▼
//!   EventLoopProxy::send_event(WakerEvent)
//!         │
//!         ▼
//!   Winit EventLoop (thread principal)
//!     └─ user_event() → servo.spin_event_loop()
//!           └─ Servo traite ses messages internes
//!           └─ Appelle WebViewDelegate méthodes
//!              (notify_new_frame_ready, notify_url_changed, etc.)
//! ```
//!
//! ## Architecture du rendu
//!
//! ```text
//! Window (1280x800)
//! ┌──────────────────────────────────────┐
//! │ Chrome (40px) — GL direct sur window │
//! ├──────────────────────────────────────┤
//! │ Servo WebView — OffscreenRenderCtx   │
//! │ blitté dans la zone restante         │
//! └──────────────────────────────────────┘
//! ```

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use euclid::Scale;
use servo::{InputEvent, WheelDelta, WheelEvent, WheelMode};
use servo::{MouseButton as ServoMouseButton, MouseButtonAction, MouseButtonEvent};
use servo::{MouseLeftViewportEvent, MouseMoveEvent};
use servo::{
    OffscreenRenderingContext, RenderingContext, Servo, ServoBuilder, WebView, WebViewBuilder,
    WindowRenderingContext,
};
use url::Url;
use webrender_api::units::DevicePoint;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, MouseButton as WinitMouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::EventLoop;
use winit::keyboard::{Key, NamedKey};
use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::window::Window;

use crate::chrome::ChromeRenderer;
use crate::config::Config;
use crate::preferences::build_servo_preferences;
use crate::rendering;
use crate::servo_glue::{Waker, WakerEvent};
use crate::settings;
use crate::urlbar::UrlBar;

// ─────────────────────────────────────────────────────────────────────────────
// AppState : état partagé entre Winit et Servo
// ─────────────────────────────────────────────────────────────────────────────

/// État partagé de l'application, créé lors de `resumed()`.
///
/// Encapsulé dans `Rc` car :
/// - `WebViewDelegate` attend un `Rc<dyn WebViewDelegate>`
/// - Servo et l'App doivent référencer les mêmes données
/// - Tout vit sur le thread principal (pas besoin d'`Arc`)
pub struct AppState {
    /// Handle de la fenêtre Winit.
    pub window: Window,

    /// Instance du moteur Servo.
    pub servo: Servo,

    /// Contexte de rendu OpenGL plein écran (surface fenêtre).
    /// Utilisé pour le chrome (barre d'URL) et le blit du FBO.
    pub window_rendering_context: Rc<WindowRenderingContext>,

    /// Contexte de rendu offscreen (FBO) pour le WebView.
    /// Servo peint dedans via `webview.paint()`.
    pub offscreen_context: Rc<OffscreenRenderingContext>,

    /// WebViews actives.
    pub webviews: RefCell<Vec<WebView>>,

    /// Position courante du curseur en device pixels.
    pub cursor_position: Cell<DevicePoint>,

    /// État des modificateurs clavier (Ctrl, Shift, Alt, Meta).
    pub modifiers: Cell<winit::keyboard::ModifiersState>,

    /// Moteur adblock.
    pub adblock_engine: Option<crate::privacy::AdblockEngine>,

    /// URL courante de la page.
    pub current_url: RefCell<Option<Url>>,

    /// État de la barre d'URL.
    pub urlbar: RefCell<UrlBar>,

    /// Renderer GL pour le chrome (barre d'URL).
    pub chrome: RefCell<ChromeRenderer>,

    /// Configuration de l'application.
    pub config: Config,
}

// ─────────────────────────────────────────────────────────────────────────────
// App : enum deux phases
// ─────────────────────────────────────────────────────────────────────────────

/// Application à deux phases de vie.
#[allow(clippy::large_enum_variant)]
pub enum App {
    /// Phase pré-initialisation : on attend que Winit appelle `resumed()`.
    Initial {
        waker: Waker,
        initial_url: Url,
        config: Config,
    },

    /// Phase opérationnelle : le navigateur est actif.
    Running(Rc<AppState>),
}

impl App {
    /// Crée l'application dans son état initial avec l'URL à charger.
    pub fn new(event_loop: &EventLoop<WakerEvent>, initial_url: Url, config: Config) -> Self {
        Self::Initial {
            waker: Waker::new(event_loop),
            initial_url,
            config,
        }
    }
}

/// Calcule la taille du webview (fenêtre moins le chrome).
fn webview_size(window_size: PhysicalSize<u32>, chrome_height: u32) -> PhysicalSize<u32> {
    PhysicalSize::new(
        window_size.width,
        window_size.height.saturating_sub(chrome_height),
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// ApplicationHandler : dispatch des événements Winit
// ─────────────────────────────────────────────────────────────────────────────

impl ApplicationHandler<WakerEvent> for App {
    /// Appelé une fois par Winit quand l'application est prête à créer des fenêtres.
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let (waker, initial_url, config) = match self {
            Self::Initial {
                waker,
                initial_url,
                config,
            } => (waker.clone(), initial_url.clone(), config.clone()),
            Self::Running(_) => return,
        };

        // ── 1. Créer la fenêtre Winit ──────────────────────────────────
        let display_handle = event_loop
            .display_handle()
            .expect("Impossible d'obtenir le DisplayHandle");

        let window_attributes = Window::default_attributes()
            .with_title(&config.general.window_title)
            .with_inner_size(winit::dpi::LogicalSize::new(
                config.window.width as f64,
                config.window.height as f64,
            ));

        let window = event_loop
            .create_window(window_attributes)
            .expect("Impossible de créer la fenêtre Winit");

        let window_handle = window
            .window_handle()
            .expect("Impossible d'obtenir le WindowHandle");

        // ── 2. Créer les contextes de rendu ─────────────────────────────
        // Contexte fenêtre (plein écran) — pour le chrome et le blit.
        let window_rendering_context =
            rendering::create_rendering_context(display_handle, window_handle, window.inner_size());

        // Contexte offscreen (FBO) — Servo peint dedans.
        let inner_size = window.inner_size();
        let wv_size = webview_size(inner_size, config.chrome.height);
        let offscreen_context = Rc::new(window_rendering_context.offscreen_context(wv_size));

        // ── 3. Initialiser le chrome renderer ───────────────────────────
        let gl = window_rendering_context.glow_gl_api();
        let chrome_renderer = unsafe { ChromeRenderer::new(gl, &config.chrome) };

        // ── 4. Construire l'instance Servo ──────────────────────────────
        let servo = ServoBuilder::default()
            .preferences(build_servo_preferences(&config.servo, &config.privacy))
            .event_loop_waker(Box::new(waker))
            .build();

        // ── 5. Encapsuler dans AppState ─────────────────────────────────
        let adblock_engine = crate::privacy::AdblockEngine::new();
        let app_state = Rc::new(AppState {
            window,
            servo,
            window_rendering_context,
            offscreen_context: offscreen_context.clone(),
            webviews: RefCell::new(Vec::new()),
            cursor_position: Cell::new(DevicePoint::zero()),
            modifiers: Cell::new(winit::keyboard::ModifiersState::default()),
            adblock_engine,
            current_url: RefCell::new(None),
            urlbar: RefCell::new(UrlBar::new(config.search.engine_url.clone())),
            chrome: RefCell::new(chrome_renderer),
            config,
        });

        // ── 6. Créer la WebView initiale ────────────────────────────────
        let url = initial_url;
        let scale_factor = app_state.window.scale_factor() as f32;

        let webview = WebViewBuilder::new(
            &app_state.servo,
            offscreen_context as Rc<dyn RenderingContext>,
        )
        .url(url)
        .hidpi_scale_factor(Scale::new(scale_factor))
        .delegate(app_state.clone())
        .build();

        app_state.webviews.borrow_mut().push(webview);

        // ── 7. Transition Initial → Running ─────────────────────────────
        *self = Self::Running(app_state);
    }

    /// Appelé quand un `WakerEvent` arrive depuis les threads Servo.
    fn user_event(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop, _event: WakerEvent) {
        if let Self::Running(state) = self {
            state.servo.spin_event_loop();
        }
    }

    /// Dispatch des événements fenêtre Winit vers Servo.
    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        // Toujours faire tourner la boucle Servo en premier.
        if let Self::Running(state) = self {
            state.servo.spin_event_loop();
        }

        let chrome_h = if let Self::Running(state) = self {
            state.config.chrome.height as f32
        } else {
            40.0
        };

        match event {
            // ── Fermeture de la fenêtre ────────────────────────────────
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }

            // ── Redraw : blit FBO + chrome ─────────────────────────────
            WindowEvent::RedrawRequested => {
                if let Self::Running(state) = self {
                    let inner_size = state.window.inner_size();

                    // 1. Servo peint dans le FBO offscreen
                    if let Some(webview) = state.webviews.borrow().last() {
                        webview.paint();
                    }

                    // 2. Préparer le contexte fenêtre et blitter le FBO
                    state.window_rendering_context.prepare_for_rendering();

                    if let Some(blit) = state.offscreen_context.render_to_parent_callback() {
                        let gl = state.window_rendering_context.glow_gl_api();
                        // GL coords: (0,0) = bottom-left
                        // Blit to bottom portion: y=0 to y=height-40 (leaves top 40px for chrome)
                        let target_rect = euclid::default::Rect::new(
                            euclid::default::Point2D::new(0, 0),
                            euclid::default::Size2D::new(
                                inner_size.width as i32,
                                inner_size.height.saturating_sub(state.config.chrome.height) as i32,
                            ),
                        );
                        blit(&gl, target_rect);
                    }

                    // 3. Dessiner le chrome (barre d'URL) dans les 40px du haut
                    let urlbar = state.urlbar.borrow();
                    let cursor_offset = if urlbar.is_focused() {
                        Some(urlbar.cursor_char_offset())
                    } else {
                        None
                    };
                    unsafe {
                        state.chrome.borrow().draw(
                            inner_size.width,
                            inner_size.height,
                            urlbar.display_text(),
                            urlbar.is_focused(),
                            cursor_offset,
                        );
                    }

                    // 4. Présenter
                    state.window_rendering_context.present();
                }
            }

            // ── Scroll souris ──────────────────────────────────────────
            WindowEvent::MouseWheel { delta, .. } => {
                if let Self::Running(state) = self {
                    let pos = state.cursor_position.get();
                    // Ne forwarde le scroll que si le curseur est dans la zone webview
                    if pos.y >= chrome_h
                        && let Some(webview) = state.webviews.borrow().last()
                    {
                        let (delta_x, delta_y, mode) = match delta {
                            MouseScrollDelta::LineDelta(dx, dy) => {
                                ((dx * 76.0) as f64, (dy * 76.0) as f64, WheelMode::DeltaLine)
                            }
                            MouseScrollDelta::PixelDelta(delta) => {
                                (delta.x, delta.y, WheelMode::DeltaPixel)
                            }
                        };

                        let adjusted = DevicePoint::new(pos.x, pos.y - chrome_h);
                        webview.notify_input_event(InputEvent::Wheel(WheelEvent::new(
                            WheelDelta {
                                x: delta_x,
                                y: delta_y,
                                z: 0.0,
                                mode,
                            },
                            adjusted.into(),
                        )));
                    }
                }
            }

            // ── Redimensionnement de la fenêtre ────────────────────────
            WindowEvent::Resized(new_size) => {
                if let Self::Running(state) = self {
                    // Redimensionner le contexte fenêtre
                    state.window_rendering_context.resize(new_size);
                    // Redimensionner le FBO offscreen (zone webview)
                    let wv_size = webview_size(new_size, state.config.chrome.height);
                    state.offscreen_context.resize(wv_size);
                }
            }

            // ── Modificateurs clavier (Ctrl, Shift, Alt, Meta) ────────
            WindowEvent::ModifiersChanged(new_modifiers) => {
                if let Self::Running(state) = self {
                    state.modifiers.set(new_modifiers.state());
                }
            }

            // ── Mouvement du curseur ──────────────────────────────────
            WindowEvent::CursorMoved { position, .. } => {
                if let Self::Running(state) = self {
                    let point = DevicePoint::new(position.x as f32, position.y as f32);
                    state.cursor_position.set(point);

                    // Ne forwarde que si le curseur est dans la zone webview
                    if position.y >= chrome_h as f64 {
                        let adjusted = DevicePoint::new(
                            position.x as f32,
                            (position.y - chrome_h as f64) as f32,
                        );
                        if let Some(webview) = state.webviews.borrow().last() {
                            webview.notify_input_event(InputEvent::MouseMove(MouseMoveEvent::new(
                                adjusted.into(),
                            )));
                        }
                    }
                }
            }

            // ── Curseur quitte la fenêtre ─────────────────────────────
            WindowEvent::CursorLeft { .. } => {
                if let Self::Running(state) = self
                    && let Some(webview) = state.webviews.borrow().last()
                {
                    webview.notify_input_event(InputEvent::MouseLeftViewport(
                        MouseLeftViewportEvent::default(),
                    ));
                }
            }

            // ── Clics souris ──────────────────────────────────────────
            WindowEvent::MouseInput {
                state: btn_state,
                button,
                ..
            } => {
                if let Self::Running(state) = self {
                    let pos = state.cursor_position.get();

                    if pos.y < chrome_h {
                        // Clic dans la zone chrome → focus la barre d'URL
                        if btn_state == ElementState::Pressed && button == WinitMouseButton::Left {
                            state.urlbar.borrow_mut().focus();
                            state.window.request_redraw();
                        }
                    } else {
                        // Clic dans la zone webview → unfocus urlbar + forward
                        if btn_state == ElementState::Pressed {
                            let was_focused = state.urlbar.borrow().is_focused();
                            if was_focused {
                                state.urlbar.borrow_mut().unfocus();
                                state.window.request_redraw();
                            }
                        }

                        let adjusted = DevicePoint::new(pos.x, pos.y - chrome_h);
                        if let Some(webview) = state.webviews.borrow().last() {
                            let servo_button = match button {
                                WinitMouseButton::Left => ServoMouseButton::Left,
                                WinitMouseButton::Right => ServoMouseButton::Right,
                                WinitMouseButton::Middle => ServoMouseButton::Middle,
                                WinitMouseButton::Back => ServoMouseButton::Back,
                                WinitMouseButton::Forward => ServoMouseButton::Forward,
                                WinitMouseButton::Other(id) => ServoMouseButton::Other(id),
                            };
                            let action = match btn_state {
                                ElementState::Pressed => MouseButtonAction::Down,
                                ElementState::Released => MouseButtonAction::Up,
                            };
                            webview.notify_input_event(InputEvent::MouseButton(
                                MouseButtonEvent::new(action, servo_button, adjusted.into()),
                            ));
                        }
                    }
                }
            }

            // ── Saisie clavier ────────────────────────────────────────
            WindowEvent::KeyboardInput { event, .. } => {
                if let Self::Running(state) = self {
                    let mods = state.modifiers.get();

                    // ── Raccourcis globaux (toujours actifs) ──────────
                    if event.state == ElementState::Pressed {
                        // Ctrl+L : focus barre d'URL
                        if mods.control_key()
                            && let Key::Character(ref c) = event.logical_key
                            && (c.as_str() == "l" || c.as_str() == "L")
                        {
                            state.urlbar.borrow_mut().focus();
                            state.window.request_redraw();
                            return;
                        }

                        // Ctrl+, : open settings
                        if mods.control_key()
                            && let Key::Character(ref c) = event.logical_key
                            && c.as_str() == ","
                        {
                            let html = settings::generate_settings_html(&state.config);
                            let encoded = settings::url_encode(&html);
                            let data_url = format!("data:text/html;charset=utf-8,{encoded}");
                            if let Ok(url) = Url::parse(&data_url)
                                && let Some(webview) = state.webviews.borrow().last()
                            {
                                webview.load(url);
                            }
                            return;
                        }

                        // Ctrl+R : recharger
                        if mods.control_key()
                            && let Key::Character(ref c) = event.logical_key
                            && (c.as_str() == "r" || c.as_str() == "R")
                        {
                            if let Some(webview) = state.webviews.borrow().last() {
                                webview.reload();
                            }
                            return;
                        }

                        // F5 : recharger
                        if let Key::Named(NamedKey::F5) = event.logical_key {
                            if let Some(webview) = state.webviews.borrow().last() {
                                webview.reload();
                            }
                            return;
                        }

                        // Alt+Left : retour
                        if mods.alt_key()
                            && let Key::Named(NamedKey::ArrowLeft) = event.logical_key
                        {
                            if let Some(webview) = state.webviews.borrow().last() {
                                webview.go_back(1);
                            }
                            return;
                        }

                        // Alt+Right : avant
                        if mods.alt_key()
                            && let Key::Named(NamedKey::ArrowRight) = event.logical_key
                        {
                            if let Some(webview) = state.webviews.borrow().last() {
                                webview.go_forward(1);
                            }
                            return;
                        }
                    }

                    // ── URL bar focusée → consommer les touches ──────
                    if state.urlbar.borrow().is_focused() && event.state == ElementState::Pressed {
                        let mut urlbar = state.urlbar.borrow_mut();

                        match &event.logical_key {
                            Key::Named(NamedKey::Enter) => {
                                if let Some(url) = urlbar.submit() {
                                    drop(urlbar);
                                    if let Some(webview) = state.webviews.borrow().last() {
                                        webview.load(url);
                                    }
                                }
                            }
                            Key::Named(NamedKey::Escape) => {
                                urlbar.unfocus();
                            }
                            Key::Named(NamedKey::Backspace) => {
                                urlbar.backspace();
                            }
                            Key::Named(NamedKey::Delete) => {
                                urlbar.delete();
                            }
                            Key::Named(NamedKey::ArrowLeft) => {
                                urlbar.move_cursor_left();
                            }
                            Key::Named(NamedKey::ArrowRight) => {
                                urlbar.move_cursor_right();
                            }
                            Key::Named(NamedKey::Home) => {
                                urlbar.home();
                            }
                            Key::Named(NamedKey::End) => {
                                urlbar.end();
                            }
                            Key::Character(c) => {
                                if mods.control_key() && (c.as_str() == "a" || c.as_str() == "A") {
                                    urlbar.select_all();
                                } else if !mods.control_key() && !mods.alt_key() {
                                    for ch in c.chars() {
                                        urlbar.insert_char(ch);
                                    }
                                }
                            }
                            _ => {}
                        }

                        state.window.request_redraw();
                        return;
                    }

                    // ── Passer à Servo (URL bar pas focusée) ─────────
                    if let Some(webview) = state.webviews.borrow().last() {
                        let keyboard_event =
                            crate::keyutils::keyboard_event_from_winit(&event, mods);
                        webview.notify_input_event(InputEvent::Keyboard(keyboard_event));
                    }
                }
            }

            _ => (),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_CHROME_HEIGHT: u32 = 40;

    // ── webview_size ──────────────────────────────────────────────────

    #[test]
    fn test_webview_size_subtracts_chrome_height() {
        let result = webview_size(PhysicalSize::new(1280, 800), TEST_CHROME_HEIGHT);
        assert_eq!(result, PhysicalSize::new(1280, 760));
    }

    #[test]
    fn test_webview_size_preserves_width() {
        let result = webview_size(PhysicalSize::new(1920, 1080), TEST_CHROME_HEIGHT);
        assert_eq!(result.width, 1920);
    }

    #[test]
    fn test_webview_size_saturates_at_zero() {
        // Height 20 < chrome_height 40, saturating_sub gives 0
        let result = webview_size(PhysicalSize::new(100, 20), TEST_CHROME_HEIGHT);
        assert_eq!(result, PhysicalSize::new(100, 0));
    }

    #[test]
    fn test_webview_size_exact_chrome_height() {
        let result = webview_size(
            PhysicalSize::new(100, TEST_CHROME_HEIGHT),
            TEST_CHROME_HEIGHT,
        );
        assert_eq!(result, PhysicalSize::new(100, 0));
    }

    #[test]
    fn test_webview_size_zero() {
        let result = webview_size(PhysicalSize::new(0, 0), TEST_CHROME_HEIGHT);
        assert_eq!(result, PhysicalSize::new(0, 0));
    }

    #[test]
    fn test_default_config_chrome_height_is_40() {
        let config = Config::default();
        assert_eq!(config.chrome.height, 40);
    }
}
