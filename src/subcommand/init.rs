use std::{
    collections::{HashMap, VecDeque},
    fs,
};

use crate::{
    JBACKUP_PATH, arguments, file_structure, io_util::simplify_result, transformer::get_transformer,
};

/// The init command creates a .jbackup directory in the current working
/// directory, if one doesn't already exist.
///
/// The .jbackup directory should contain the files: 'branches', 'head', 'config'.
pub fn main(mut args: VecDeque<String>) -> Result<(), String> {
    let mut parsed_args = arguments::Parser::new()
        .option("--transformer")
        .parse(args.drain(..));

    let mut transformers = Vec::new();

    if let Some(transformer) = parsed_args.options.remove("--transformer") {
        if let Some(_) = get_transformer(&transformer) {
            transformers.push(transformer);
        } else {
            return Err(String::from("Invalid transformer: '") + &transformer + "'");
        }
    }

    simplify_result(fs::create_dir(JBACKUP_PATH))?;

    file_structure::BranchesFile {
        branches: HashMap::new(),
    }
    .write()?;

    file_structure::HeadFile {
        curr_snapshot_id: None,
        curr_branch: String::from("main"),
    }
    .write()?;

    file_structure::ConfigFile { transformers }.write()?;

    println!("Successfully initalized jbackup in the current working directory.");
    Ok(())
}
