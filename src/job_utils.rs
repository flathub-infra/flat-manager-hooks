use std::error::Error;

use reqwest::blocking::Client;
use serde::Serialize;

use crate::{config::Config, utils::get_job_id};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "status", content = "reason")]
pub enum CheckStatus {
    ReviewRequired(String),
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
struct ReviewRequestArgs {
    new_status: CheckStatus,
}

pub fn require_review(reason: &str, config: &Config) -> Result<(), Box<dyn Error>> {
    let client = Client::new();
    client
        .post(format!(
            "{}/api/v1/job/{}/check/review",
            config.flat_manager_url,
            get_job_id()?
        ))
        .bearer_auth(&config.flat_manager_token)
        .json(&ReviewRequestArgs {
            new_status: CheckStatus::ReviewRequired(reason.to_string()),
        })
        .send()?
        .error_for_status()?;

    Ok(())
}
