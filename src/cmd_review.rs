use std::{collections::HashMap, io::Read};

use anyhow::{anyhow, Result};
use clap::Args;
use elementtree::Element;
use flate2::read::GzDecoder;
use log::info;
use ostree::{
    gio::{Cancellable, File},
    prelude::FileExt,
    Repo,
};
use serde::{Deserialize, Serialize};

use crate::{
    config::Config,
    job_utils::require_review,
    utils::{app_id_from_ref, arch_from_ref, get_build_id, get_job_id, retry},
};

#[derive(Args, Debug)]
pub struct ReviewArgs {}

impl ReviewArgs {
    pub fn run(&self, config: &Config) -> Result<()> {
        /* Open the build repo at the current directory */
        let repo = Repo::new(&File::for_path("."));
        repo.open(Cancellable::NONE)?;

        let refs = repo.list_refs(None, Cancellable::NONE)?;
        let mut request = ReviewRequest {
            build_id: get_build_id()?,
            job_id: get_job_id()?,
            app_metadata: HashMap::new(),
        };

        info!("Refs present in build: {:?}", refs.keys());

        for (refstring, checksum) in refs.iter() {
            /* Upload metadata to the backend to check for changes */
            if refstring.starts_with("app/") || refstring.starts_with("runtime/") {
                let arch = arch_from_ref(refstring);

                /* For now, only review the x86_64 upload, since that's the one we show on the website */
                if arch == "x86_64" {
                    match get_app_metadata(&repo, refstring, checksum) {
                        Ok(metadata) => {
                            request
                                .app_metadata
                                .insert(app_id_from_ref(refstring), metadata);
                        }
                        Err(e) => {
                            info!("Failed to get app metadata for {}: {}", refstring, e)
                        }
                    }
                }
            }
        }

        submit_review_request(request, config)?;

        Ok(())
    }
}

fn get_app_metadata(repo: &Repo, refstring: &str, checksum: &str) -> Result<ReviewItem> {
    let app_id = app_id_from_ref(refstring);

    let root = load_appstream(repo, &app_id, checksum)?;
    let component = root
        .find("component")
        .ok_or(anyhow!("No <component> in appstream"))?;

    Ok(ReviewItem {
        name: component.find("name").map(|f| f.text().to_string()),
        summary: component.find("summary").map(|f| f.text().to_string()),
        developer_name: component
            .find("developer_name")
            .map(|f| f.text().to_string()),
        project_license: component
            .find("project_license")
            .map(|f| f.text().to_string()),
        project_group: component
            .find("project_group")
            .map(|f| f.text().to_string()),
        compulsory_for_desktop: component
            .find("compulsory_for_desktop")
            .map(|f| f.text().to_string()),
    })
}

fn load_appstream(repo: &Repo, app_id: &str, checksum: &str) -> Result<Element> {
    let (file, _checksum) = repo.read_commit(checksum, Cancellable::NONE)?;

    let appstream_path = format!("files/share/app-info/xmls/{app_id}.xml.gz");
    let appstream_file = file.resolve_relative_path(&appstream_path);
    let (appstream_content, _etag) = appstream_file
        .load_contents(Cancellable::NONE)
        .map_err(|e| anyhow!("Failed to load the appstream file at {appstream_path}: {e}"))?;

    let mut s = String::new();
    GzDecoder::new(&*appstream_content).read_to_string(&mut s)?;

    let root = Element::from_reader(s.as_bytes())
        .map_err(|e| anyhow!("Failed to parse the appstream file at {appstream_path}: {e}"))?;

    Ok(root)
}

fn submit_review_request(request: ReviewRequest, config: &Config) -> Result<()> {
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
            config,
        )?;
    }

    Ok(())
}

#[derive(Debug, Serialize)]
struct ReviewItem {
    name: Option<String>,
    summary: Option<String>,
    developer_name: Option<String>,
    project_license: Option<String>,
    project_group: Option<String>,
    compulsory_for_desktop: Option<String>,
}

#[derive(Debug, Serialize)]
struct ReviewRequest {
    build_id: i64,
    job_id: i64,
    app_metadata: HashMap<String, ReviewItem>,
}

#[derive(Deserialize)]
struct ReviewRequestResponse {
    requires_review: bool,
}
