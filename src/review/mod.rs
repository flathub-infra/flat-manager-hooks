use std::collections::HashMap;

use anyhow::Result;
use log::info;
use ostree::gio::{Cancellable, File};
use ostree::Repo;

use crate::config::{Config, ValidateConfig};
use crate::review::diagnostics::CheckResult;
use crate::review::moderation::review_build;
use crate::review::validation::validate_build;

pub mod diagnostics;
pub mod moderation;
mod validation;

pub fn do_validation<C: ValidateConfig>(
    config: &C,
) -> Result<(Repo, HashMap<String, String>, CheckResult)> {
    /* Open the build repo at the current directory */
    let repo = Repo::new(&File::for_path("."));
    repo.open(Cancellable::NONE)?;

    let refs = repo.list_refs(None, Cancellable::NONE)?;

    let build = config.get_build()?;

    info!("Refs present in build: {:?}", refs.keys());

    let mut result = CheckResult {
        diagnostics: vec![],
    };

    validate_build(config, &build, &repo, &refs, &mut result)?;

    Ok((repo, refs, result))
}

pub fn do_review<C: Config>(config: &C) -> Result<()> {
    let (_, _, result) = do_validation(config)?;

    /* If any errors were found, mark the check as failed */
    if result.diagnostics.iter().any(|d| !d.is_warning) {
        config.mark_failure("One or more validations failed.", &result)?;
        config.post_email_notification(&result)?;
        return Ok(());
    }

    let request = review_build(config)?;

    /* Make sure nothing failed while collecting metadata for the moderation step */
    if result.diagnostics.iter().any(|d| !d.is_warning) {
        config.mark_failure("One or more validations failed.", &result)?;
        config.post_email_notification(&result)?;
        return Ok(());
    }

    info!("Submitting appdata for review: {request:?}");

    let response = config.post_review_request(request)?;

    if response.requires_review {
        config.require_review(
            "Some of the changes in this build require review by a moderator.",
            &result,
        )?;
    } else {
        config.mark_still_pending(&result)?;
    }

    config.post_email_notification(&result)?;

    Ok(())
}
