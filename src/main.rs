#![feature(is_some_and)]

mod cmd_publish;
mod storefront;
mod utils;

use std::env;
use std::error::Error;

use clap::{Parser, Subcommand};
use cmd_publish::PublishArgs;

#[derive(Parser, Debug)]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Publish(PublishArgs),
}

fn main() -> Result<(), Box<dyn Error>> {
    // Set up logging, with a default verbosity of "info"
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "info");
    }
    env_logger::init();

    let args = Args::parse();

    match args.command {
        Command::Publish(cmd) => cmd.run(),
    }
}
