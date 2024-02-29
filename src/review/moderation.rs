use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::config::Config;

/// Review the metadata for a build and create a review request to send to the backend.
pub fn review_build<C: Config>(config: &C) -> Result<ReviewRequest> {
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
