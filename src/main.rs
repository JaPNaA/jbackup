mod tab_separated_key_value;
mod util;

use std::{
    collections::HashMap,
    env, fs,
    io::{self, ErrorKind},
    process::{self, Stdio},
    time::SystemTime,
};
use util::simplify_result;

fn main() {
    let mut args_iter = env::args();
    args_iter.next(); // ignore path

    let command = args_iter.next().unwrap_or_default();

    let result = match command.as_str() {
        "" => Err(String::from("Error: no command specified")),
        "init" => match init_repo() {
            Err(error) => Err(format!("Failed to initalize repository: {error}")),
            Ok(_) => Ok(()),
        },
        "snapshot" => match snapshot_repo() {
            Err(error) => Err(format!("Failed to snapshot repository: {error}")),
            Ok(_) => Ok(()),
        },
        _ => Err(format!("Error: unknown command '{}'", command)),
    };

    match result {
        Err(error) => {
            println!("Fatal: {}", error);
        }
        Ok(_) => (),
    }
}

fn init_repo() -> Result<(), String> {
    simplify_result(fs::create_dir(".jbackup"))?;

    BranchesFile {
        branches: HashMap::new(),
    }.write()?;

    HeadFile {
        curr_snapshot_id: None,
        curr_branch: String::from("main"),
    }.write()?;

    println!("Successfully initalized jbackup in the current working directory.");
    Ok(())
}

fn snapshot_repo() -> Result<(), String> {
    if !simplify_result(is_jbackup_in_working_dir())? {
        return Err(String::from(
            "Error: jbackup not found in current working directory. (To make a new backup for this directory, do 'jbackup init')",
        ));
    }

    let new_id = create_full_snapshot()?;
    print!("Created snapshot with id: {}", &new_id);

    let mut head_file = read_head()?;
    let mut branch_file = read_branches()?;

    let head_tar_path = get_head_tar(&head_file)?;

    match head_tar_path {
        None => {
            head_file.curr_snapshot_id = Some(new_id.clone());
            branch_file
                .branches
                .insert(head_file.curr_branch.clone(), new_id);
        }
        Some(p) => {
            todo!();
        }
    }

    head_file.write()?;
    branch_file.write()?;

    Ok(())
}

fn is_jbackup_in_working_dir() -> io::Result<bool> {
    match fs::read_dir("./.jbackup") {
        Err(err) => match err.kind() {
            ErrorKind::NotFound => Ok(false),
            ErrorKind::NotADirectory => Ok(false),
            _ => Err(err),
        },
        Ok(result) => {
            let mut found_branches = false;
            let mut found_head = false;

            for item in result {
                match item.ok() {
                    None => {}
                    Some(entry) => match entry.file_name().into_string() {
                        Ok(s) => match s.as_str() {
                            "branches" => found_branches = true,
                            "head" => found_head = true,
                            _ => {}
                        },
                        Err(_) => {}
                    },
                }
            }

            if found_branches && found_head {
                Ok(true)
            } else {
                println!(
                    "Warning: found .jbackup directory, but some files were missing. The directory may be corrupted. Consider removing '.jbackup' (this will discard your backups!)"
                );
                Ok(false)
            }
        }
    }
}

/// Creates a `tar` of the current working directly, excluding "./.jbackup".
/// The `tar` is placed in the returned path.
fn create_full_snapshot() -> Result<String, String> {
    let tmp_snapshot_path = create_tmp_snapshot()?;
    let md5 = calc_md5(&tmp_snapshot_path)?;
    let timestamp = match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(n) => n.as_secs(),
        Err(_) => 0,
    };

    let id: String = timestamp.to_string() + "-" + &md5;

    let snapshot_metadata = SnapshotMetaFile {
        id: id.clone(),
        snapshot_type: SnapshotType::Full,
        date: timestamp,
        message: None,
        children: Vec::new(),
        parents: Vec::new(),
    };

    commit_tmp_snapshot(&tmp_snapshot_path, snapshot_metadata)?;

    Ok(id)
}

/// Creates a `tar` of the current working directly, excluding "./.jbackup".
/// The `tar` is placed in the returned path.
fn create_tmp_snapshot() -> Result<String, String> {
    let output_path = String::from("./.jbackup/tmp_snapshot.tar");
    let spawn_result = process::Command::new("tar")
        .arg("--exclude=./.jbackup")
        .arg("-cf")
        .arg(&output_path)
        .arg(".")
        .spawn();

    let mut proc = simplify_result(spawn_result)?;
    simplify_result(proc.wait())?;

    Ok(output_path)
}

fn calc_md5(file_path: &str) -> Result<String, String> {
    let output_result = process::Command::new("md5sum")
        .arg(file_path)
        .stdout(Stdio::piped())
        .output();
    let output = simplify_result(output_result)?;

    if output.status.success() {
        let output_str = simplify_result(String::from_utf8(output.stdout))?;
        match output_str.find(' ') {
            Some(index) => Ok(String::from(&output_str[..index])),
            None => Err(String::from(
                "md5sum did not output in the expected format.",
            )),
        }
    } else {
        let stdout_str = simplify_result(String::from_utf8(output.stdout))?;
        let stderr_str = simplify_result(String::from_utf8(output.stderr))?;

        eprintln!("Stdout from md5sum:\n{}", stdout_str);
        eprintln!("Stderr from md5sum:\n{}", stderr_str);
        Err(String::from("Failed to calculate md5 sum."))
    }
}

fn commit_tmp_snapshot(tmp_snapshot_path: &str, data: SnapshotMetaFile) -> Result<(), String> {
    ensure_snapshots_directory_exists()?;

    let snapshot_path = String::from("./.jbackup/snapshots/") + &data.id;

    simplify_result(fs::rename(
        tmp_snapshot_path,
        String::clone(&snapshot_path) + "-" + data.snapshot_type.to_string().as_str(),
    ))?;
    simplify_result(fs::write(snapshot_path + ".meta", data.serialize()?))?;
    Ok(())
}

/// Checks if "./.jbackup/snapshots" exists, otherwise, creates the directory
fn ensure_snapshots_directory_exists() -> Result<(), String> {
    match fs::read_dir("./.jbackup/snapshots") {
        Err(err) => match err.kind() {
            ErrorKind::NotFound => simplify_result(fs::create_dir("./.jbackup/snapshots")),
            ErrorKind::NotADirectory => Err(String::from(
                "Expected ./.jbackup/snapshots to be a directory",
            )),
            _ => simplify_result(Err(err)),
        },
        Ok(_) => Ok(()),
    }
}

/// Retrieves the tar file for HEAD and returns the path
fn get_head_tar(head_file: &HeadFile) -> Result<Option<String>, String> {
    Ok(None)
    // head_file.curr_snapshot_id
}

struct BranchesFile {
    branches: HashMap<String, String>,
}

struct HeadFile {
    curr_snapshot_id: Option<String>,
    curr_branch: String,
}

struct SnapshotMetaFile {
    id: String,
    snapshot_type: SnapshotType,
    date: u64,
    message: Option<String>,
    children: Vec<String>,
    parents: Vec<String>,
}

enum SnapshotType {
    Full,
    FullCompressed,
    Forward,
    Backward,
}

impl BranchesFile {
    fn write(self) -> Result<(), String> {
        tab_separated_key_value::Contents {
            multi_value: HashMap::new(),
            single_value: self.branches,
        }
        .write_file(".jbackup/branches")
    }
}

fn read_branches() -> Result<BranchesFile, String> {
    let contents =
        tab_separated_key_value::Config::single_value_only().read_file("./.jbackup/branches")?;
    Ok(BranchesFile {
        branches: contents.single_value,
    })
}

impl HeadFile {
    fn write(self) -> Result<(), String> {
        tab_separated_key_value::Contents {
            multi_value: HashMap::new(),
            single_value: {
                let mut m = HashMap::new();
                self.curr_snapshot_id
                    .map(|s| m.insert(String::from("snapshotid"), s));
                m.insert(String::from("branch"), self.curr_branch);
                m
            },
        }
        .write_file(".jbackup/head")
    }
}

fn read_head() -> Result<HeadFile, String> {
    let map = tab_separated_key_value::Config::single_value_only().read_file("./.jbackup/head")?;
    let curr_snapshot_id = map.single_value.get("snapshotid");
    let curr_branch = map.single_value.get("branch");
    if curr_branch.is_none() {
        return Err(String::from(
            "The head file is missing required values (snapshotid, branch)",
        ));
    }

    Ok(HeadFile {
        curr_snapshot_id: curr_snapshot_id.map(|s| s.clone()),
        curr_branch: curr_branch
            .expect("branch should have been validated to have a value")
            .clone(),
    })
}

impl SnapshotMetaFile {
    fn serialize(self) -> Result<String, String> {
        tab_separated_key_value::Contents {
            single_value: {
                let mut m = HashMap::new();
                m.insert(String::from("type"), self.snapshot_type.to_string());
                m.insert(String::from("date"), self.date.to_string());
                self.message.map(|s| m.insert(String::from("message"), s));
                m
            },
            multi_value: {
                let mut m = HashMap::new();
                m.insert(String::from("parent"), self.parents);
                m.insert(String::from("child"), self.children);
                m
            },
        }
        .write_string()
    }
}

impl ToString for SnapshotType {
    fn to_string(&self) -> String {
        String::from(match self {
            SnapshotType::Full => "full",
            SnapshotType::FullCompressed => "fullcompressed",
            SnapshotType::Forward => "forward",
            SnapshotType::Backward => "backward",
        })
    }
}
