//! Couche d'intégration entre Servo et l'application SuriBrows.
//!
//! Ce module contient trois éléments clés :
//!
//! 1. **[`Waker`] / [`WakerEvent`]** : Le pont `Send + Sync` entre les threads
//!    internes de Servo (Constellation, script, réseau) et le thread principal Winit.
//!    C'est le seul mécanisme de synchronisation inter-threads de l'embedder.
//!
//! 2. **[`WebViewDelegate`] pour [`AppState`]** : Callbacks invoqués par Servo pour
//!    notifier l'embedder des changements d'état (nouveau frame, changement d'URL, etc.).
//!
//! 3. **[`SuriBrowsServoDelegate`]** : Callbacks moteur de niveau global (erreurs,
//!    chargement de ressources hors-webview).

use servo::{WebResourceLoad, WebResourceResponse, WebView, WebViewDelegate};
use tracing::{debug, warn};
use url::Url;
use winit::event_loop::{EventLoop, EventLoopProxy};

use crate::browser::AppState;

// ─────────────────────────────────────────────────────────────────────────────
// Waker : pont Servo → Winit
// ─────────────────────────────────────────────────────────────────────────────

/// Événement marqueur envoyé à travers le `EventLoopProxy` de Winit.
///
/// Quand un thread interne de Servo a terminé un travail (page chargée,
/// frame prête, réponse réseau reçue), il appelle `Waker::wake()`,
/// qui envoie ce `WakerEvent`. La boucle Winit le reçoit dans
/// `user_event()` et appelle `servo.spin_event_loop()` en réponse.
#[derive(Debug)]
pub struct WakerEvent;

/// Pont thread-safe entre les threads internes de Servo et la boucle
/// d'événements Winit sur le thread principal.
///
/// ## Pourquoi c'est nécessaire
///
/// Servo exécute le JavaScript, le layout parallèle, et le réseau sur
/// des threads séparés (gérés par la Constellation). Quand ces threads
/// produisent des résultats nécessitant un traitement sur le thread
/// principal (ex: un frame à peindre), ils appellent `wake()` sur ce struct.
///
/// ## Thread Safety
///
/// `Waker` est `Clone + Send + Sync` car `EventLoopProxy` l'est.
/// C'est requis par le trait `EventLoopWaker: 'static + Send + Sync`.
#[derive(Clone)]
pub struct Waker(EventLoopProxy<WakerEvent>);

impl Waker {
    pub fn new(event_loop: &EventLoop<WakerEvent>) -> Self {
        Self(event_loop.create_proxy())
    }
}

/// Implémentation du trait Servo `EventLoopWaker`.
///
/// Ce trait est défini dans `embedder_traits` et est le contrat que tout
/// embedder Servo doit remplir pour la communication inter-threads.
impl embedder_traits::EventLoopWaker for Waker {
    fn clone_box(&self) -> Box<dyn embedder_traits::EventLoopWaker> {
        Box::new(Self(self.0.clone()))
    }

    fn wake(&self) {
        if let Err(error) = self.0.send_event(WakerEvent) {
            warn!(?error, "Échec du réveil de la boucle d'événements Winit");
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// WebViewDelegate : callbacks Servo → embedder (par webview)
// ─────────────────────────────────────────────────────────────────────────────

/// Implémentation du `WebViewDelegate` pour `AppState`.
///
/// Le trait `WebViewDelegate` a 34 méthodes, toutes avec des implémentations
/// par défaut (no-op). On n'override que celles essentielles au squelette.
///
/// ## Méthodes implémentées
///
/// - `notify_new_frame_ready` : déclenche un redraw Winit quand Servo a composité
/// - `notify_url_changed` : met à jour le titre de la fenêtre
/// - `notify_page_title_changed` : idem, depuis la balise `<title>`
///
/// ## Points d'extension futurs
///
/// - `load_web_resource()` → middleware privacy (adblock, tracker blocking)
/// - `notify_cursor_changed()` → changement de curseur souris
/// - `request_navigation()` → contrôle de navigation (filtrage d'URLs)
impl WebViewDelegate for AppState {
    /// Appelé quand Servo a composité un nouveau frame prêt à être affiché.
    ///
    /// On demande un redraw à Winit, ce qui déclenchera `RedrawRequested`
    /// → `webview.paint()` + `rendering_context.present()`.
    ///
    /// SECURITY (V-4): Wrapped with panic safety for FFI boundary protection.
    fn notify_new_frame_ready(&self, _webview: WebView) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.window.request_redraw();
        }));
        // Panic recovery: if window access fails, skip this frame redraw
    }

    /// Appelé quand l'URL de la page change (navigation, redirection).
    /// Servo fournit la nouvelle URL directement en paramètre.
    ///
    /// SECURITY (V-4): Wrapped with panic safety to prevent UB if concurrent
    /// access to Rc<RefCell<>> causes a panic across the FFI boundary.
    fn notify_url_changed(&self, _webview: WebView, url: Url) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.window
                .set_title(&format!("SuriBrows — {}", url));
            self.urlbar.borrow_mut().set_url(&url);
            *self.current_url.borrow_mut() = Some(url.clone());
            if let Some(ref engine) = self.adblock_engine {
                engine.clear_cache();
            }
        }));
        // If panic occurs (e.g., RefCell already borrowed), silently recover
        // instead of allowing undefined behavior across FFI boundary
    }

    /// Appelé quand le titre de la page change (balise `<title>`).
    /// Servo fournit le nouveau titre en paramètre (None si pas de `<title>`).
    ///
    /// SECURITY (V-4): Wrapped with panic safety for FFI boundary protection.
    fn notify_page_title_changed(&self, _webview: WebView, title: Option<String>) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if let Some(title) = title {
                self.window
                    .set_title(&format!("SuriBrows — {}", title));
            }
        }));
        // Panic recovery: prevent UB if window access causes panic
    }

    /// Intercepte les requêtes réseau pour le filtrage adblock.
    ///
    /// Appelé pour chaque requête HTTP émise par Servo. Si le moteur adblock
    /// est actif et que l'URL match un filtre, la requête est annulée.
    /// Sinon, on ne fait rien et Servo procède normalement.
    ///
    /// SECURITY (V-7 partial fix): Also updates URL bar immediately for main frame
    /// navigations to reduce TOCTOU window for phishing attacks.
    /// SECURITY (V-4): Wrapped with panic safety for FFI boundary protection.
    fn load_web_resource(&self, _webview: WebView, load: WebResourceLoad) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let request = load.request();
            let url = request.url.as_str();

            // SECURITY (V-7): Update URL bar immediately for main frame navigations
            // This reduces (but doesn't eliminate) the TOCTOU window where the displayed
            // URL doesn't match the loading content.
            if request.is_for_main_frame {
                // Optimistically update URL bar before the page loads
                self.urlbar.borrow_mut().set_url(&request.url);
                self.window
                    .set_title(&format!("Loading — {}", request.url));
            }

            // Ad-blocking logic
            let Some(ref engine) = self.adblock_engine else { return };

            let source_url = self
                .current_url
                .borrow()
                .as_ref()
                .map(|u| u.to_string())
                .unwrap_or_default();
            let request_type = if request.is_for_main_frame {
                "document"
            } else {
                "other"
            };

            if engine.should_block(url, &source_url, request_type) {
                debug!(url, "Requête bloquée par adblock");
                let response = WebResourceResponse::new(request.url.clone());
                load.intercept(response).cancel();
            }
        }));
        // Panic recovery: if RefCell borrow fails or adblock panics, silently continue
        // This prevents crashes but allows the request to proceed (fail-open for safety)
    }
}
