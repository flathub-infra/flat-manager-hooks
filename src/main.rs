mod cmd_publish;
mod cmd_review;
mod cmd_validate;
mod config;
mod job_utils;
mod review;
mod storefront;
mod utils;

use anyhow::Result;
use clap::{Parser, Subcommand};
use cmd_publish::PublishArgs;
use cmd_review::ReviewArgs;
use cmd_validate::ValidateArgs;
use std::env;

#[derive(Parser, Debug)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Publish(PublishArgs),
    Review(ReviewArgs),
    Validate(ValidateArgs),
}

fn main() -> Result<()> {
    // Set up logging, with a default verbosity of "info"
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "info");
    }
    env_logger::init();

    let args = Args::parse();

    match args.command {
        Command::Publish(cmd) => cmd.run(),
        Command::Review(cmd) => cmd.run(),
        Command::Validate(cmd) => cmd.run(),
    }
}
