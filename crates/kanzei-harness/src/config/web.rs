//! Web/native-search settings (R-365 B2b).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct WebSection {
    /// Enable native hosted search for channels that passed B1. None defaults to true.
    #[serde(default)]
    pub native_search: Option<bool>,
    /// Profiles where hosted search is always disabled; default is research until evidence gates land.
    #[serde(default)]
    pub native_search_off_profiles: Option<Vec<String>>,
    /// Anthropic server-search tool version; consumed only after the Claude probe passes.
    #[serde(default)]
    pub anthropic_search_tool: Option<String>,
    /// auto | duckduckgo | codex; invalid values fail back to auto.
    #[serde(default)]
    pub search_backend: Option<String>,
}

pub(crate) const WEB_KEYS: &[&str] = &[
    "native_search",
    "native_search_off_profiles",
    "anthropic_search_tool",
    "search_backend",
];

impl WebSection {
    pub fn native_search_enabled(&self) -> bool {
        self.native_search.unwrap_or(true)
    }

    pub fn search_backend(&self) -> &str {
        match self.search_backend.as_deref().map(str::trim) {
            Some("codex") => "codex",
            Some("duckduckgo") => "duckduckgo",
            Some("auto") | None | Some("") => "auto",
            Some(_) => "auto",
        }
    }

    pub fn native_search_disabled_for(&self, profile: &str) -> bool {
        match self.native_search_off_profiles.as_deref() {
            Some(disabled) => disabled
                .iter()
                .any(|name| name.eq_ignore_ascii_case(profile)),
            None => profile.eq_ignore_ascii_case("research"),
        }
    }
}
