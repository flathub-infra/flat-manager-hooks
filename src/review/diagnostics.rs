use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct CheckResult {
    pub diagnostics: Vec<ValidationDiagnostic>,
}

#[derive(Debug, Serialize)]
pub struct ValidationDiagnostic {
    pub refstring: Option<String>,
    pub is_warning: bool,
    #[serde(flatten)]
    pub info: DiagnosticInfo,
}

#[derive(Debug, Serialize)]
#[serde(tag = "category", content = "data", rename_all = "snake_case")]
pub enum DiagnosticInfo {
    /// The appstream file is missing or couldn't be read.
    FailedToLoadAppstream { path: String, error: String },
    /// There is a problem in one of the appstream files.
    FlatpakBuilderLint {
        stdout: serde_json::value::Value,
        stderr: String,
    },
    /// The app is FOSS, but a URL for the build's CI log was not given or is not a valid URL.
    MissingBuildLogUrl,
}

impl ValidationDiagnostic {
    pub fn new(info: DiagnosticInfo, refstring: Option<String>) -> Self {
        Self {
            refstring,
            is_warning: false,
            info,
        }
    }

    pub fn new_failed_to_load_appstream(path: &str, error: &str, refstring: &str) -> Self {
        Self::new(
            DiagnosticInfo::FailedToLoadAppstream {
                path: path.to_string(),
                error: error.to_string(),
            },
            Some(refstring.to_string()),
        )
    }
}
