use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct CheckResult {
    pub diagnostics: Vec<ValidationDiagnostic>,
}

#[derive(Debug, Serialize)]
pub struct ValidationDiagnostic {
    pub refstring: Option<String>,
    pub is_warning: bool,
    pub info: DiagnosticInfo,
}

#[derive(Debug, Serialize)]
#[serde(tag = "category", content = "data")]
pub enum DiagnosticInfo {
    FailedToLoadAppstream {
        path: String,
        error: String,
    },
    AppstreamValidation {
        path: String,
        stdout: String,
        stderr: String,
    },
    MissingIcon {
        appstream_path: String,
    },
    NoLocalIcon {
        appstream_path: String,
    },
}

impl ValidationDiagnostic {
    pub fn new(info: DiagnosticInfo, refstring: Option<String>) -> Self {
        Self {
            refstring,
            is_warning: false,
            info,
        }
    }

    pub fn new_warning(info: DiagnosticInfo, refstring: Option<String>) -> Self {
        Self {
            refstring,
            is_warning: true,
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
