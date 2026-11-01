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
    pub token_type: Option<String>,
}

#[derive(Deserialize)]
pub struct BuildRef {
    pub ref_name: String,
    pub build_log_url: Option<String>,
}

#[derive(Serialize)]
pub struct LintBuildInfo<'a> {
    pub build: LintBuild<'a>,
    pub build_refs: Vec<LintBuildRef<'a>>,
}

#[derive(Serialize)]
pub struct LintBuild<'a> {
    pub repo: &'a str,
    pub token_type: Option<&'a str>,
}

#[derive(Serialize)]
pub struct LintBuildRef<'a> {
    pub ref_name: &'a str,
}

impl BuildExtended {
    pub fn linter_projection(&self) -> LintBuildInfo<'_> {
        LintBuildInfo {
            build: LintBuild {
                repo: &self.build.repo,
                token_type: self.build.token_type.as_deref(),
            },
            build_refs: self
                .build_refs
                .iter()
                .map(|build_ref| LintBuildRef {
                    ref_name: &build_ref.ref_name,
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "status", content = "reason")]
pub enum CheckStatus {
    ReviewRequired(String),
    Failed(String),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn build_extended(token_type: Option<&str>) -> BuildExtended {
        BuildExtended {
            build: Build {
                app_id: Some("org.example.App".to_owned()),
                repo: "stable".to_owned(),
                build_log_url: Some("https://example.com/build.log".to_owned()),
                token_type: token_type.map(str::to_owned),
            },
            build_refs: vec![BuildRef {
                ref_name: "app/org.example.App/x86_64/stable".to_owned(),
                build_log_url: Some("https://example.com/ref.log".to_owned()),
            }],
        }
    }

    #[test]
    fn linter_projection_exposes_only_policy_fields() {
        let projection =
            serde_json::to_value(build_extended(Some("app")).linter_projection()).unwrap();

        assert_eq!(
            projection,
            serde_json::json!({
                "build": {
                    "repo": "stable",
                    "token_type": "app",
                },
                "build_refs": [{
                    "ref_name": "app/org.example.App/x86_64/stable",
                }],
            })
        );
    }

    #[test]
    fn linter_projection_keeps_missing_token_type_explicit() {
        let projection = serde_json::to_value(build_extended(None).linter_projection()).unwrap();

        assert_eq!(projection["build"]["token_type"], serde_json::Value::Null);
    }
}
