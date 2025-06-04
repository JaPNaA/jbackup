use std::{collections::HashMap, fs};

use crate::{JBACKUP_PATH, file_structure, io_util::simplify_result};

/// The init command creates a .jbackup directory in the current working
/// directory, if one doesn't already exist.
///
/// The .jbackup directory should contain the 'branches' and 'head' files.
pub fn main() -> Result<(), String> {
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

    println!("Successfully initalized jbackup in the current working directory.");
    Ok(())
}
