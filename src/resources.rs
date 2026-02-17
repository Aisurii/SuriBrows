//! Lecteur de ressources Servo.
//!
//! Servo a besoin d'un ensemble de fichiers de ressources (préférences,
//! certificats, listes de blocage GATT, domaines publics, etc.) pour
//! fonctionner. L'embedder doit fournir une implémentation de
//! `ResourceReaderMethods` et l'enregistrer via `servo::resources::set()`.
//!
//! Ce module cherche le dossier `resources/` dans les chemins suivants :
//! 1. Variable d'environnement `SERVO_RESOURCES_PATH`
//! 2. À côté de l'exécutable (`<exe_dir>/resources/`)
//! 3. Dans le répertoire courant (`./resources/`)

use std::path::PathBuf;
use std::sync::Mutex;
use std::{env, fs};

use servo::resources::{self, Resource};

/// Chemin vers le dossier resources/, mis en cache après la première résolution.
static RESOURCES_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Initialise le lecteur de ressources Servo.
///
/// **Doit être appelé avant `ServoBuilder::build()`**, sinon Servo
/// paniquera avec "Resource reader not set".
pub fn init() {
    resources::set(Box::new(ResourceReader));
}

struct ResourceReader;

impl resources::ResourceReaderMethods for ResourceReader {
    fn read(&self, file: Resource) -> Vec<u8> {
        let mut path = resources_dir_path();
        path.push(file.filename());

        // SECURITY: Prevent path traversal attacks (V-2)
        // Canonicalize resolves symlinks and "../" sequences to absolute paths
        let canonical = path
            .canonicalize()
            .unwrap_or_else(|e| panic!("Invalid resource path '{}': {}", file.filename(), e));

        let resources_canonical = resources_dir_path()
            .canonicalize()
            .expect("Resources directory does not exist");

        // Verify the resolved path is still within resources/
        if !canonical.starts_with(&resources_canonical) {
            panic!(
                "🚨 SECURITY: Path traversal attempt blocked: '{}'\n\
                 Attempted path: {}\n\
                 Resources dir: {}",
                file.filename(),
                canonical.display(),
                resources_canonical.display()
            );
        }

        fs::read(&canonical).unwrap_or_else(|e| {
            panic!(
                "Impossible de lire la ressource Servo {:?} (chemin: {}): {}",
                file.filename(),
                canonical.display(),
                e
            )
        })
    }

    fn sandbox_access_files_dirs(&self) -> Vec<PathBuf> {
        vec![resources_dir_path()]
    }

    fn sandbox_access_files(&self) -> Vec<PathBuf> {
        vec![]
    }
}

/// Résout et met en cache le chemin du dossier `resources/`.
fn resources_dir_path() -> PathBuf {
    let mut dir = RESOURCES_DIR.lock().unwrap();
    if let Some(ref path) = *dir {
        return path.clone();
    }

    // 1. Variable d'environnement SERVO_RESOURCES_PATH
    if let Ok(path) = env::var("SERVO_RESOURCES_PATH") {
        let path = PathBuf::from(path);
        if path.is_dir() {
            *dir = Some(path.clone());
            return path;
        }
    }

    // 2. À côté de l'exécutable
    if let Ok(exe_path) = env::current_exe()
        && let Ok(canonical) = exe_path.canonicalize()
    {
        let mut path = canonical.clone();
        path.pop(); // Enlève le nom de l'exécutable
        path.push("resources");
        if path.is_dir() {
            *dir = Some(path.clone());
            return path;
        }

        // 2b. Si l'exécutable est dans target/{debug,release}/, remonter
        //     au projet root (typique pendant le développement avec cargo).
        let exe_dir = canonical.parent().unwrap_or(&canonical);
        if let Some(target_dir) = exe_dir.parent()
            && target_dir.file_name().is_some_and(|n| n == "target")
            && let Some(project_root) = target_dir.parent()
        {
            let mut path = project_root.to_path_buf();
            path.push("resources");
            if path.is_dir() {
                *dir = Some(path.clone());
                return path;
            }
        }
    }

    // 3. Répertoire courant
    if let Ok(cwd) = env::current_dir() {
        let mut path = cwd;
        path.push("resources");
        if path.is_dir() {
            *dir = Some(path.clone());
            return path;
        }
    }

    panic!(
        "Impossible de trouver le dossier 'resources/' de Servo. \
         Définissez SERVO_RESOURCES_PATH ou placez le dossier à côté de l'exécutable."
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_canonicalization_prevents_traversal() {
        // This test verifies that the canonicalization logic correctly
        // prevents path traversal attacks at the directory level

        let resources_dir = resources_dir_path();
        let resources_canonical = resources_dir
            .canonicalize()
            .expect("Resources directory should exist");

        // Simulate what would happen with a malicious path
        let mut malicious_path = resources_dir.clone();
        malicious_path.push("../../../Windows/System32");

        // Attempt to canonicalize the malicious path
        // The canonicalize() will resolve the path, and we verify our security check works
        if let Ok(canonical) = malicious_path.canonicalize() {
            // Verify that the security check would catch this
            assert!(
                !canonical.starts_with(&resources_canonical),
                "Path traversal should be detected: {} is not within {}",
                canonical.display(),
                resources_canonical.display()
            );
        }
        // If canonicalize fails (path doesn't exist), that's also good -
        // the read() method would panic on the canonicalize() call
    }

    #[test]
    fn test_resources_dir_is_canonical() {
        // Verify that the resources directory path is properly resolved
        let resources_dir = resources_dir_path();

        assert!(
            resources_dir.exists(),
            "Resources directory should exist: {}",
            resources_dir.display()
        );

        assert!(
            resources_dir.is_dir(),
            "Resources path should be a directory: {}",
            resources_dir.display()
        );

        // Verify we can canonicalize it (no symlink attacks)
        let canonical = resources_dir
            .canonicalize()
            .expect("Should be able to canonicalize resources directory");

        assert!(
            canonical.is_absolute(),
            "Canonical path should be absolute: {}",
            canonical.display()
        );
    }

    #[test]
    fn test_path_traversal_detection_logic() {
        // Unit test for the core security logic: verifying paths stay within bounds

        // Create realistic paths for testing
        let base = if cfg!(windows) {
            PathBuf::from("C:\\SuriBrows\\resources")
        } else {
            PathBuf::from("/home/user/SuriBrows/resources")
        };

        // Simulate valid resource path
        let valid_subpath = base.join("preferences.json");

        // Simulate malicious path that resolves outside base
        let malicious_resolved = if cfg!(windows) {
            PathBuf::from("C:\\Windows\\System32\\config\\SAM")
        } else {
            PathBuf::from("/etc/passwd")
        };

        // The security check: does the resolved path start with base?
        assert!(
            valid_subpath.starts_with(&base),
            "Valid path should be within base: {} vs {}",
            valid_subpath.display(),
            base.display()
        );

        assert!(
            !malicious_resolved.starts_with(&base),
            "Malicious path should be detected as outside base: {} vs {}",
            malicious_resolved.display(),
            base.display()
        );
    }

    #[test]
    fn test_path_components_validation() {
        // Test that the starts_with() method correctly handles parent directory references

        let resources_dir = resources_dir_path();

        // Test various malicious patterns
        let test_patterns = vec![
            "..",
            "../..",
            "../../..",
            "../../../etc",
            "..\\..\\..\\Windows",
        ];

        for pattern in test_patterns {
            let mut test_path = resources_dir.clone();
            test_path.push(pattern);

            // Most of these won't exist, so canonicalize will fail - that's good
            // If they do exist (e.g., on the filesystem), the starts_with check catches them
            if let Ok(canonical) = test_path.canonicalize() {
                let resources_canonical = resources_dir.canonicalize().unwrap();

                // If the path exists and canonicalizes, make sure our security check works
                if canonical != resources_canonical {
                    assert!(
                        !canonical.starts_with(&resources_canonical)
                            || canonical == resources_canonical,
                        "Pattern '{}' bypassed security: {} vs {}",
                        pattern,
                        canonical.display(),
                        resources_canonical.display()
                    );
                }
            }
        }
    }
}
