//! Factory pour le contexte de rendu GPU.
//!
//! Ce module isole la création du `WindowRenderingContext` (basé sur surfman/OpenGL)
//! du reste de l'application. Cela facilite :
//!
//! - Le swap futur vers un contexte WGPU pour l'overlay UI
//! - L'ajout d'un fallback software (`SoftwareRenderingContext`)
//! - Les tests headless avec un contexte offscreen

use std::rc::Rc;

use servo::{RenderingContext, WindowRenderingContext};
use winit::dpi::PhysicalSize;
use winit::raw_window_handle::{DisplayHandle, WindowHandle};

/// Crée un contexte de rendu hardware-acceleré lié à la fenêtre Winit.
///
/// Utilise surfman sous le capot pour établir un contexte OpenGL natif.
/// Sur Windows, cela utilise WGL par défaut (ou ANGLE si le feature
/// `no-wgl` est activé sur libservo).
///
/// Le contexte est rendu courant (`make_current`) avant d'être retourné,
/// ce qui est requis avant de le passer à `WebViewBuilder`.
///
/// # Arguments
///
/// * `display_handle` — Handle de display plateforme (depuis `ActiveEventLoop`)
/// * `window_handle` — Handle de fenêtre (depuis la `Window` Winit)
/// * `size` — Taille initiale de la fenêtre en pixels physiques
///
/// # Panics
///
/// Panic si le contexte OpenGL ne peut pas être créé (pas de driver compatible,
/// handles invalides, etc.). C'est un échec fatal — pas de navigateur sans GPU.
pub fn create_rendering_context(
    display_handle: DisplayHandle<'_>,
    window_handle: WindowHandle<'_>,
    size: PhysicalSize<u32>,
) -> Rc<WindowRenderingContext> {
    let rendering_context = WindowRenderingContext::new(display_handle, window_handle, size)
        .expect("Impossible de créer le WindowRenderingContext — vérifiez vos drivers GPU");

    rendering_context
        .make_current()
        .expect("Impossible de rendre le contexte OpenGL courant");

    Rc::new(rendering_context)
}
