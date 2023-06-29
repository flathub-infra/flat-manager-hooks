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
    /// The appstream file is missing or couldn't be read.
    FailedToLoadAppstream { path: String, error: String },
    /// There is a problem in one of the appstream files.
    AppstreamValidation {
        path: String,
        stdout: String,
        stderr: String,
    },
    /// The app does not have a suitable icon.
    MissingIcon { appstream_path: String },
    /// The app has a remote icon listed in appstream, but no icon included in the build.
    NoLocalIcon { appstream_path: String },
    /// The app is FOSS, but a URL for the build's CI log was not given or is not a valid URL.
    MissingBuildLogUrl,
    /// A screenshot in appstream does not point to the flathub screenshot mirror.
    ScreenshotNotMirrored {
        appstream_path: String,
        urls: Vec<String>,
    },
    /// A screenshot in appstream points to the flathub screenshot mirror, but the screenshot is not found in the
    /// screenshots ref.
    MirroredScreenshotNotFound {
        appstream_path: String,
        expected_branch: String,
        urls: Vec<String>,
    },
    /// No screenshots branch was uploaded.
    NoScreenshotBranch { expected_branch: String },
    /// The ref contains executables or shared library files that are for a different architecture than the ref.
    WrongArchExecutables {
        expected_arch: String,
        executables: Vec<WrongArchExecutable>,
    },
}

#[derive(Debug, Serialize)]
pub struct WrongArchExecutable {
    pub path: String,
    pub detected_arch: String,
    pub detected_arch_code: u16,
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
