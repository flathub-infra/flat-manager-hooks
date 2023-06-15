use anyhow::{anyhow, Result};
use log::info;
use ostree::gio::{Cancellable, File};
use ostree::Repo;

use crate::config::Config;
use crate::job_utils::{get_build, mark_failure, mark_still_pending, require_review};
use crate::review::diagnostics::CheckResult;
use crate::review::moderation::{review_build, ReviewRequestResponse};
use crate::review::validation::validate_build;
use crate::utils::retry;

use self::moderation::ReviewRequest;

pub mod diagnostics;
mod moderation;
mod validation;

pub fn do_review(config: &Config) -> Result<()> {
    /* Open the build repo at the current directory */
    let repo = Repo::new(&File::for_path("."));
    repo.open(Cancellable::NONE)?;

    let refs = repo.list_refs(None, Cancellable::NONE)?;

    let build = get_build(config)?;

    info!("Refs present in build: {:?}", refs.keys());

    let mut result = CheckResult {
        diagnostics: vec![],
    };

    validate_build(config, &build, &repo, &refs, &mut result)?;

    /* If any errors were found, mark the check as failed */
    if result.diagnostics.iter().any(|d| !d.is_warning) {
        mark_failure("One or more validations failed.", &result, config)?;
        return Ok(());
    }

    let request = review_build(&repo, &refs, &mut result)?;

    /* Make sure nothing failed while collecting metadata for the moderation step */
    if result.diagnostics.iter().any(|d| !d.is_warning) {
        mark_failure("One or more validations failed.", &result, config)?;
        return Ok(());
    }

    submit_review_request(request, result, config)?;

    Ok(())
}

fn submit_review_request(
    request: ReviewRequest,
    result: CheckResult,
    config: &Config,
) -> Result<()> {
    // Submit the data to the backend for review
    let endpoint = format!("{}/moderation/submit_review_request", config.backend_url);

    let convert_err = |e| anyhow!("Failed to contact backend {}: {}", &endpoint, e);

    info!("Submitting appdata for review: {request:?}");

    let response = retry(|| {
        reqwest::blocking::Client::new()
            .post(&endpoint)
            .bearer_auth(&config.flat_manager_token)
            .json(&request)
            .send()
            .map_err(convert_err)?
            .error_for_status()
            .map_err(convert_err)?
            .json::<ReviewRequestResponse>()
            .map_err(convert_err)
    })?;

    if response.requires_review {
        require_review(
            "Some of the changes in this build require review by a moderator.",
            &result,
            config,
        )?;
    } else {
        mark_still_pending(&result, config)?;
    }

    Ok(())
}
