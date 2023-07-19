use anyhow::{Ok, Result};
use clap::Args;

use crate::{config::Config, review::do_validation};

#[derive(Args, Debug)]
pub struct ValidateArgs {}

impl ValidateArgs {
    pub fn run<C: Config>(&self, config: &C) -> Result<()> {
        let (_repo, _refs, result) = do_validation(config)?;

        /* Print the results */
        println!("{}", serde_json::to_string_pretty(&result)?);

        Ok(())
    }
}
