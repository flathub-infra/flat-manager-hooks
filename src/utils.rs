use std::error::Error;

use ostree::{
    gio::Cancellable, glib, glib::GString, prelude::InputStreamExtManual, MutableTree, Repo,
};

pub fn app_id_from_ref(refstring: &str) -> String {
    let ref_id = refstring.split('/').nth(1).unwrap().to_string();
    let id_parts: Vec<&str> = ref_id.split('.').collect();

    if ["Sources", "Debug", "Locale"].contains(id_parts.last().unwrap()) {
        id_parts[..id_parts.len() - 1].to_vec().join(".")
    } else {
        ref_id
    }
}

pub fn mtree_lookup(
    mtree: &MutableTree,
    path: &[&str],
) -> Result<(Option<GString>, Option<MutableTree>), Box<dyn Error>> {
    match path {
        [file] => mtree.lookup(file).map_err(Into::into),
        [subdir, rest @ ..] => mtree_lookup(
            &mtree
                .lookup(subdir)?
                .1
                .ok_or_else(|| "subdirectory not found".to_string())?,
            rest,
        ),
        [] => Err("no path given".into()),
    }
}

pub fn mtree_lookup_file(mtree: &MutableTree, path: &[&str]) -> Result<GString, Box<dyn Error>> {
    mtree_lookup(mtree, path)?
        .0
        .ok_or_else(|| "file not found".into())
}

pub fn read_file_from_repo(repo: &Repo, file_checksum: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let (appstream_file, fileinfo, _) = repo.load_file(file_checksum, Cancellable::NONE)?;
    let appstream_file = appstream_file.unwrap();

    let mut buffer = vec![0; fileinfo.size() as usize];
    appstream_file.read_all(&mut buffer, Cancellable::NONE)?;

    Ok(buffer)
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
