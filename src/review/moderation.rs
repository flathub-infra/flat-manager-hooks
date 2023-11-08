use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::{
    config::Config,
};

/// Review the metadata for a build and create a review request to send to the backend.
pub fn review_build<C: Config>(
    config: &C,
) -> Result<ReviewRequest> {
    /* Collect the app's metadata and send it to the backend, to see if it needs to be held for review */
    let request = ReviewRequest {
        build_id: config.get_build_id()?,
        job_id: config.get_job_id()?,
    };

    Ok(request)
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
