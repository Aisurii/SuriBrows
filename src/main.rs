//! Point d'entrée de SuriBrows.
//!
//! Usage :
//!   suribrows [URL] [--secure-mode]
//!
//! Exemples :
//!   cargo run                              → charge https://example.com
//!   cargo run -- https://servo.org         → charge servo.org
//!   cargo run -- wikipedia.org             → ajoute https:// automatiquement
//!   cargo run -- --secure-mode             → mode sécurisé (JIT désactivé, ACG activé)

use std::env;
use std::error::Error;

use url::Url;
use winit::event_loop::EventLoop;

/// Page par défaut si aucune URL n'est fournie.
const DEFAULT_URL: &str = "https://example.com";

fn main() -> Result<(), Box<dyn Error>> {
    // ── 0. Parse command-line flags ────────────────────────────────────
    let args: Vec<String> = env::args().collect();
    let secure_mode = args.contains(&"--secure-mode".to_string());

    if secure_mode {
        eprintln!("⚠️  SECURE MODE ENABLED");
        eprintln!("    JavaScript JIT will be disabled (2-5x slower JS execution)");
        eprintln!("    Arbitrary Code Guard (ACG) will be enabled (blocks shellcode)");
        eprintln!();
    }

    // ── 1. Windows Security Hardening (BEFORE any DLLs load) ──────────
    suribrows::security::apply_process_mitigations(secure_mode);

    // ── 2. Provider crypto TLS ─────────────────────────────────────────
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("Échec de l'installation du provider crypto rustls");

    // ── 3. Logging / Tracing ───────────────────────────────────────────
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    #[cfg(debug_assertions)]
    tracing::warn!(
        "Running in DEBUG mode — pages will load very slowly. Use `cargo run --release` for normal speed."
    );

    // ── 4. Lecteur de ressources Servo ─────────────────────────────────
    suribrows::resources::init();

    // ── 5. Parser l'URL depuis les arguments CLI ───────────────────────
    let url = parse_url_from_args();

    // ── 6. Boucle d'événements Winit ───────────────────────────────────
    let event_loop = EventLoop::with_user_event()
        .build()
        .expect("Échec de la création du EventLoop Winit");

    let mut app = suribrows::browser::App::new(&event_loop, url);

    Ok(event_loop.run_app(&mut app)?)
}

/// Parse le premier argument CLI comme URL.
/// Si l'argument ne contient pas de schéma (http/https), on ajoute "https://".
/// Ignore le flag --secure-mode.
fn parse_url_from_args() -> Url {
    // Filter out flags (starting with --) and get first non-flag argument
    let input = env::args()
        .skip(1) // Skip binary name
        .find(|arg| !arg.starts_with("--"))
        .unwrap_or_else(|| DEFAULT_URL.to_string());

    // Essaie de parser directement (fonctionne si l'utilisateur a mis le schéma)
    if let Ok(url) = Url::parse(&input) {
        return url;
    }

    // Sinon, ajoute https:// et réessaie
    Url::parse(&format!("https://{input}"))
        .unwrap_or_else(|e| panic!("URL invalide '{input}': {e}"))
}
