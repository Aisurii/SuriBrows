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
    if let Ok(exe_path) = std::env::current_exe() {
        if let Ok(canonical) = exe_path.canonicalize() {
            let mut path = canonical.clone();
            path.pop();
            path.push("resources");
            path.push("filters");
            if path.is_dir() {
                return Some(path);
            }

            // 2b. Si l'exécutable est dans target/{debug,release}/, remonter au projet root.
            let exe_dir = canonical.parent().unwrap_or(&canonical);
            if let Some(target_dir) = exe_dir.parent() {
                if target_dir.file_name().is_some_and(|n| n == "target") {
                    if let Some(project_root) = target_dir.parent() {
                        let mut path = project_root.to_path_buf();
                        path.push("resources");
                        path.push("filters");
                        if path.is_dir() {
                            return Some(path);
                        }
                    }
                }
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
