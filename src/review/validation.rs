use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::process::Command;

use anyhow::Result;
use elementtree::Element;
use ostree::gio::FileQueryInfoFlags;
use ostree::prelude::FileExt;
use ostree::Repo;
use ostree::{gio::Cancellable, prelude::Cast};
use reqwest::Url;

use crate::config::ValidateConfig;
use crate::utils::is_screenshots_ref;
use crate::{
    job_utils::BuildExtended,
    utils::{
        app_id_from_ref, arch_from_ref, get_appstream_path, is_primary_ref, load_appstream,
        read_repo_file, ref_directory,
    },
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
        } else if is_screenshots_ref(refstring) {
            result
                .diagnostics
                .extend(validate_screenshots_ref(repo, refstring, checksum)?);
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
    let app_id = app_id_from_ref(refstring);
    let (ref_files, _checksum) = repo.read_commit(checksum, Cancellable::NONE)?;

    let mut diagnostics = vec![];

    /* Check for a local 128x128 icon. If it's not present, the appstream files must contain a remote icon. */
    let has_local_icon = ref_files
        .resolve_relative_path(format!(
            "files/share/app-info/icons/flatpak/128x128/{app_id}.png"
        ))
        .query_exists(Cancellable::NONE);

    /* Validate the input appstream files. Check both the current and legacy paths. */
    diagnostics.extend(validate_appstream_file(
        repo,
        checksum,
        refstring,
        &format!("files/share/appdata/{app_id}.appdata.xml"),
        has_local_icon,
    )?);
    diagnostics.extend(validate_appstream_file(
        repo,
        checksum,
        refstring,
        &format!("files/share/metainfo/{app_id}.metainfo.xml"),
        has_local_icon,
    )?);

    /* Validate the appstream catalog file. This is the one that shows up on the website and in software centers.
    (The other ones are exported to the user's system.) */
    diagnostics.extend(validate_appstream_catalog_file(
        config,
        build,
        repo,
        checksum,
        refstring,
        has_local_icon,
    )?);

    /* Run validations that cover all the files, e.g. warnings for executables with the wrong target architecture */
    diagnostics.extend(review_files(&ref_files, refstring)?);

    Ok(diagnostics)
}

fn validate_appstream_file(
    repo: &Repo,
    checksum: &str,
    refstring: &str,
    appstream_path: &str,
    has_local_icon: bool,
) -> Result<Vec<ValidationDiagnostic>> {
    let mut diagnostics = vec![];

    let appstream_file = repo
        .read_commit(checksum, Cancellable::NONE)?
        .0
        .resolve_relative_path(appstream_path);

    /* It's okay for either of these files to not exist */
    if !appstream_file.query_exists(Cancellable::NONE) {
        return Ok(vec![]);
    };

    let appstream_content = match read_repo_file(appstream_file.downcast_ref().unwrap()) {
        Ok(content) => content,
        Err(error) => {
            diagnostics.push(ValidationDiagnostic::new_failed_to_load_appstream(
                appstream_path,
                &error.to_string(),
                refstring,
            ));
            return Ok(diagnostics);
        }
    };

    let appstream = Element::from_reader(appstream_content.as_slice())?;

    diagnostics.extend(run_appstream_validate(
        appstream_content,
        refstring,
        appstream_path,
    )?);

    diagnostics.extend(validate_appstream_component(
        &appstream,
        refstring,
        appstream_path,
        has_local_icon,
    )?);

    Ok(diagnostics)
}

fn validate_appstream_catalog_file<C: ValidateConfig>(
    config: &C,
    build: &BuildExtended,
    repo: &Repo,
    checksum: &str,
    refstring: &str,
    has_local_icon: bool,
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
        has_local_icon,
    )?);

    /* Check that the screenshots are mirrored */
    if let Some(screenshots) = component.find("screenshots") {
        diagnostics.extend(validate_appstream_screenshot_mirror(
            repo,
            refstring,
            screenshots,
            &appstream_path,
        )?);
    }

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

fn validate_appstream_screenshot_mirror(
    repo: &Repo,
    refstring: &str,
    screenshots: &Element,
    appstream_path: &str,
) -> Result<Vec<ValidationDiagnostic>> {
    let mut not_mirrored_screenshots = vec![];
    let mut not_found_screenshots = vec![];

    let arch = arch_from_ref(refstring);
    let screenshots_rev = repo.resolve_rev(&format!("screenshots/{arch}"), true)?;

    let screenshots_file = match screenshots_rev {
        Some(screenshots_rev) => {
            repo.read_commit(screenshots_rev.as_str(), Cancellable::NONE)?
                .0
        }
        None => {
            return Ok(vec![ValidationDiagnostic::new(
                DiagnosticInfo::NoScreenshotBranch {
                    expected_branch: format!("screenshots/{arch}"),
                },
                Some(refstring.to_string()),
            )])
        }
    };

    for screenshot in screenshots.find_all("screenshot") {
        let source = screenshot.find_all("image").find(|i| {
            let t = i.get_attr("type");
            t.is_none() || t == Some("source")
        });

        let source = if let Some(source) = source {
            source
        } else {
            continue;
        };

        let thumbnails = screenshot
            .find_all("image")
            .filter(|i| i.get_attr("type") == Some("thumbnail"))
            .collect::<Vec<_>>();

        if thumbnails.is_empty() {
            not_mirrored_screenshots.push(source.text().to_owned());
        }

        for thumbnail in thumbnails {
            let url = thumbnail.text();
            if let Some(filename) = url.strip_prefix("https://dl.flathub.org/repo/screenshots/") {
                /* Make sure the file exists in the screenshots branch */
                let found = screenshots_file
                    .resolve_relative_path(filename)
                    .query_exists(Cancellable::NONE);

                if !found {
                    not_found_screenshots.push(url.to_owned());
                }
            } else {
                not_mirrored_screenshots.push(url.to_owned());
            }
        }
    }

    let mut diagnostics = vec![];

    if !not_found_screenshots.is_empty() {
        diagnostics.push(ValidationDiagnostic::new(
            DiagnosticInfo::MirroredScreenshotNotFound {
                appstream_path: appstream_path.to_owned(),
                expected_branch: format!("screenshots/{arch}"),
                urls: not_found_screenshots,
            },
            Some(refstring.to_string()),
        ));
    }

    if !not_mirrored_screenshots.is_empty() {
        diagnostics.push(ValidationDiagnostic::new(
            DiagnosticInfo::ScreenshotNotMirrored {
                appstream_path: appstream_path.to_owned(),
                urls: not_mirrored_screenshots,
            },
            Some(refstring.to_string()),
        ));
    }

    Ok(diagnostics)
}

fn run_appstream_validate(
    appstream_content: Vec<u8>,
    refstring: &str,
    appstream_path: &str,
) -> Result<Vec<ValidationDiagnostic>> {
    /* Run the appstream validation tool */
    let appstream_checkout = ref_directory(refstring).join(
        Path::new(appstream_path)
            .file_name()
            .expect("Failed to get file name"),
    );
    std::fs::File::create(&appstream_checkout)?.write_all(&appstream_content)?;

    let output = Command::new("flatpak")
        .args([
            "run",
            "--env=G_DEBUG=fatal-criticals",
            "--command=appstream-util",
            "org.flatpak.Builder",
            "validate",
            "--nonet",
            appstream_checkout.to_str().unwrap(),
        ])
        .output()?;

    if !output.status.success() {
        Ok(vec![ValidationDiagnostic::new(
            DiagnosticInfo::AppstreamValidation {
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                path: appstream_path.to_owned(),
            },
            Some(refstring.to_string()),
        )])
    } else {
        Ok(vec![])
    }
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
    has_local_icon: bool,
) -> Result<Vec<ValidationDiagnostic>> {
    let mut diagnostics = vec![];

    if let Err(e) = check_appstream_component_id(component, refstring) {
        diagnostics.push(ValidationDiagnostic::new_failed_to_load_appstream(
            appstream_path,
            &e,
            refstring,
        ));
    }

    if !has_local_icon {
        let has_remote_icon = component
            .find_all("icon")
            .any(|icon| icon.get_attr("type") == Some("remote"));

        if has_remote_icon {
            /* Just emit a warning */
            diagnostics.push(ValidationDiagnostic::new_warning(
                DiagnosticInfo::NoLocalIcon {
                    appstream_path: appstream_path.to_owned(),
                },
                Some(refstring.to_string()),
            ));
        } else {
            /* No icon at all, this is an error */
            diagnostics.push(ValidationDiagnostic::new(
                DiagnosticInfo::MissingIcon {
                    appstream_path: appstream_path.to_owned(),
                },
                Some(refstring.to_string()),
            ));
        }
    }

    Ok(diagnostics)
}

fn validate_screenshots_ref(
    repo: &Repo,
    refstring: &str,
    checksum: &str,
) -> Result<Vec<ValidationDiagnostic>> {
    let mut unexpected_files = vec![];

    let (ref_files, _checksum) = repo.read_commit(checksum, Cancellable::NONE)?;

    let children =
        ref_files.enumerate_children("standard::", FileQueryInfoFlags::NONE, Cancellable::NONE)?;

    let app_ids = repo
        .list_refs(None, Cancellable::NONE)?
        .keys()
        .filter(|s| is_primary_ref(s))
        .map(|s| app_id_from_ref(s))
        .collect::<Vec<String>>();

    for child in children {
        let child_name = child?.name().to_string_lossy().to_string();
        if !app_ids
            .iter()
            .any(|app_id| child_name.starts_with(&format!("{app_id}-")))
        {
            unexpected_files.push(child_name);
        }
    }

    if unexpected_files.is_empty() {
        Ok(vec![])
    } else {
        Ok(vec![ValidationDiagnostic::new(
            DiagnosticInfo::UnexpectedFilesInScreenshotBranch {
                files: unexpected_files,
            },
            Some(refstring.to_string()),
        )])
    }
}
