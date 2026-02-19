//! # SuriBrows — Navigateur Privacy-First
//!
//! Navigateur web expérimental ultra-léger construit sur le moteur Servo,
//! avec une architecture orientée privacy-by-design.
//!
//! ## Architecture des modules
//!
//! - [`browser`] : Boucle d'événements Winit et gestion du cycle de vie de la fenêtre.
//!   Implémente le pattern "Two-Phase App" imposé par winit 0.30.
//!
//! - [`servo_glue`] : Couche d'intégration Servo — pont entre les threads internes
//!   de Servo (Constellation, script, layout) et le thread principal Winit via
//!   le `Waker`. Contient aussi les implémentations des delegates.
//!
//! - [`rendering`] : Factory pour le contexte de rendu GPU (WindowRenderingContext).
//!   Isole le setup OpenGL/surfman pour faciliter un futur swap vers WGPU.
//!
//! - [`config`] : Système de configuration TOML — charge les paramètres depuis
//!   un fichier `config.toml` avec fallback sur les valeurs par défaut.
//!
//! - [`keyutils`] : Conversion des événements clavier Winit vers les types Servo.
//!
//! - [`preferences`] : Configuration du moteur Servo — performance tuning et
//!   paramètres privacy/sécurité (TLS, fingerprinting, WebRTC, etc.).
//!
//! - [`privacy`] : Middleware d'interception réseau — ad-blocking et tracker blocking
//!   via le crate `adblock` (Brave). Intégré dans `WebViewDelegate::load_web_resource()`.
//!
//! - [`settings`] : Page de paramètres HTML — génère un formulaire rendu par
//!   Servo via `data:` URLs avec interception du save via `load_web_resource`.
//!
//! - [`security`] : Durcissement de sécurité Windows — applique des politiques de
//!   mitigation de processus (ACG, Image Load Policy, Job Object) pour bloquer les
//!   exploits communs. Optionnel sur Windows, no-op sur Linux/macOS.
//!
//! ## Modules futurs (non implémentés)
//!
//! - `ui` : Overlay GPU pour le chrome du navigateur (barre d'URL, onglets)
//! - `plugins` : Hôte WebAssembly pour extensions natives (wasmtime)

pub mod browser;
pub mod chrome;
pub mod config;
pub mod keyutils;
pub mod preferences;
pub mod privacy;
pub mod rendering;
pub mod resources;
pub mod security;
pub mod servo_glue;
pub mod settings;
pub mod urlbar;
