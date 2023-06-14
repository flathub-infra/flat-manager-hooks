use anyhow::Result;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

use crate::{config::Config, review::diagnostics::CheckResult, utils::retry};

pub fn get_job_id() -> Result<i64> {
    Ok(std::env::var("FLAT_MANAGER_JOB_ID")?.parse()?)
}

pub fn get_build_id() -> Result<i64> {
    Ok(std::env::var("FLAT_MANAGER_BUILD_ID")?.parse()?)
}

pub fn get_is_republish() -> Result<bool> {
    Ok(std::env::var("FLAT_MANAGER_IS_REPUBLISH")? == "true")
}

#[derive(Deserialize)]
pub struct BuildExtended {
    pub build: Build,
    pub build_refs: Vec<BuildRef>,
}

#[derive(Deserialize)]
pub struct Build {
    pub build_log_url: Option<String>,
}

#[derive(Deserialize)]
pub struct BuildRef {
    pub ref_name: String,
    pub build_log_url: Option<String>,
}

pub fn get_build(config: &Config) -> Result<BuildExtended> {
    let client = Client::new();
    let build_id = get_build_id()?;
    let build = retry(|| {
        client
            .get(format!(
                "{}/api/v1/build/{}/extended",
                config.flat_manager_url, build_id
            ))
            .bearer_auth(&config.flat_manager_token)
            .send()?
            .error_for_status()?
            .json::<BuildExtended>()
    })?;
    Ok(build)
}

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

pub fn require_review(reason: &str, result: &CheckResult, config: &Config) -> Result<()> {
    set_check_status(
        &ReviewRequestArgs {
            new_status: CheckStatus::ReviewRequired(reason.to_string()),
            new_results: serde_json::to_string(result)?,
        },
        config,
    )
}

pub fn mark_failure(reason: &str, result: &CheckResult, config: &Config) -> Result<()> {
    set_check_status(
        &ReviewRequestArgs {
            new_status: if config.validation_observe_only {
                CheckStatus::PassedWithWarnings(reason.to_string())
            } else {
                CheckStatus::Failed(reason.to_string())
            },
            new_results: serde_json::to_string(result)?,
        },
        config,
    )
}

pub fn mark_still_pending(result: &CheckResult, config: &Config) -> Result<()> {
    /* We can't mark it as passed because the process hasn't exited yet, but we still need to upload the results */
    set_check_status(
        &ReviewRequestArgs {
            new_status: CheckStatus::Pending,
            new_results: serde_json::to_string(result)?,
        },
        config,
    )
}
