use anyhow::{Ok, Result};
use clap::Args;

use crate::{
    config::ValidateConfig,
    job_utils::{Build, BuildExtended},
    review::do_validation,
};

#[derive(Args, Debug)]
pub struct ValidateArgs {}

impl ValidateArgs {
    pub fn run(&self) -> Result<()> {
        let (_repo, _refs, result) = do_validation(self)?;

        /* Print the results */
        println!("{}", serde_json::to_string_pretty(&result)?);

        Ok(())
    }
}

impl ValidateConfig for ValidateArgs {
    fn get_is_free_software(&self, _app_id: &str, _license: Option<&str>) -> Result<bool> {
        Ok(false)
    }

    fn get_build(&self) -> Result<BuildExtended> {
        Ok(BuildExtended {
            build: Build {
                build_log_url: None,
            },
            build_refs: vec![],
        })
    }
}
