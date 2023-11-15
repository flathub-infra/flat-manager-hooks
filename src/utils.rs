use std::io::Read;

use anyhow::{anyhow, Result};
use elementtree::Element;
use flate2::read::GzDecoder;
use log::info;
use ostree::{
    gio::Cancellable,
    glib,
    glib::GString,
    prelude::{Cast, FileExt, InputStreamExtManual},
    MutableTree, Repo, RepoFile,
};

pub fn arch_from_ref(refstring: &str) -> String {
    refstring.split('/').nth(2).unwrap().to_string()
}

pub const APP_SUFFIXES: [&str; 3] = ["Sources", "Debug", "Locale"];
pub const APPID_SKIPLIST: [&str; 8] = [
    "net.pcsx2.PCSX2",
    "net.wz2100.wz2100",
    "org.freedesktop.Platform.ClInfo",
    "org.freedesktop.Platform.GlxInfo",
    "org.freedesktop.Platform.VaInfo",
    "org.freedesktop.Platform.VulkanInfo",
    "org.mozilla.Thunderbird",
    "org.mozilla.firefox",
];

pub fn app_id_from_ref(refstring: &str) -> String {
    let ref_id = refstring.split('/').nth(1).unwrap().to_string();
    let id_parts: Vec<&str> = ref_id.split('.').collect();

    if APP_SUFFIXES.contains(id_parts.last().unwrap()) {
        id_parts[..id_parts.len() - 1].to_vec().join(".")
    } else {
        ref_id
    }
}

/// Determines whether the refstring is either an app or extension (as opposed to a Sources/Debug/Locales ref, or
/// something else like the branch we store screenshots in).
pub fn is_primary_ref(refstring: &str) -> bool {
    if refstring.starts_with("app/") {
        let appid = refstring.split('/').nth(1).unwrap().to_string();
        !APPID_SKIPLIST.contains(&appid.as_str())
    } else {
        false
    }
}

pub fn mtree_lookup(
    mtree: &MutableTree,
    path: &[&str],
) -> Result<(Option<GString>, Option<MutableTree>)> {
    match path {
        [file] => mtree.lookup(file).map_err(Into::into),
        [subdir, rest @ ..] => mtree_lookup(
            &mtree
                .lookup(subdir)?
                .1
                .ok_or(anyhow!("subdirectory not found"))?,
            rest,
        ),
        [] => Err(anyhow!("no path given")),
    }
}

pub fn mtree_lookup_file(mtree: &MutableTree, path: &[&str]) -> Result<GString> {
    mtree_lookup(mtree, path)?
        .0
        .ok_or(anyhow!("file not found"))
}

pub fn read_file_from_repo(repo: &Repo, file_checksum: &str) -> Result<Vec<u8>> {
    let (appstream_file, fileinfo, _) = repo.load_file(file_checksum, Cancellable::NONE)?;
    let appstream_file = appstream_file.unwrap();

    let mut buffer = vec![0; fileinfo.size() as usize];
    appstream_file.read_all(&mut buffer, Cancellable::NONE)?;

    Ok(buffer)
}

pub fn read_repo_file(file: &RepoFile) -> Result<Vec<u8>> {
    if !file.query_exists(Cancellable::NONE) {
        return Err(anyhow!("File does not exist"));
    }

    read_file_from_repo(&file.repo(), &file.checksum())
}

pub fn get_appstream_path(app_id: &str) -> String {
    format!("files/share/app-info/xmls/{app_id}.xml.gz")
}

/// Loads the appstream file from the given commit. Returns the file contents and the parsed XML.
pub fn load_appstream(repo: &Repo, app_id: &str, checksum: &str) -> Result<(String, Element)> {
    let (file, _checksum) = repo.read_commit(checksum, Cancellable::NONE)?;

    let appstream_path = get_appstream_path(app_id);
    let appstream_file = file.resolve_relative_path(&appstream_path);
    let appstream_content = read_repo_file(appstream_file.downcast_ref().unwrap())?;

    let content = if appstream_path.ends_with(".gz") {
        let mut s = String::new();
        GzDecoder::new(&*appstream_content).read_to_string(&mut s)?;
        s
    } else {
        String::from_utf8(appstream_content)?
    };

    let root = Element::from_reader(content.as_bytes())?;

    Ok((content, root))
}

/// Wrapper for OSTree transactions that automatically aborts the transaction when dropped if it hasn't been committed.
pub struct Transaction<'a> {
    repo: &'a Repo,
    finished: bool,
}

impl<'a> Transaction<'a> {
    pub fn new(repo: &'a Repo) -> Result<Self, glib::Error> {
        repo.prepare_transaction(Cancellable::NONE)?;
        Ok(Self {
            repo,
            finished: false,
        })
    }

    pub fn commit(mut self) -> Result<(), glib::Error> {
        self.repo.commit_transaction(Cancellable::NONE)?;
        self.finished = true;
        Ok(())
    }
}

impl Drop for Transaction<'_> {
    fn drop(&mut self) {
        if !self.finished {
            self.repo
                .abort_transaction(Cancellable::NONE)
                .expect("Aborting the transaction should not fail");
        }
    }
}

/// Try the given retry function up to `retry_count + 1` times. The first successful result is returned, or the last error if all attempts failed.
pub fn retry<T, E: std::fmt::Display, F: Fn() -> Result<T, E>>(f: F) -> Result<T, E> {
    let mut i = 0;

    let retry_count = 5;
    let mut wait_time = 1;

    loop {
        match f() {
            Ok(info) => return Ok(info),
            Err(e) => {
                info!("{}", e);
                i += 1;
                if i > retry_count {
                    return Err(e);
                }
                info!("Retrying ({i}/{retry_count}) in {wait_time} seconds...");
                std::thread::sleep(std::time::Duration::from_secs(wait_time));
                wait_time *= 2;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_id_from_refstring() {
        assert_eq!(
            app_id_from_ref("app/org.gnome.Builder/x86_64/stable"),
            "org.gnome.Builder"
        );
        assert_eq!(
            app_id_from_ref("runtime/org.gnome.Builder.Sources/x86_64/stable"),
            "org.gnome.Builder"
        );
        assert_eq!(
            app_id_from_ref("runtime/org.gnome.Builder.Debug/x86_64/stable"),
            "org.gnome.Builder"
        );
        assert_eq!(
            app_id_from_ref("runtime/org.gnome.Builder.Locale/x86_64/stable"),
            "org.gnome.Builder"
        );
        assert_eq!(
            app_id_from_ref("runtime/org.gnome.Platform/x86_64/3.38"),
            "org.gnome.Platform"
        );
    }

    #[test]
    fn test_is_primary_ref() {
        assert!(is_primary_ref("app/org.gnome.Builder/x86_64/stable"));
        assert!(!is_primary_ref("runtime/org.gnome.Platform/x86_64/3.38"));
        assert!(!is_primary_ref(
            "runtime/org.gnome.Builder.Sources/x86_64/stable"
        ));
    }
}
