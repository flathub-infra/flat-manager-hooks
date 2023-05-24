use anyhow::Result;
use reqwest::blocking::Client;
use serde::Serialize;

use crate::{
    config::Config,
    utils::{get_job_id, retry},
};

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
    new_status: CheckStatus,
    new_results: String,
}

fn set_check_status(args: &ReviewRequestArgs, config: &Config) -> Result<()> {
    let client = Client::new();
    retry(|| {
        client
            .post(format!(
                "{}/api/v1/job/{}/check/review",
                config.flat_manager_url,
                get_job_id()?
            ))
            .bearer_auth(&config.flat_manager_token)
            .json(args)
            .send()?
            .error_for_status()?;
        Ok(())
    })
}

pub fn require_review(reason: &str, result: String, config: &Config) -> Result<()> {
    set_check_status(
        &ReviewRequestArgs {
            new_status: CheckStatus::ReviewRequired(reason.to_string()),
            new_results: result,
        },
        config,
    )
}

pub fn mark_failure(reason: &str, result: String, config: &Config) -> Result<()> {
    set_check_status(
        &ReviewRequestArgs {
            new_status: if config.validation_observe_only {
                CheckStatus::PassedWithWarnings(reason.to_string())
            } else {
                CheckStatus::Failed(reason.to_string())
            },
            new_results: result,
        },
        config,
    )
}

pub fn mark_passed_with_warnings(
    reason: &str,
    result: String,
    config: &Config,
) -> Result<()> {
    set_check_status(
        &ReviewRequestArgs {
            new_status: CheckStatus::PassedWithWarnings(reason.to_string()),
            new_results: result,
        },
        config,
    )
}

pub fn mark_still_pending(result: String, config: &Config) -> Result<()> {
    /* We can't mark it as passed because the process hasn't exited yet, but we still need to upload the results */
    set_check_status(
        &ReviewRequestArgs {
            new_status: CheckStatus::Pending,
            new_results: result,
        },
        config,
    )
}
