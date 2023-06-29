use anyhow::{anyhow, Result};
use elf::{
    abi::{EM_386, EM_AARCH64, EM_X86_64},
    endian::AnyEndian,
    to_str::e_machine_to_string,
    ElfBytes,
};
use ostree::{
    gio::{content_type_guess, Cancellable, File, FileQueryInfoFlags, FileType},
    prelude::{Cast, FileExt, InputStreamExtManual},
    RepoFile,
};

use crate::utils::{arch_from_ref, read_repo_file};

use super::diagnostics::{DiagnosticInfo, ValidationDiagnostic};

pub fn review_files(ref_files: &File, refstring: &str) -> Result<Vec<ValidationDiagnostic>> {
    let mut diagnostics = vec![];

    let files = ref_files.child("files");
    diagnostics.extend(review_directory(&files, refstring)?);

    Ok(diagnostics)
}

fn review_directory(directory: &File, refstring: &str) -> Result<Vec<ValidationDiagnostic>> {
    let mut diagnostics = vec![];

    let children =
        directory.enumerate_children("standard::", FileQueryInfoFlags::NONE, Cancellable::NONE)?;

    for child in children {
        let child = child?;
        let child_file = directory.child(child.name());

        match child.file_type() {
            FileType::Regular => {
                diagnostics.extend(review_file(&child_file, refstring)?);
            }
            FileType::Directory => {
                diagnostics.extend(review_directory(&child_file, refstring)?);
            }
            _ => {}
        }
    }

    Ok(diagnostics)
}

fn review_file(file: &File, refstring: &str) -> Result<Vec<ValidationDiagnostic>> {
    /* Work around https://github.com/ostreedev/ostree/issues/2703 */
    let repo_file: &RepoFile = file.downcast_ref().unwrap();
    let (stream, _, _) = repo_file
        .repo()
        .unwrap()
        .load_file(&repo_file.checksum().unwrap(), Cancellable::NONE)?;
    let stream = stream.unwrap();

    /* Detect content type from the filename and start of file */
    let mut buf = [0; 512];
    let (read, _partial_error) = stream.read_all(&mut buf, Cancellable::NONE)?;
    let (mime_type, _uncertain) = content_type_guess(file.path(), &buf[..read]);

    let diagnostics = match mime_type.as_str() {
        "application/x-executable" | "application/x-sharedlib" => {
            review_executable_file(file, refstring)?
        }
        _ => vec![],
    };

    Ok(diagnostics)
}

fn review_executable_file(file: &File, refstring: &str) -> Result<Vec<ValidationDiagnostic>> {
    let data = read_repo_file(file.downcast_ref().unwrap())?;
    let elf = match ElfBytes::<AnyEndian>::minimal_parse(&data) {
        Ok(elf) => elf,
        // Ignore errors, we'll just skip this file
        Err(_) => return Ok(vec![]),
    };

    let expected_arch = arch_from_ref(refstring);
    let expected_codes = match expected_arch.as_str() {
        "x86_64" => vec![EM_X86_64, EM_386],
        "aarch64" => vec![EM_AARCH64],
        _ => vec![],
    };

    if !expected_codes.iter().any(|x| x == &elf.ehdr.e_machine) {
        return Ok(vec![ValidationDiagnostic::new_warning(
            DiagnosticInfo::WrongArchExecutable {
                path: file
                    .path()
                    .ok_or(anyhow!("expected path"))?
                    .to_string_lossy()
                    .to_string(),
                detected_arch: e_machine_to_string(elf.ehdr.e_machine),
                detected_arch_code: elf.ehdr.e_machine,
            },
            Some(refstring.to_string()),
        )]);
    }

    Ok(vec![])
}
