use anyhow::{anyhow, Result};
use log::info;
use reqwest::blocking::Client;
use serde::Deserialize;

use crate::{
    job_utils::{BuildExtended, BuildNotificationRequest, CheckStatus, ReviewRequestArgs},
    review::{
        diagnostics::CheckResult,
        moderation::{ReviewRequest, ReviewRequestResponse},
    },
    storefront::{get_is_free_software, StorefrontInfo},
    utils::retry,
};

/// Services for the validation step.
pub trait ValidateConfig {
    fn get_is_free_software(&self, app_id: &str, license: Option<&str>) -> Result<bool>;
    fn get_build(&self) -> Result<BuildExtended>;
}

pub trait Config: ValidateConfig {
    fn get_build_id(&self) -> Result<i64>;
    fn get_job_id(&self) -> Result<i64>;
    fn get_is_republish(&self) -> Result<bool>;
    fn validation_observe_only(&self) -> bool;

    fn get_storefront_info(&self, app_id: &str) -> Result<StorefrontInfo>;

    fn set_check_status(&self, args: &ReviewRequestArgs) -> Result<()>;

    fn require_review(&self, reason: &str, result: &CheckResult) -> Result<()> {
        self.set_check_status(&ReviewRequestArgs {
            new_status: CheckStatus::ReviewRequired(reason.to_string()),
            new_results: serde_json::to_string(result)?,
        })
    }

    fn mark_failure(&self, reason: &str, result: &CheckResult) -> Result<()> {
        self.set_check_status(&ReviewRequestArgs {
            new_status: if self.validation_observe_only() {
                CheckStatus::Pending
            } else {
                CheckStatus::Failed(reason.to_string())
            },
            new_results: serde_json::to_string(result)?,
        })
    }

    fn mark_still_pending(&self, result: &CheckResult) -> Result<()> {
        /* We can't mark it as passed because the process hasn't exited yet, but we still need to upload the results */
        self.set_check_status(&ReviewRequestArgs {
            new_status: CheckStatus::Pending,
            new_results: serde_json::to_string(result)?,
        })
    }

    fn post_review_request(&self, request: ReviewRequest) -> Result<ReviewRequestResponse>;
    fn post_email_notification(&self, result: &CheckResult) -> Result<()>;
}

#[derive(Clone, Deserialize)]
pub struct RegularConfig {
    pub backend_url: String,
    pub flat_manager_url: String,
    pub flat_manager_token: String,
    #[serde(default)]
    pub validation_observe_only: bool,
}

impl RegularConfig {}

impl ValidateConfig for RegularConfig {
    /// Uses a backend endpoint to determine if an app is FOSS based on its ID and license.
    fn get_is_free_software(&self, app_id: &str, license: Option<&str>) -> Result<bool> {
        get_is_free_software(&self.backend_url, app_id, license)
    }

    fn get_build(&self) -> Result<BuildExtended> {
        let client = Client::new();
        let build_id = self.get_build_id()?;
        let build = retry(|| {
            client
                .get(format!(
                    "{}/api/v1/build/{}/extended",
                    self.flat_manager_url, build_id
                ))
                .bearer_auth(&self.flat_manager_token)
                .send()?
                .error_for_status()?
                .json::<BuildExtended>()
        })?;
        Ok(build)
    }
}

impl Config for RegularConfig {
    fn get_job_id(&self) -> Result<i64> {
        Ok(std::env::var("FLAT_MANAGER_JOB_ID")?.parse()?)
    }

    fn get_build_id(&self) -> Result<i64> {
        Ok(std::env::var("FLAT_MANAGER_BUILD_ID")?.parse()?)
    }

    fn get_is_republish(&self) -> Result<bool> {
        Ok(std::env::var("FLAT_MANAGER_IS_REPUBLISH")? == "true")
    }

    fn validation_observe_only(&self) -> bool {
        self.validation_observe_only
    }

    fn get_storefront_info(&self, app_id: &str) -> Result<StorefrontInfo> {
        StorefrontInfo::fetch(&self.backend_url, app_id)
    }

    fn set_check_status(&self, args: &ReviewRequestArgs) -> Result<()> {
        let client = Client::new();
        retry(|| {
            client
                .post(format!(
                    "{}/api/v1/job/{}/check/review",
                    self.flat_manager_url,
                    self.get_job_id()?
                ))
                .bearer_auth(&self.flat_manager_token)
                .json(args)
                .send()?
                .error_for_status()?;
            Ok(())
        })
    }

    fn post_review_request(&self, request: ReviewRequest) -> Result<ReviewRequestResponse> {
        let endpoint = format!("{}/moderation/submit_review_request", self.backend_url);
        let client = reqwest::blocking::Client::new();
        let convert_err = |e| anyhow!("Failed to contact backend {}: {}", &endpoint, e);

        retry(|| {
            client
                .post(&endpoint)
                .bearer_auth(&self.flat_manager_token)
                .json(&request)
                .send()
                .map_err(convert_err)?
                .error_for_status()
                .map_err(convert_err)?
                .json::<ReviewRequestResponse>()
                .map_err(convert_err)
        })
    }

    fn post_email_notification(&self, result: &CheckResult) -> Result<()> {
        if result.diagnostics.is_empty() {
            return Ok(());
        }

        let build = self.get_build()?;

        let endpoint = format!("{}/emails/build-notification", self.backend_url);

        let convert_err = |e| anyhow!("Failed to contact backend {}: {}", &endpoint, e);

        let app_id = if let Some(app_id) = &build.build.app_id {
            app_id.clone()
        } else {
            info!("Skipping notification email because app ID is not set for this build");
            return Ok(());
        };

        info!("Submitting notification email");

        let request = BuildNotificationRequest {
            app_id,
            build_id: self.get_build_id()?,
            build_repo: build.build.repo,
            diagnostics: &result.diagnostics,
        };

        retry(|| {
            reqwest::blocking::Client::new()
                .post(&endpoint)
                .bearer_auth(&self.flat_manager_token)
                .json(&request)
                .send()
                .map_err(convert_err)?
                .error_for_status()
                .map_err(convert_err)
        })?;

        Ok(())
    }
}
