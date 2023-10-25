use std::collections::HashMap;
use std::process::Command;

use anyhow::Result;
use elementtree::Element;
use ostree::gio::Cancellable;
use ostree::Repo;
use reqwest::Url;

use crate::config::ValidateConfig;
use crate::{
    job_utils::BuildExtended,
    utils::{app_id_from_ref, get_appstream_path, is_primary_ref, load_appstream},
};

use super::{
    diagnostics::{CheckResult, DiagnosticInfo, ValidationDiagnostic},
    review_files::review_files,
};

/// Run all of the validations on a build.
pub fn validate_build<C: ValidateConfig>(
    config: &C,
    build: &BuildExtended,
    repo: &Repo,
    refs: &HashMap<String, String>,
    result: &mut CheckResult,
) -> Result<()> {
    for (refstring, checksum) in refs.iter() {
        if is_primary_ref(refstring) {
            result.diagnostics.extend(validate_primary_ref(
                config, build, repo, refstring, checksum,
            )?);
        }
    }

    Ok(())
}

/// Run all the validations specific to "primary" refs (app, runtime, or extension).
pub fn validate_primary_ref<C: ValidateConfig>(
    config: &C,
    build: &BuildExtended,
    repo: &Repo,
    refstring: &str,
    checksum: &str,
) -> Result<Vec<ValidationDiagnostic>> {
    let (ref_files, _checksum) = repo.read_commit(checksum, Cancellable::NONE)?;

    let mut diagnostics = vec![];
    diagnostics.extend(validate_flatpak_build(refstring)?);

    /* Validate the appstream catalog file. This is the one that shows up on the website and in software centers.
    (The other ones are exported to the user's system.) */
    diagnostics.extend(validate_appstream_catalog_file(
        config, build, repo, checksum, refstring,
    )?);

    /* Run validations that cover all the files, e.g. warnings for executables with the wrong target architecture */
    diagnostics.extend(review_files(&ref_files, refstring)?);

    Ok(diagnostics)
}

fn run_flatpak_builder_lint(refstring: &str) -> Result<Vec<ValidationDiagnostic>> {
    let output = Command::new("flatpak")
        .args([
            "run",
            "--command=flatpak-builder-lint",
            "org.flatpak.Builder",
            "--exceptions",
            "repo",
            "--cwd",
            "noop",
        ])
        .output()?;

    if !output.status.success() {
        Ok(vec![ValidationDiagnostic::new(
            DiagnosticInfo::FlatpakBuilderLint {
                stdout: serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            },
            Some(refstring.to_string()),
        )])
    } else {
        Ok(vec![])
    }
}

fn validate_flatpak_build(refstring: &str) -> Result<Vec<ValidationDiagnostic>> {
    let mut diagnostics = vec![];

    diagnostics.extend(run_flatpak_builder_lint(refstring)?);

    Ok(diagnostics)
}

fn validate_appstream_catalog_file<C: ValidateConfig>(
    config: &C,
    build: &BuildExtended,
    repo: &Repo,
    checksum: &str,
    refstring: &str,
) -> Result<Vec<ValidationDiagnostic>> {
    let app_id = app_id_from_ref(refstring);

    let appstream_path = get_appstream_path(&app_id);
    let (_appstream_content, appstream) = match load_appstream(repo, &app_id, checksum) {
        Ok(x) => x,
        Err(e) => {
            return Ok(vec![ValidationDiagnostic::new_failed_to_load_appstream(
                &appstream_path,
                &e.to_string(),
                refstring,
            )])
        }
    };

    /* Make sure the file contains one component, and that component is the correct app */
    if appstream.tag().name() != "components" {
        return Ok(vec![ValidationDiagnostic::new_failed_to_load_appstream(
            &appstream_path,
            &format!("Expected <components>, not <{}>", appstream.tag()),
            refstring,
        )]);
    }
    let component = match appstream.find_all("component").collect::<Vec<_>>()[..] {
        [component] => component,
        [_, ..] => {
            return Ok(vec![ValidationDiagnostic::new_failed_to_load_appstream(
                &appstream_path,
                "Expected exactly one <component>, found multiple",
                refstring,
            )])
        }
        [] => {
            return Ok(vec![ValidationDiagnostic::new_failed_to_load_appstream(
                &appstream_path,
                "Expected exactly one <component>, found none",
                refstring,
            )])
        }
    };

    let mut diagnostics = vec![];

    diagnostics.extend(validate_appstream_component(
        component,
        refstring,
        &appstream_path,
    )?);

    /* For now, we don't run `appstream-util validate` or `appstreamcli validate` on this file, because it sometimes
    produces false positives. */

    /* If the app is free software, it must have a link to the build log. The link is stored in flat-manager and will
    be inserted into appstream by the publish hook. */
    let license = component.find("project_license").map(|x| x.text());
    let is_free_software = config.get_is_free_software(&app_id, license)?;

    if is_free_software {
        let build_url = build
            .build_refs
            .iter()
            .find(|x| x.ref_name == refstring)
            .and_then(|x| x.build_log_url.as_ref())
            .or(build.build.build_log_url.as_ref());

        if build_url.is_none() || Url::parse(build_url.unwrap()).is_err() {
            diagnostics.push(ValidationDiagnostic {
                info: DiagnosticInfo::MissingBuildLogUrl,
                refstring: Some(refstring.to_string()),
                is_warning: false,
            })
        }
    }

    Ok(diagnostics)
}

/// Make sure an appstream component has the correct ID.
fn check_appstream_component_id(component: &Element, refstring: &str) -> Result<(), String> {
    match component.find_all("id").count() {
        1 => {}
        0 => return Err("Appstream component does not have an ID".to_owned()),
        _ => return Err("Appstream component has multiple IDs".to_owned()),
    }

    let id = component.find("id").unwrap();
    let expected_id = app_id_from_ref(refstring);
    if id.text() != expected_id && id.text() != format!("{expected_id}.desktop") {
        return Err(format!(
            "Appstream component ID ({}) does not match expected ID ({expected_id})",
            id.text()
        ));
    }
    Ok(())
}

fn validate_appstream_component(
    component: &Element,
    refstring: &str,
    appstream_path: &str,
) -> Result<Vec<ValidationDiagnostic>> {
    let mut diagnostics = vec![];

    if let Err(e) = check_appstream_component_id(component, refstring) {
        diagnostics.push(ValidationDiagnostic::new_failed_to_load_appstream(
            appstream_path,
            &e,
            refstring,
        ));
    }

    Ok(diagnostics)
}
