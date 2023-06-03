use anyhow::Result;
use clap::Args;
use reqwest::blocking::Client;
use serde::Serialize;

use crate::config::Config;

#[derive(Args, Debug)]
pub struct ReviewArgs {}

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

impl ReviewArgs {
    pub fn run(&self, config: &Config) -> Result<()> {
        let client = Client::new();

        let job_id: i64 = std::env::var("FLAT_MANAGER_JOB_ID")?.parse()?;
        client
            .post(format!(
                "{}/api/v1/job/{}/check/review",
                config.flat_manager_url, job_id
            ))
            .bearer_auth(&config.flat_manager_token)
            .json(&ReviewRequestArgs {
                new_status: CheckStatus::ReviewRequired("".to_string()),
            })
            .send()?
            .error_for_status()?;

        Ok(())
    }
}
