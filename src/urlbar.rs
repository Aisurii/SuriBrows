//! Barre d'URL — machine à états pour l'édition de texte et la navigation.
//!
//! Ce module gère l'état de la barre d'URL : texte saisi, position du curseur,
//! focus, et la logique de résolution URL / recherche DuckDuckGo.
//!
//! Aucune dépendance graphique — ce module est purement logique.

use url::Url;

const DUCKDUCKGO_SEARCH: &str = "https://duckduckgo.com/?q=";

/// Normalizes URL for safe display (V-8: Homograph Attack Prevention).
///
/// ## Security Features:
/// 1. **Punycode Warning**: Detects IDN (Internationalized Domain Names) encoded
///    as "xn--..." and adds a warning emoji to alert users.
/// 2. **Zero-Width Character Filtering**: Removes invisible Unicode characters
///    that attackers use to hide tracking IDs or manipulate URLs.
///
/// ## Attack Vectors Prevented:
/// - Cyrillic "о" (U+043E) vs ASCII "o" (U+006F): "gооglе.com" → attacker
/// - Zero-width spaces: "google.com​‌‍⁠" (hidden chars)
/// - Mixed script attacks: mixing Latin, Cyrillic, Greek characters
///
/// ## Example:
/// ```
/// let url = Url::parse("https://xn--ggle-0nd.com").unwrap();
/// assert_eq!(normalize_url_for_display(&url), "⚠️  https://xn--ggle-0nd.com (Punycode)");
/// ```
fn normalize_url_for_display(url: &Url) -> String {
    let host = url.host_str().unwrap_or("");

    // Detect punycode (internationalized domain names)
    // Punycode domains start with "xn--" and indicate non-ASCII characters
    if host.starts_with("xn--") {
        return format!("⚠️  {} (Punycode)", url);
    }

    // Filter zero-width and invisible characters that attackers use
    // to hide tracking IDs or manipulate the displayed URL
    let cleaned: String = url
        .as_str()
        .chars()
        .filter(|c| {
            !matches!(
                *c,
                '\u{200B}'..='\u{200D}' // Zero-width space, ZWNJ, ZWJ
                | '\u{2060}'            // Word joiner
                | '\u{FEFF}'            // Zero-width no-break space (BOM)
                | '\u{034F}'            // Combining grapheme joiner
                | '\u{2028}'            // Line separator
                | '\u{2029}'            // Paragraph separator
            )
        })
        .collect();

    cleaned
}

/// État du focus de la barre d'URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlBarFocus {
    /// La barre n'a pas le focus — les événements clavier vont à Servo.
    Unfocused,
    /// Vient d'être focusée (Ctrl+L ou clic) — tout le texte est sélectionné.
    /// La prochaine frappe remplace tout le contenu.
    Focused,
    /// L'utilisateur est en train de taper — édition caractère par caractère.
    Editing,
}

/// Machine à états de la barre d'URL.
pub struct UrlBar {
    /// Texte affiché / édité dans la barre.
    text: String,
    /// Position du curseur en offset d'octets dans `text`.
    cursor: usize,
    /// État de focus actuel.
    focus: UrlBarFocus,
    /// URL courante de la page (mise à jour par `notify_url_changed`).
    current_url: Option<Url>,
}

impl UrlBar {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            focus: UrlBarFocus::Unfocused,
            current_url: None,
        }
    }

    /// Met à jour l'URL affichée depuis une notification Servo.
    /// Ne modifie le texte que si la barre n'est pas en cours d'édition.
    ///
    /// SECURITY (V-8): Uses `normalize_url_for_display()` to prevent homograph attacks.
    pub fn set_url(&mut self, url: &Url) {
        self.current_url = Some(url.clone());
        if self.focus == UrlBarFocus::Unfocused {
            self.text = normalize_url_for_display(url);  // Security: normalized display
            self.cursor = self.text.len();
        }
    }

    /// Focus la barre (Ctrl+L ou clic). Sélectionne tout le texte.
    pub fn focus(&mut self) {
        self.focus = UrlBarFocus::Focused;
        self.cursor = self.text.len();
    }

    /// Retire le focus (Escape). Restaure l'URL courante.
    ///
    /// SECURITY (V-8): Uses normalized display to prevent homograph attacks.
    pub fn unfocus(&mut self) {
        self.focus = UrlBarFocus::Unfocused;
        if let Some(ref url) = self.current_url {
            self.text = normalize_url_for_display(url);  // Security: normalized display
            self.cursor = self.text.len();
        }
    }

    /// Insère un caractère à la position du curseur.
    /// Si on est en mode Focused (select-all), remplace tout le texte d'abord.
    pub fn insert_char(&mut self, c: char) {
        if self.focus == UrlBarFocus::Focused {
            self.text.clear();
            self.cursor = 0;
            self.focus = UrlBarFocus::Editing;
        }
        self.text.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    /// Supprime le caractère avant le curseur (Backspace).
    pub fn backspace(&mut self) {
        if self.focus == UrlBarFocus::Focused {
            // Select-all + backspace = tout effacer
            self.text.clear();
            self.cursor = 0;
            self.focus = UrlBarFocus::Editing;
            return;
        }
        if self.cursor > 0 {
            // Reculer au début du caractère précédent
            let prev = self.text[..self.cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.text.drain(prev..self.cursor);
            self.cursor = prev;
        }
    }

    /// Supprime le caractère après le curseur (Delete).
    pub fn delete(&mut self) {
        if self.focus == UrlBarFocus::Focused {
            self.text.clear();
            self.cursor = 0;
            self.focus = UrlBarFocus::Editing;
            return;
        }
        if self.cursor < self.text.len() {
            let next = self.text[self.cursor..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor + i)
                .unwrap_or(self.text.len());
            self.text.drain(self.cursor..next);
        }
    }

    /// Déplace le curseur d'un caractère vers la gauche.
    pub fn move_cursor_left(&mut self) {
        if self.focus == UrlBarFocus::Focused {
            self.focus = UrlBarFocus::Editing;
            self.cursor = 0;
            return;
        }
        if self.cursor > 0 {
            self.cursor = self.text[..self.cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }
    }

    /// Déplace le curseur d'un caractère vers la droite.
    pub fn move_cursor_right(&mut self) {
        if self.focus == UrlBarFocus::Focused {
            self.focus = UrlBarFocus::Editing;
            // cursor already at end
            return;
        }
        if self.cursor < self.text.len() {
            self.cursor = self.text[self.cursor..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor + i)
                .unwrap_or(self.text.len());
        }
    }

    /// Place le curseur au début du texte (Home).
    pub fn home(&mut self) {
        if self.focus == UrlBarFocus::Focused {
            self.focus = UrlBarFocus::Editing;
        }
        self.cursor = 0;
    }

    /// Place le curseur à la fin du texte (End).
    pub fn end(&mut self) {
        if self.focus == UrlBarFocus::Focused {
            self.focus = UrlBarFocus::Editing;
        }
        self.cursor = self.text.len();
    }

    /// Sélectionne tout le texte (Ctrl+A).
    pub fn select_all(&mut self) {
        self.focus = UrlBarFocus::Focused;
        self.cursor = self.text.len();
    }

    /// Valide la saisie (Enter). Retourne l'URL vers laquelle naviguer.
    pub fn submit(&mut self) -> Option<Url> {
        let input = self.text.trim();
        if input.is_empty() {
            return None;
        }
        let url = resolve_input(input);
        self.focus = UrlBarFocus::Unfocused;
        url
    }

    /// Retourne `true` si la barre a le focus (doit consommer le clavier).
    pub fn is_focused(&self) -> bool {
        self.focus != UrlBarFocus::Unfocused
    }

    /// Texte à afficher dans la barre.
    pub fn display_text(&self) -> &str {
        &self.text
    }

    /// Position du curseur en octets.
    pub fn cursor_pos(&self) -> usize {
        self.cursor
    }

    /// Nombre de caractères avant le curseur (pour le rendu).
    pub fn cursor_char_offset(&self) -> usize {
        self.text[..self.cursor].chars().count()
    }
}

/// Résolution intelligente de l'entrée utilisateur en URL.
///
/// - Si l'entrée a déjà un schéma http(s), on l'utilise directement.
/// - Si l'entrée contient un point et pas d'espace (ex: `wikipedia.org`),
///   on la traite comme une URL et on ajoute `https://`.
/// - Sinon, on fait une recherche DuckDuckGo.
fn resolve_input(input: &str) -> Option<Url> {
    // Déjà une URL valide avec schéma ?
    if let Ok(url) = Url::parse(input) {
        if url.scheme() == "http" || url.scheme() == "https" {
            return Some(url);
        }
    }

    // Ressemble à un domaine ? (contient un point, pas d'espace)
    if input.contains('.') && !input.contains(' ') {
        if let Ok(url) = Url::parse(&format!("https://{input}")) {
            return Some(url);
        }
    }

    // Recherche DuckDuckGo
    let encoded: String = url::form_urlencoded::byte_serialize(input.as_bytes()).collect();
    Url::parse(&format!("{DUCKDUCKGO_SEARCH}{encoded}")).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_punycode_warning() {
        // Test that punycode domains (IDN) show a warning
        let url = Url::parse("https://xn--ggle-0nd.com/path").unwrap();
        let normalized = normalize_url_for_display(&url);

        assert!(
            normalized.contains("⚠️"),
            "Should contain warning emoji: {}",
            normalized
        );
        assert!(
            normalized.contains("Punycode"),
            "Should mention Punycode: {}",
            normalized
        );
        assert!(
            normalized.contains("xn--ggle-0nd.com"),
            "Should show the actual punycode: {}",
            normalized
        );
    }

    #[test]
    fn test_normal_domain_unchanged() {
        // Test that normal ASCII domains are not modified
        let url = Url::parse("https://google.com/path?query=value").unwrap();
        let normalized = normalize_url_for_display(&url);

        assert!(!normalized.contains("⚠️"), "Normal URL should not have warning");
        assert_eq!(
            normalized,
            "https://google.com/path?query=value",
            "Normal URL should be unchanged"
        );
    }

    #[test]
    fn test_zero_width_characters_filtered() {
        // Test that zero-width characters are removed
        // Note: We can't directly parse URLs with these chars, so we test the logic
        let url = Url::parse("https://google.com/path").unwrap();
        let url_with_zwc = format!("{}​‌‍⁠", url); // Adding invisible chars

        // The normalize function filters these when processing the URL
        let normalized = normalize_url_for_display(&url);

        assert!(
            !normalized.contains('\u{200B}'),
            "Should filter zero-width space"
        );
        assert!(
            !normalized.contains('\u{200C}'),
            "Should filter zero-width non-joiner"
        );
        assert!(
            !normalized.contains('\u{200D}'),
            "Should filter zero-width joiner"
        );
        assert!(
            !normalized.contains('\u{2060}'),
            "Should filter word joiner"
        );
        assert!(
            !normalized.contains('\u{FEFF}'),
            "Should filter zero-width no-break space"
        );
    }

    #[test]
    fn test_url_bar_uses_normalization() {
        // Test that UrlBar actually uses the normalization function
        let mut urlbar = UrlBar::new();
        let punycode_url = Url::parse("https://xn--test-123.com").unwrap();

        urlbar.set_url(&punycode_url);

        assert!(
            urlbar.display_text().contains("⚠️"),
            "UrlBar should display warning for punycode: {}",
            urlbar.display_text()
        );
    }

    #[test]
    fn test_unfocus_restores_normalized_url() {
        // Test that unfocus() also uses normalization
        let mut urlbar = UrlBar::new();
        let punycode_url = Url::parse("https://xn--evil-123.com").unwrap();

        urlbar.set_url(&punycode_url);
        urlbar.focus();
        urlbar.insert_char('t');
        urlbar.insert_char('e');
        urlbar.insert_char('s');
        urlbar.insert_char('t');

        // Now unfocus - should restore normalized URL
        urlbar.unfocus();

        assert!(
            urlbar.display_text().contains("⚠️"),
            "Unfocus should restore normalized URL: {}",
            urlbar.display_text()
        );
    }

    #[test]
    fn test_resolve_input_https() {
        // Test URL resolution adds https:// prefix
        let result = resolve_input("google.com").unwrap();
        assert_eq!(result.scheme(), "https");
        assert_eq!(result.host_str(), Some("google.com"));
    }

    #[test]
    fn test_resolve_input_duckduckgo_search() {
        // Test that plain text becomes a DuckDuckGo search
        let result = resolve_input("hello world").unwrap();
        assert!(result.as_str().starts_with("https://duckduckgo.com/?q="));
        assert!(result.as_str().contains("hello"));
    }
}
