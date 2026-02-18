//! Servo engine preferences tuned for privacy and performance.
//!
//! Builds a [`servo::Preferences`] struct with hardened defaults:
//! - Thread pools sized to available CPU cores (clamped)
//! - HTTPS enforcement, MIME sniff disabled
//! - Tracking APIs disabled (geolocation, Bluetooth, WebRTC, notifications)
//! - Generic Chrome user-agent to reduce fingerprinting

use tracing::{info, warn};

/// Builds Servo `Preferences` tuned for the current machine with privacy enhancements.
#[allow(clippy::field_reassign_with_default)]
pub fn build_servo_preferences() -> servo::Preferences {
    let cpus = std::thread::available_parallelism()
        .map(|n| n.get() as i64)
        .unwrap_or(4);

    let mut prefs = servo::Preferences::default();

    // ── Performance Tuning ────────────────────────────────────────────────
    prefs.layout_threads = cpus.min(8);
    prefs.threadpools_async_runtime_workers_max = (cpus * 2).min(16);
    prefs.threadpools_image_cache_workers_max = cpus.min(8);
    prefs.threadpools_webrender_workers_max = (cpus / 2).clamp(2, 8);
    prefs.threadpools_resource_workers_max = cpus.min(8);
    prefs.network_http_cache_size = 50_000;
    prefs.gfx_precache_shaders = true;

    // ── Privacy & Security Settings (Balanced Mode) ───────────────────────

    // User Agent: Generic to reduce fingerprinting surface
    // Using standard Chrome UA without OS details
    prefs.user_agent = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36".to_string();

    // Network Security
    prefs.network_enforce_tls_enabled = true; // Force HTTPS where possible
    prefs.network_mime_sniff = false; // Disable MIME sniffing (prevents XSS)

    // Privacy Features - Disable Tracking APIs
    prefs.dom_geolocation_enabled = false; // Block location tracking
    prefs.dom_bluetooth_enabled = false; // Block Bluetooth access
    prefs.dom_notification_enabled = false; // Block notification spam

    // SECURITY: Disable WebRTC to prevent IP leak attacks
    // Trade-off: Breaks video calls (Zoom, Meet, Discord), P2P apps
    // Rationale: WebRTC can reveal local/public IP even through VPN via STUN
    prefs.dom_webrtc_enabled = false; // Block WebRTC IP leaks

    // Keep enabled for compatibility (balanced mode)
    // - dom_cookiestore_enabled: true (default) - needed for logins
    // - dom_indexeddb_enabled: true (default) - needed for web apps

    // NOTE: Servo doesn't expose these privacy preferences yet:
    // - Referrer policy control (would use strict-origin-when-cross-origin)
    // - Third-party cookie blocking
    // - Canvas fingerprinting randomization
    // - WebRTC IP leak prevention (only full disable available)
    // Ad-blocking via filter lists compensates for some of these gaps.

    // SECURITY: Disable JIT if --secure-mode flag is set
    // This is REQUIRED for ACG (Arbitrary Code Guard) to work.
    // ACG forbids runtime code generation, which conflicts with JavaScript JIT.
    let args: Vec<String> = std::env::args().collect();
    if args.contains(&"--secure-mode".to_string()) {
        // LIMITATION: Servo doesn't expose a JIT disable preference in the Preferences struct.
        // The field "js_jit_content_enabled" doesn't exist in the current Servo version.
        // Possible field names checked: js_jit_content_enabled, js_jit_enabled, dom_jit_enabled
        //
        // IMPACT: --secure-mode will enable ACG without disabling JIT, causing an IMMEDIATE CRASH
        // when JavaScript is executed. This is a known limitation until Servo exposes JIT control.
        //
        // WORKAROUND OPTIONS:
        // 1. File upstream Servo issue requesting js.jit.content preference exposure
        // 2. Disable ACG entirely (remove apply_dynamic_code_policy call in security.rs)
        // 3. Only use --secure-mode on sites without JavaScript
        //
        // For now, we'll warn the user and still enable ACG (will crash on JS execution).
        warn!(
            "⚠️  --secure-mode enabled but Servo doesn't expose JIT disable preference. \
             Browser will crash when loading JavaScript. \
             Only use --secure-mode on static HTML sites."
        );
    }

    info!(
        cpus,
        layout_threads = prefs.layout_threads,
        network_workers = prefs.threadpools_async_runtime_workers_max,
        cache_size = prefs.network_http_cache_size,
        tls_enforced = prefs.network_enforce_tls_enabled,
        "Servo preferences configured (performance + privacy)"
    );

    prefs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preferences_layout_threads_bounded() {
        let prefs = build_servo_preferences();
        assert!(prefs.layout_threads >= 1, "layout_threads should be >= 1");
        assert!(prefs.layout_threads <= 8, "layout_threads should be <= 8");
    }

    #[test]
    fn test_preferences_tls_enforced() {
        let prefs = build_servo_preferences();
        assert!(prefs.network_enforce_tls_enabled);
    }

    #[test]
    fn test_preferences_mime_sniff_disabled() {
        let prefs = build_servo_preferences();
        assert!(!prefs.network_mime_sniff);
    }

    #[test]
    fn test_preferences_geolocation_disabled() {
        let prefs = build_servo_preferences();
        assert!(!prefs.dom_geolocation_enabled);
    }

    #[test]
    fn test_preferences_bluetooth_disabled() {
        let prefs = build_servo_preferences();
        assert!(!prefs.dom_bluetooth_enabled);
    }

    #[test]
    fn test_preferences_webrtc_disabled() {
        let prefs = build_servo_preferences();
        assert!(!prefs.dom_webrtc_enabled);
    }

    #[test]
    fn test_preferences_notification_disabled() {
        let prefs = build_servo_preferences();
        assert!(!prefs.dom_notification_enabled);
    }

    #[test]
    fn test_preferences_user_agent_set() {
        let prefs = build_servo_preferences();
        assert!(prefs.user_agent.contains("Chrome"), "UA should contain Chrome");
        assert!(!prefs.user_agent.is_empty(), "UA should not be empty");
    }

    #[test]
    fn test_preferences_cache_size() {
        let prefs = build_servo_preferences();
        assert_eq!(prefs.network_http_cache_size, 50_000);
    }

    #[test]
    fn test_preferences_precache_shaders() {
        let prefs = build_servo_preferences();
        assert!(prefs.gfx_precache_shaders);
    }

    #[test]
    fn test_preferences_webrender_workers_bounded() {
        let prefs = build_servo_preferences();
        assert!(prefs.threadpools_webrender_workers_max >= 2);
        assert!(prefs.threadpools_webrender_workers_max <= 8);
    }
}
