use anyhow::Result;
use clap::Args;

use crate::{config::Config, review::do_review};

#[derive(Args, Debug)]
pub struct ReviewArgs {}

impl ReviewArgs {
    pub fn run(&self, config: &Config) -> Result<()> {
        do_review(config)
    }
}
