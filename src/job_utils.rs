use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct BuildExtended {
    pub build: Build,
    pub build_refs: Vec<BuildRef>,
}

#[derive(Deserialize)]
pub struct Build {
    pub build_log_url: Option<String>,
}

#[derive(Deserialize)]
pub struct BuildRef {
    pub ref_name: String,
    pub build_log_url: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "status", content = "reason")]
pub enum CheckStatus {
    ReviewRequired(String),
    Failed(String),
    PassedWithWarnings(String),
    Pending,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct ReviewRequestArgs {
    pub new_status: CheckStatus,
    pub new_results: String,
}
