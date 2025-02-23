use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    path::PathBuf,
};

use anyhow::{anyhow, Result};
use clap::Args;
use elementtree::Element;
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use log::info;
use ostree::{
    gio::{Cancellable, File},
    glib::VariantDict,
    prelude::Cast,
    MutableTree, Repo,
};

use crate::{
    config::{Config, RegularConfig, ValidateConfig},
    job_utils::BuildExtended,
    storefront::StorefrontInfo,
    utils::{app_id_from_ref, mtree_lookup, mtree_lookup_file, read_file_from_repo, Transaction},
};

#[derive(Args, Debug)]
pub struct PublishArgs {
    /// Path to the config file. The script is usually run in the build directory, so this needs to be an absolute path.
    #[arg(short, long)]
    config: PathBuf,
}

impl PublishArgs {
    pub fn run(&self) -> Result<()> {
        let config: RegularConfig = serde_json::from_reader(fs::File::open(self.config.clone())?)?;

        // Open the build repo at the current directory
        let repo = Repo::new(&File::for_path("."));
        repo.open(Cancellable::NONE)?;

        let refs = repo.list_refs(None, Cancellable::NONE)?;

        // Get build info from flat-manager
        let build = if config.get_is_republish()? {
            None
        } else {
            Some(config.get_build()?)
        };

        let mut storefront_infos = HashMap::new();

        // Rewrite each one
        for (refstring, checksum) in refs.into_iter() {
            let refstring = refstring.to_string();

            info!("Rewriting {refstring} ({checksum})");

            let app_id = app_id_from_ref(&refstring);

            let storefront_info = config.get_storefront_info(&app_id)?;
            if !storefront_infos.contains_key(&app_id) {
                storefront_infos.insert(app_id.clone(), storefront_info);
            }
            let storefront_info = storefront_infos.get(&app_id).unwrap();

            rewrite_ref(&repo, storefront_info, &build, &refstring, &checksum)?;
        }

        Ok(())
    }
}

fn rewrite_ref(
    repo: &Repo,
    storefront_info: &StorefrontInfo,
    build: &Option<BuildExtended>,
    refstring: &str,
    checksum: &str,
) -> Result<()> {
    let app_id = app_id_from_ref(refstring);

    let tx = Transaction::new(repo)?;

    // Create a MutableTree so we can edit the commit's files
    let mtree = MutableTree::from_commit(repo, checksum)?;

    rewrite_appstream_file(repo, &mtree, &app_id, storefront_info, build, refstring)?;

    // Write the modified MutableTree to the repository.
    let repo_file = repo.write_mtree(&mtree, Cancellable::NONE)?;

    // Copy the original commit metadata. Leave out extended attributes, that's just the signature, which
    // won't be valid when we rewrite the commit (and flat-manager will sign the resulting commit with its own key
    // anyway)
    let commit_metadata = repo.load_commit(checksum)?.0;
    let metadata = commit_metadata.child_get::<VariantDict>(0);
    let subject = &commit_metadata.child_get::<String>(3);
    let body = &commit_metadata.child_get::<String>(4);
    let time = ostree::commit_get_timestamp(&commit_metadata);
    let parent = ostree::commit_get_parent(&commit_metadata).map(|x| x.to_string());

    rewrite_metadata(&metadata, storefront_info)?;

    // Write a new commit with the new dirtree but (mostly) the same metadata
    let new_checksum = repo
        .write_commit_with_time(
            parent.as_deref(),
            Some(subject),
            Some(body),
            Some(&metadata.end()),
            repo_file.dynamic_cast_ref().unwrap(),
            time,
            Cancellable::NONE,
        )?
        .to_string();

    if checksum == new_checksum {
        info!("No changes to {refstring}");
    } else {
        info!("Rewriting ref {refstring} from {checksum} to {new_checksum}");
        // Update the ref to point to the edited commit
        repo.transaction_set_ref(None, refstring, Some(&new_checksum));
    }

    tx.commit()?;

    Ok(())
}

pub fn rewrite_appstream_file(
    repo: &Repo,
    mtree: &MutableTree,
    app_id: &str,
    storefront_info: &StorefrontInfo,
    build: &Option<BuildExtended>,
    refstring: &str,
) -> Result<()> {
    let appstream_filename = &format!("{app_id}.xml.gz");
    let appstream_file = mtree_lookup_file(
        mtree,
        &["files", "share", "app-info", "xmls", appstream_filename],
    );

    if appstream_file.is_err() {
        return Ok(());
    }

    let appstream_content = read_file_from_repo(repo, &appstream_file.unwrap())?;

    let mut s = String::new();
    GzDecoder::new(&*appstream_content).read_to_string(&mut s)?;

    let new_appstream = rewrite_appstream_xml(storefront_info, refstring, build, &s)?;

    if new_appstream == s {
        // If the appstream contents didn't change, we shouldn't bother rewriting the file
        return Ok(());
    } else {
        let difference = diff::lines(&s, &new_appstream)
            .iter()
            .map(|l| match l {
                diff::Result::Left(l) => format!("-{l}\n"),
                diff::Result::Both(b, _) => format!(" {b}\n"),
                diff::Result::Right(r) => format!("+{r}\n"),
            })
            .collect::<String>();
        info!("Changes to {}: {}", appstream_filename, difference);
    }

    // gzip encode the new appstream file
    let mut s = vec![];
    GzEncoder::new(&mut s, Compression::default()).write_all(new_appstream.as_bytes())?;

    // Write the new appstream file to the repo
    let checksum = repo.write_regfile_inline(None, 0, 0, 0o100644, None, &s, Cancellable::NONE)?;

    // Edit the MutableTree with a reference to the new appstream file
    mtree_lookup(mtree, &["files", "share", "app-info", "xmls"])?
        .1
        .ok_or(anyhow!("file not found"))?
        .replace_file(&format!("{app_id}.xml.gz"), &checksum)?;

    Ok(())
}

pub fn rewrite_appstream_xml(
    storefront_info: &StorefrontInfo,
    refstring: &str,
    build: &Option<BuildExtended>,
    original_appstream: &str,
) -> Result<String> {
    let mut changed = false;

    let mut root = Element::from_reader(original_appstream.as_bytes())?;

    let mut components: Vec<_> = root.children_mut().collect();
    if components.len() != 1 {
        return Err(anyhow!(
            "Expected exactly 1 <component> tag, found {}",
            components.len()
        ));
    }

    let component = &mut components[0];

    // Delete all existing "flathub::" keys
    for metadata_tag in component.find_all_mut("custom") {
        metadata_tag.retain_children(|value: &Element| {
            if let Some(key) = value.get_attr("key") {
                if key.to_lowercase().starts_with("flathub::") {
                    #[allow(clippy::if_same_then_else)]
                    if key.to_lowercase() == "flathub::manifest" {
                        /* Preserve the flathub::manifest key, it is allowed to be set by upstream */
                        true
                    } else if key.to_lowercase().starts_with("flathub::build::") && build.is_none()
                    {
                        /* On republishes, preserve the previous build log URL */
                        true
                    } else {
                        changed = true;
                        false
                    }
                } else {
                    true
                }
            } else {
                true
            }
        });
    }

    fn find_element<'a>(
        parent: &'a mut Element,
        tag: &'a str,
        attr: Option<(&'_ str, &'_ str)>,
    ) -> Option<&'a mut Element> {
        let existing = if let Some((key, val)) = attr {
            parent
                .find_all_mut(tag)
                .find(|el| el.get_attr(key) == Some(val))
        } else {
            parent.find_mut(tag)
        };

        existing
    }

    fn find_or_create_element<'a>(
        parent: &'a mut Element,
        tag: &'a str,
        attr: Option<(&'_ str, &'_ str)>,
    ) -> &'a mut Element {
        if find_element(parent, tag, attr).is_some() {
            // running find_element twice is a borrow checker workaround
            find_element(parent, tag, attr).unwrap()
        } else {
            let new_tag = parent.append_new_child(tag);
            new_tag.set_tail("\n  ");
            if let Some((key, val)) = attr {
                new_tag.set_attr(key, val);
            }
            new_tag
        }
    }

    let mut set_value = |key: &str, value: Option<&str>| {
        if let Some(value) = value {
            let custom = find_or_create_element(component, "custom", None);
            find_or_create_element(custom, "value", Some(("key", key))).set_text(value);

            changed = true;
        }
    };

    // Add verification tags
    if let Some(verification) = &storefront_info.verification {
        set_value(
            "flathub::verification::verified",
            Some(if verification.verified {
                "true"
            } else {
                "false"
            }),
        );

        set_value(
            "flathub::verification::timestamp",
            verification.timestamp.as_deref(),
        );
        set_value(
            "flathub::verification::method",
            verification.method.as_deref(),
        );
        set_value(
            "flathub::verification::login_name",
            verification.login_name.as_deref(),
        );
        set_value(
            "flathub::verification::login_provider",
            verification.login_provider.as_deref(),
        );
        set_value(
            "flathub::verification::website",
            verification.website.as_deref(),
        );
        set_value(
            "flathub::verification::login_is_organization",
            Some(if verification.login_is_organization.is_some() {
                "true"
            } else {
                "false"
            }),
        );
    }

    // Add pricing tags
    if let Some(pricing) = &storefront_info.pricing {
        set_value(
            "flathub::pricing::recommended_donation",
            pricing
                .recommended_donation
                .map(|x| x.to_string())
                .as_deref(),
        );
        set_value(
            "flathub::pricing::minimum_payment",
            pricing.minimum_payment.map(|x| x.to_string()).as_deref(),
        );
    }

    // Add build log tags
    if let Some(build) = build {
        if let Some(build_log_url) = &build.build.build_log_url {
            set_value(
                "flathub::build::build_log_url",
                Some(build_log_url.as_str()),
            );
        }
        if let Some(build_ref_log_url) = &build
            .build_refs
            .iter()
            .find(|x| x.ref_name == refstring)
            .and_then(|x| x.build_log_url.as_ref())
        {
            set_value(
                "flathub::build::build_ref_log_url",
                Some(build_ref_log_url.as_str()),
            );
        }
    }

    if changed {
        Ok(root.to_string()?)
    } else {
        Ok(original_appstream.to_string())
    }
}

/// Edits a commit's metadata according to the given storefront info.
pub fn rewrite_metadata(metadata: &VariantDict, storefront_info: &StorefrontInfo) -> Result<()> {
    let subsets = list_subsets(storefront_info);

    if subsets.is_empty() {
        metadata.remove("xa.subsets");
    } else {
        info!("Setting subsets: {}", subsets.join(", "));
        metadata.insert("xa.subsets", &subsets);
    }

    let is_paid = storefront_info
        .pricing
        .as_ref()
        .map(|pricing| {
            pricing.recommended_donation.is_some_and(|x| x > 0)
                || pricing.minimum_payment.is_some_and(|x| x > 0)
        })
        .unwrap_or(false);

    if is_paid {
        info!("Setting token type to 1");
        metadata.insert("xa.token-type", 1_i32.to_le());
    } else {
        metadata.remove("xa.token-type");
    }

    Ok(())
}

/// Lists all the subsets that we should add to a commit, based on the given storefront info.
fn list_subsets(storefront_info: &StorefrontInfo) -> Vec<String> {
    let mut subsets = vec![];

    let verified = storefront_info
        .verification
        .as_ref()
        .is_some_and(|x| x.verified);

    let floss = storefront_info.is_free_software.is_some_and(|x| x);

    if verified {
        subsets.push("verified".to_string());
    }
    if floss {
        subsets.push("floss".to_string());
    }
    if verified && floss {
        subsets.push("verified_floss".to_string());
    }

    subsets
}

#[cfg(test)]
mod tests {
    use crate::{
        job_utils::{Build, BuildRef},
        storefront::{PricingInfo, VerificationInfo},
    };

    use super::*;

    fn assert_eq_ignore_space(a: &str, b: &str) {
        assert_eq!(a.replace([' ', '\n'], ""), b.replace([' ', '\n'], ""))
    }

    #[test]
    fn test_list_subsets_1() {
        let storefront_info = StorefrontInfo {
            verification: Some(VerificationInfo {
                verified: true,
                ..Default::default()
            }),
            pricing: None,
            is_free_software: Some(true),
        };
        let subsets = list_subsets(&storefront_info);

        assert_eq!(vec!["verified", "floss", "verified_floss"], subsets);
    }

    #[test]
    fn test_list_subsets_2() {
        let storefront_info = StorefrontInfo {
            verification: None,
            pricing: None,
            is_free_software: Some(false),
        };
        let subsets = list_subsets(&storefront_info);

        assert!(subsets.is_empty());
    }

    #[test]
    fn test_rewrite_appstream_xml_1() {
        let original_appstream = r#"<?xml version="1.0" encoding="utf-8"?>
<components>
    <component>
        <id>org.flatpak.Test</id>
    </component>
</components>"#;

        let storefront_info = StorefrontInfo {
            verification: Some(VerificationInfo {
                verified: true,
                timestamp: Some("2023-01-01T00:00:00".to_string()),
                method: Some("website".to_string()),
                website: Some("example.com".to_string()),
                ..Default::default()
            }),
            pricing: None,
            is_free_software: None,
        };

        let result = rewrite_appstream_xml(
            &storefront_info,
            "app/org.flatpak.Test/x86_64/stable",
            &Some(BuildExtended {
                build: Build {
                    app_id: None,
                    repo: "".to_owned(),
                    build_log_url: Some("https://example.com".to_string()),
                },
                build_refs: vec![],
            }),
            original_appstream,
        )
        .unwrap();

        assert_eq_ignore_space(
            &result,
            r#"<?xml version="1.0" encoding="utf-8"?><components>
<component>
    <id>org.flatpak.Test</id>
    <custom>
        <value key="flathub::verification::verified">true</value>
        <value key="flathub::verification::timestamp">2023-01-01T00:00:00</value>
        <value key="flathub::verification::method">website</value>
        <value key="flathub::verification::website">example.com</value>
        <value key="flathub::verification::login_is_organization">false</value>
        <value key="flathub::build::build_log_url">https://example.com</value>
    </custom>
</component>
</components>"#,
        )
    }

    #[test]
    fn test_rewrite_appstream_xml_2() {
        let original_appstream = r#"<?xml version="1.0" encoding="utf-8"?>
<components>
    <component>
        <id>org.flatpak.Test</id>
    </component>
</components>"#;

        let storefront_info = StorefrontInfo {
            verification: None,
            pricing: Some(PricingInfo {
                minimum_payment: None,
                recommended_donation: Some(1),
            }),
            is_free_software: None,
        };

        let result = rewrite_appstream_xml(
            &storefront_info,
            "app/org.flatpak.Test/x86_64/stable",
            &Some(BuildExtended {
                build: Build {
                    app_id: None,
                    repo: "".to_owned(),
                    build_log_url: None,
                },
                build_refs: vec![
                    BuildRef {
                        ref_name: "app/org.flatpak.Test.Locale/x86_64/stable".to_string(),
                        build_log_url: Some("https://example.com/wrong".to_string()),
                    },
                    BuildRef {
                        ref_name: "app/org.flatpak.Test/x86_64/stable".to_string(),
                        build_log_url: Some("https://example.com".to_string()),
                    },
                ],
            }),
            original_appstream,
        )
        .unwrap();

        assert_eq_ignore_space(
            &result,
            r#"<?xml version="1.0" encoding="utf-8"?><components>
<component>
    <id>org.flatpak.Test</id>
    <custom>
        <value key="flathub::pricing::recommended_donation">1</value>
        <value key="flathub::build::build_ref_log_url">https://example.com</value>
    </custom>
</component>
</components>"#,
        )
    }

    #[test]
    fn test_rewrite_appstream_xml_removes_old_tags() {
        let original_appstream = r#"<?xml version="1.0" encoding="utf-8"?>
<components>
    <component>
        <id>org.flatpak.Test</id>
        <custom>
            <value key="flathub::pricing::recommended_donation">1</value>
            <value key="flathub::build_log_url">https://example.com</value>
        </custom>
    </component>
</components>"#;

        let storefront_info = StorefrontInfo {
            verification: None,
            pricing: Some(PricingInfo {
                minimum_payment: Some(2),
                recommended_donation: None,
            }),
            is_free_software: None,
        };

        let result = rewrite_appstream_xml(
            &storefront_info,
            "app/org.flatpak.Test/x86_64/master",
            &Some(BuildExtended {
                build: Build {
                    app_id: None,
                    repo: "".to_owned(),
                    build_log_url: None,
                },
                build_refs: vec![],
            }),
            original_appstream,
        )
        .unwrap();

        assert_eq_ignore_space(
            &result,
            r#"<?xml version="1.0" encoding="utf-8"?><components>
<component>
    <id>org.flatpak.Test</id>
    <custom>
        <value key="flathub::pricing::minimum_payment">2</value>
    </custom>
</component>
</components>"#,
        )
    }
}
