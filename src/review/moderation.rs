use std::collections::HashMap;

use anyhow::{Result, anyhow};
use elementtree::Element;
use ostree::Repo;
use serde::{Deserialize, Serialize};

use crate::{
    job_utils::{get_build_id, get_job_id},
    utils::{app_id_from_ref, arch_from_ref, get_appstream_path, is_primary_ref, load_appstream},
};

use super::diagnostics::{CheckResult, ValidationDiagnostic};

/// Review the metadata for a build and create a review request to send to the backend.
pub fn review_build(
    repo: &Repo,
    refs: &HashMap<String, String>,
    result: &mut CheckResult,
) -> Result<ReviewRequest> {
    /* Collect the app's metadata and send it to the backend, to see if it needs to be held for review */
    let mut request = ReviewRequest {
        build_id: get_build_id()?,
        job_id: get_job_id()?,
        app_metadata: HashMap::new(),
    };

    for (refstring, checksum) in refs.iter() {
        if is_primary_ref(refstring) {
            match review_primary_ref(repo, refstring, checksum) {
                Ok(Some(item)) => {
                    request
                        .app_metadata
                        .insert(app_id_from_ref(refstring), item);
                }
                Ok(None) => {}
                Err(diagnostic) => {
                    result.diagnostics.push(diagnostic);
                }
            }
        }
    }

    Ok(request)
}

/// Collects the metadata from a single ref in the build.
fn review_primary_ref(
    repo: &Repo,
    refstring: &str,
    checksum: &str,
) -> Result<Option<ReviewItem>, ValidationDiagnostic> {
    let arch = arch_from_ref(refstring);

    /* For now, only review the x86_64 upload, since that's the one we show on the website */
    if arch == "x86_64" {
        let app_id = app_id_from_ref(refstring);
        let appstream =
            load_appstream(repo, &app_id, checksum).and_then(|(_, x)| get_app_metadata(&x));

        return match appstream {
            Ok(metadata) => Ok(Some(metadata)),
            Err(e) => Err(ValidationDiagnostic::new_failed_to_load_appstream(
                &get_appstream_path(&app_id),
                &e.to_string(),
                refstring,
            )),
        };
    }

    Ok(None)
}

fn get_app_metadata(root: &Element) -> Result<ReviewItem> {
    let component = root
        .find("component")
        .ok_or(anyhow!("No <component> in appstream"))?;

    Ok(ReviewItem {
        name: component.find("name").map(|f| f.text().to_string()),
        summary: component.find("summary").map(|f| f.text().to_string()),
        developer_name: component
            .find("developer_name")
            .map(|f| f.text().to_string()),
        project_license: component
            .find("project_license")
            .map(|f| f.text().to_string()),
        project_group: component
            .find("project_group")
            .map(|f| f.text().to_string()),
        compulsory_for_desktop: component
            .find("compulsory_for_desktop")
            .map(|f| f.text().to_string()),
    })
}

#[derive(Debug, Serialize)]
pub struct ReviewItem {
    name: Option<String>,
    summary: Option<String>,
    developer_name: Option<String>,
    project_license: Option<String>,
    project_group: Option<String>,
    compulsory_for_desktop: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ReviewRequest {
    pub build_id: i64,
    pub job_id: i64,
    pub app_metadata: HashMap<String, ReviewItem>,
}

#[derive(Deserialize)]
pub struct ReviewRequestResponse {
    pub requires_review: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_app_metadata() {
        let el = Element::from_reader(
            r#"
<?xml version="1.0" encoding="UTF-8"?>
<components>
<component>
    <name>Test</name>
    <summary>Test summary</summary>
    <project_group>Test project group</project_group>
    <project_license>MIT</project_license>
    <developer_name>Test developer</developer_name>
    <compulsory_for_desktop>Test desktop</compulsory_for_desktop>
</component>
</components>"#
                .as_bytes(),
        )
        .unwrap();
        let metadata = get_app_metadata(&el).unwrap();

        assert_eq!(metadata.name, Some("Test".to_string()));
        assert_eq!(metadata.summary, Some("Test summary".to_string()));
        assert_eq!(
            metadata.project_group,
            Some("Test project group".to_string())
        );
        assert_eq!(metadata.project_license, Some("MIT".to_string()));
        assert_eq!(metadata.developer_name, Some("Test developer".to_string()));
        assert_eq!(
            metadata.compulsory_for_desktop,
            Some("Test desktop".to_string())
        );
    }
}
