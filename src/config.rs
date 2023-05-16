use serde::Deserialize;

#[derive(Clone, Deserialize)]
pub struct Config {
    pub backend_url: String,
    pub flat_manager_url: String,
    pub flat_manager_token: String,
    #[serde(default)]
    pub validation_observe_only: bool,
}
