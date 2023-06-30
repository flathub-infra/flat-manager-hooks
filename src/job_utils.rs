use serde::{Deserialize, Serialize};

use crate::review::diagnostics::ValidationDiagnostic;

#[derive(Deserialize)]
pub struct BuildExtended {
    pub build: Build,
    pub build_refs: Vec<BuildRef>,
}

#[derive(Deserialize)]
pub struct Build {
    pub app_id: Option<String>,
    pub repo: String,
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct BuildNotificationRequest<'a> {
    pub app_id: String,
    pub build_id: i64,
    pub build_repo: String,
    pub diagnostics: &'a Vec<ValidationDiagnostic>,
}
