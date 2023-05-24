use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::process::Command;

use anyhow::Result;
use elementtree::Element;
use ostree::gio::Cancellable;
use ostree::prelude::FileExt;
use ostree::Repo;

use crate::utils::{
    app_id_from_ref, get_appstream_path, is_primary_ref, load_appstream, ref_directory,
};

use super::diagnostics::{CheckResult, DiagnosticInfo, ValidationDiagnostic};

/// Run all of the validations on a build.
pub fn validate_build(
    repo: &Repo,
    refs: &HashMap<String, String>,
    result: &mut CheckResult,
) -> Result<()> {
    for (refstring, checksum) in refs.iter() {
        if is_primary_ref(refstring) {
            result
                .diagnostics
                .extend(validate_primary_ref(repo, refstring, checksum)?);
        }
    }

    Ok(())
}

/// Run all the validations on a "primary" ref (app, runtime, or extension).
pub fn validate_primary_ref(
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
        repo,
        checksum,
        refstring,
        has_local_icon,
    )?);

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

    let appstream_content = match appstream_file.load_contents(Cancellable::NONE) {
        Ok(content) => content.0,
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

fn validate_appstream_catalog_file(
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

    /* For now, we don't run `appstream-util validate` or `appstreamcli validate` on this file, because it sometimes
    produces false positives. */

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
