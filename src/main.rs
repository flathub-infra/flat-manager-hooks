mod cmd_publish;
mod cmd_review;
mod config;
mod job_utils;
mod storefront;
mod utils;

use std::error::Error;
use std::fs;
use std::{env, path::PathBuf};

use clap::{Parser, Subcommand};
use cmd_publish::PublishArgs;
use cmd_review::ReviewArgs;

#[derive(Parser, Debug)]
struct Args {
    /// Path to the config file. The script is usually run in the build directory, so this needs to be an absolute path.
    #[arg(short, long)]
    config: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Publish(PublishArgs),
    Review(ReviewArgs),
}

fn main() -> Result<(), Box<dyn Error>> {
    // Set up logging, with a default verbosity of "info"
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "info");
    }
    env_logger::init();

    let args = Args::parse();

    let config = serde_json::from_reader(fs::File::open(args.config)?)?;

    match args.command {
        Command::Publish(cmd) => cmd.run(&config),
        Command::Review(cmd) => cmd.run(&config),
    }
}
