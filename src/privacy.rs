//! Middleware de filtrage réseau (ad-blocking, tracker blocking).
//!
//! Encapsule le moteur `adblock` de Brave pour filtrer les requêtes HTTP
//! en utilisant des listes de filtres au format Adblock Plus (EasyList,
//! EasyPrivacy, etc.).
//!
//! ## Utilisation
//!
//! 1. Placer les fichiers de filtres (`.txt`) dans `resources/filters/`
//! 2. `AdblockEngine::new()` les charge automatiquement au démarrage
//! 3. Si le dossier est vide ou absent, le filtrage est désactivé
//!
//! ## Listes de filtres recommandées
//!
//! - EasyList : <https://easylist.to/easylist/easylist.txt>
//! - EasyPrivacy : <https://easylist.to/easylist/easyprivacy.txt>

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use adblock::Engine;
use adblock::lists::{FilterSet, ParseOptions};
use tracing::{info, warn};

/// Wrapper autour du moteur `adblock::Engine`.
///
/// Le moteur est construit à partir de listes de filtres au format ABP
/// trouvées dans `resources/filters/`. Les vérifications se font via
/// `should_block()` qui prend l'URL, l'URL source, et le type de requête.
pub struct AdblockEngine {
    engine: Engine,
    /// Cache of (url, source_url) → blocked? to avoid redundant filter matching.
    /// Cleared on navigation via `clear_cache()`.
    cache: RefCell<HashMap<(String, String), bool>>,
}

impl AdblockEngine {
    /// Charge les listes de filtres depuis `resources/filters/` et construit le moteur.
    ///
    /// Retourne `None` si aucun fichier de filtres n'est trouvé (le navigateur
    /// fonctionnera sans ad-blocking).
    pub fn new() -> Option<Self> {
        let filters_dir = find_filters_dir()?;

        let entries: Vec<_> = fs::read_dir(&filters_dir)
            .ok()?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "txt"))
            .collect();

        if entries.is_empty() {
            warn!(
                "Dossier filters/ trouvé mais vide ({}). Ad-blocking désactivé.",
                filters_dir.display()
            );
            return None;
        }

        let mut filter_set = FilterSet::new(false);

        for entry in &entries {
            let path = entry.path();
            match fs::read_to_string(&path) {
                Ok(content) => {
                    let line_count = content.lines().count();
                    filter_set.add_filter_list(&content, ParseOptions::default());
                    info!(
                        "Liste de filtres chargée : {} ({} lignes)",
                        path.display(),
                        line_count
                    );
                }
                Err(e) => {
                    warn!("Impossible de lire {} : {}", path.display(), e);
                }
            }
        }

        let engine = Engine::from_filter_set(filter_set, true);
        info!("Moteur adblock initialisé avec {} liste(s)", entries.len());

        Some(Self {
            engine,
            cache: RefCell::new(HashMap::new()),
        })
    }

    /// Vérifie si une requête doit être bloquée.
    ///
    /// - `url` : URL de la ressource demandée
    /// - `source_url` : URL de la page qui a initié la requête
    /// - `request_type` : type de ressource ("document", "script", "image", "stylesheet", "other")
    pub fn should_block(&self, url: &str, source_url: &str, request_type: &str) -> bool {
        let key = (url.to_owned(), source_url.to_owned());
        if let Some(&cached) = self.cache.borrow().get(&key) {
            return cached;
        }

        let request = match adblock::request::Request::new(url, source_url, request_type)
            .or_else(|_| adblock::request::Request::new(url, "", "other"))
        {
            Ok(r) => r,
            Err(_) => {
                // URL unparseable by adblock (data URI, blob, etc.) — allow it.
                self.cache.borrow_mut().insert(key, false);
                return false;
            }
        };
        let blocked = self.engine.check_network_request(&request).matched;
        self.cache.borrow_mut().insert(key, blocked);
        blocked
    }

    /// Clears the result cache. Call on navigation to avoid unbounded growth.
    pub fn clear_cache(&self) {
        self.cache.borrow_mut().clear();
    }
}

/// Cherche le dossier `resources/filters/` selon la même logique que `resources.rs`.
fn find_filters_dir() -> Option<PathBuf> {
    // 1. Variable d'environnement
    if let Ok(path) = std::env::var("SERVO_RESOURCES_PATH") {
        let mut path = PathBuf::from(path);
        path.push("filters");
        if path.is_dir() {
            return Some(path);
        }
    }

    // 2. À côté de l'exécutable
    if let Ok(exe_path) = std::env::current_exe()
        && let Ok(canonical) = exe_path.canonicalize()
    {
        let mut path = canonical.clone();
        path.pop();
        path.push("resources");
        path.push("filters");
        if path.is_dir() {
            return Some(path);
        }

        // 2b. Si l'exécutable est dans target/{debug,release}/, remonter au projet root.
        let exe_dir = canonical.parent().unwrap_or(&canonical);
        if let Some(target_dir) = exe_dir.parent()
            && target_dir.file_name().is_some_and(|n| n == "target")
            && let Some(project_root) = target_dir.parent()
        {
            let mut path = project_root.to_path_buf();
            path.push("resources");
            path.push("filters");
            if path.is_dir() {
                return Some(path);
            }
        }
    }

    // 3. Répertoire courant
    if let Ok(cwd) = std::env::current_dir() {
        let mut path = cwd;
        path.push("resources");
        path.push("filters");
        if path.is_dir() {
            return Some(path);
        }
    }

    warn!("Dossier resources/filters/ introuvable. Ad-blocking désactivé.");
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds an AdblockEngine from raw filter rules (bypasses filesystem).
    fn engine_from_rules(rules: &[&str]) -> AdblockEngine {
        let mut filter_set = FilterSet::new(false);
        for rule in rules {
            filter_set.add_filter_list(rule, ParseOptions::default());
        }
        AdblockEngine {
            engine: Engine::from_filter_set(filter_set, true),
            cache: RefCell::new(HashMap::new()),
        }
    }

    #[test]
    fn test_should_block_ad_url() {
        let engine = engine_from_rules(&["||ads.example.com^"]);
        assert!(engine.should_block(
            "https://ads.example.com/banner.js",
            "https://example.com",
            "script"
        ));
    }

    #[test]
    fn test_should_not_block_clean_url() {
        let engine = engine_from_rules(&["||ads.example.com^"]);
        assert!(!engine.should_block(
            "https://example.com/page.html",
            "https://example.com",
            "document"
        ));
    }

    #[test]
    fn test_cache_returns_same_result() {
        let engine = engine_from_rules(&["||ads.example.com^"]);
        let first = engine.should_block(
            "https://ads.example.com/banner.js",
            "https://example.com",
            "script",
        );
        let second = engine.should_block(
            "https://ads.example.com/banner.js",
            "https://example.com",
            "script",
        );
        assert_eq!(first, second);
        assert_eq!(engine.cache.borrow().len(), 1);
    }

    #[test]
    fn test_clear_cache_empties_cache() {
        let engine = engine_from_rules(&["||ads.example.com^"]);
        engine.should_block(
            "https://ads.example.com/banner.js",
            "https://example.com",
            "script",
        );
        assert!(!engine.cache.borrow().is_empty());
        engine.clear_cache();
        assert!(engine.cache.borrow().is_empty());
    }

    #[test]
    fn test_malformed_url_not_blocked() {
        let engine = engine_from_rules(&["||ads.example.com^"]);
        assert!(!engine.should_block("not-a-valid-url-at-all", "https://example.com", "other"));
    }

    #[test]
    fn test_data_uri_not_blocked() {
        let engine = engine_from_rules(&["||ads.example.com^"]);
        assert!(!engine.should_block(
            "data:text/html,<h1>hi</h1>",
            "https://example.com",
            "document"
        ));
    }

    #[test]
    fn test_empty_filter_set_blocks_nothing() {
        let engine = engine_from_rules(&[]);
        assert!(!engine.should_block(
            "https://ads.example.com/banner.js",
            "https://example.com",
            "script"
        ));
    }

    #[test]
    fn test_multiple_filters() {
        let engine = engine_from_rules(&["||ads.example.com^\n||tracker.example.com^"]);
        assert!(engine.should_block(
            "https://ads.example.com/x.js",
            "https://example.com",
            "script"
        ));
        assert!(engine.should_block(
            "https://tracker.example.com/t.gif",
            "https://example.com",
            "image"
        ));
        assert!(!engine.should_block(
            "https://example.com/page.html",
            "https://example.com",
            "document"
        ));
    }

    #[test]
    fn test_different_source_urls_cached_separately() {
        let engine = engine_from_rules(&["||ads.example.com^"]);
        engine.should_block(
            "https://ads.example.com/x.js",
            "https://site-a.com",
            "script",
        );
        engine.should_block(
            "https://ads.example.com/x.js",
            "https://site-b.com",
            "script",
        );
        assert_eq!(engine.cache.borrow().len(), 2);
    }

    #[test]
    fn test_new_returns_some_when_filters_exist() {
        // This test requires running from project root where resources/filters/ exists
        if std::path::Path::new("resources/filters").is_dir() {
            let engine = AdblockEngine::new();
            assert!(engine.is_some());
        }
    }

    #[test]
    fn test_cache_hit_no_panic() {
        let engine = engine_from_rules(&["||ads.example.com^"]);
        // Call many times to exercise cache path
        for _ in 0..100 {
            engine.should_block(
                "https://ads.example.com/x.js",
                "https://example.com",
                "script",
            );
        }
        assert_eq!(engine.cache.borrow().len(), 1);
    }
}
