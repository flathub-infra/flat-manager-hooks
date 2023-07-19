use std::{fs, path::PathBuf};

use anyhow::Result;
use clap::Args;

use crate::{config::RegularConfig, review::do_review};

#[derive(Args, Debug)]
pub struct ReviewArgs {
    /// Path to the config file. The script is usually run in the build directory, so this needs to be an absolute path.
    #[arg(short, long)]
    config: PathBuf,
}

impl ReviewArgs {
    pub fn run(&self) -> Result<()> {
        let config: RegularConfig = serde_json::from_reader(fs::File::open(self.config.clone())?)?;
        do_review(&config)
    }
}
