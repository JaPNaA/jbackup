mod arguments;
mod file_structure;
mod io_util;
mod subcommand;
mod tab_separated_key_value;

use std::{
    env::{self, Args},
    process::ExitCode,
};

pub const JBACKUP_PATH: &str = "./.jbackup";
pub const SNAPSHOTS_PATH: &str = "./.jbackup/snapshots";
pub const BRANCHES_PATH: &str = "./.jbackup/branches";
pub const HEAD_PATH: &str = "./.jbackup/head";

const HELP_TEXT: &str = "
Subcommands
---

init
  Initializes a repository for jbackup in the current working directory.

snapshot
  Creates a snapshot of the current files in the repository.

  Options:
    -m <message>
      Supply a message to annotate the snapshot.

log
  View all snapshots in the repository.

help
  Lists available commands.
";

fn main() -> ExitCode {
    let mut args_iter = env::args();
    args_iter.next(); // ignore path

    let result = run_with_arguments(args_iter);

    match result {
        Err(error) => {
            println!("Fatal: {}", error);
            ExitCode::FAILURE
        }
        Ok(_) => ExitCode::SUCCESS,
    }
}

fn run_with_arguments(args_iter: Args) -> Result<(), String> {
    let mut args = arguments::Parser::new().flag("--help").parse(args_iter);

    if args.flags.contains("--help") {
        println!("{}", HELP_TEXT);
        return Ok(());
    }

    let command = args.normal.pop_front().unwrap_or_default();

    match command.as_str() {
        "" | "help" => {
            println!("{}", HELP_TEXT);
            Ok(())
        }
        "init" => match subcommand::init::main() {
            Err(error) => Err(format!("Failed to initalize repository: {error}")),
            Ok(_) => Ok(()),
        },
        "snapshot" => match subcommand::snapshot::main(args.normal) {
            Err(error) => Err(format!("Failed to snapshot repository: {error}")),
            Ok(_) => Ok(()),
        },
        "log" => match subcommand::log::main() {
            Err(error) => Err(format!("Failed to get logs: {error}")),
            Ok(_) => Ok(()),
        },
        // todo: remove this command
        // this command allows restoring of a snapshot.
        // data will be stored in the "./.jbackup/_debug" directory.
        "__debug_restore" => match subcommand::__debug_restore::main(args.normal) {
            Err(err) => Err(format!("Failed to restore: {err}")),
            Ok(_) => Ok(()),
        },
        _ => Err(format!("Error: unknown command '{}'", command)),
    }
}
