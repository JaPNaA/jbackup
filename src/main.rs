mod tab_separated_key_value;
mod util;

use std::{
    collections::{HashMap, HashSet},
    env, fs,
    io::{self, ErrorKind},
    process::{self, ExitCode, Stdio},
    str::FromStr,
    time::SystemTime,
};
use util::simplify_result;

const JBACKUP_PATH: &str = "./.jbackup";
const SNAPSHOTS_PATH: &str = "./.jbackup/snapshots";
const BRANCHES_PATH: &str = "./.jbackup/branches";
const HEAD_PATH: &str = "./.jbackup/head";

const HELP_TEXT: &str = "Subcommands:
init      Initializes a repository for jbackup in the current working
          directory.

snapshot  Creates a snapshot of the current files in the repository.

help      Lists available commands.";

fn main() -> ExitCode {
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
        "help" => {
            println!("{}", HELP_TEXT);
            Ok(())
        }
        _ => Err(format!("Error: unknown command '{}'", command)),
    };

    match result {
        Err(error) => {
            println!("Fatal: {}", error);
            ExitCode::FAILURE
        }
        Ok(_) => ExitCode::SUCCESS,
    }
}

fn init_repo() -> Result<(), String> {
    simplify_result(fs::create_dir(JBACKUP_PATH))?;

    BranchesFile {
        branches: HashMap::new(),
    }
    .write()?;

    HeadFile {
        curr_snapshot_id: None,
        curr_branch: String::from("main"),
    }
    .write()?;

    println!("Successfully initalized jbackup in the current working directory.");
    Ok(())
}

fn snapshot_repo() -> Result<(), String> {
    if !simplify_result(is_jbackup_in_working_dir())? {
        return Err(String::from(
            "Error: jbackup not found in current working directory. (To make a new backup for this directory, do 'jbackup init')",
        ));
    }

    let staged_snapshot = create_full_snapshot()?;
    print!("Created snapshot with id: {}", &staged_snapshot.id);

    let mut head_file = read_head()?;
    let mut branch_file = read_branches()?;

    match &head_file.curr_snapshot_id {
        None => {
            head_file.curr_snapshot_id = Some(staged_snapshot.id.clone());
            branch_file
                .branches
                .insert(head_file.curr_branch.clone(), staged_snapshot.id.clone());
        }
        Some(curr_snapshot_id) => {
            let curr_snapshot_meta = SnapshotMetaFile::read(&curr_snapshot_id)?;
            if curr_snapshot_meta.full_type != SnapshotFullType::Tar {
                todo!("Not implemented: Current snapshot is not a tar snapshot type");
            }

            if staged_snapshot.full_type != SnapshotFullType::Tar {
                todo!("Not implemented: Staged snapshot is not a tar snapshot type");
            }

            create_xdelta(CreateXDeltaArgs {
                from_archive: &staged_snapshot.get_full_payload_filename()?,
                to_archive: &curr_snapshot_meta.get_full_payload_filename()?,
                output_archive: &(curr_snapshot_id.clone() + "-diff-" + &staged_snapshot.id),
            })?
        }
    }

    head_file.write()?;
    branch_file.write()?;

    Ok(())
}

fn is_jbackup_in_working_dir() -> io::Result<bool> {
    match fs::read_dir(JBACKUP_PATH) {
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
fn create_full_snapshot() -> Result<SnapshotMetaFile, String> {
    let tmp_tar_path = create_tmp_tar()?;
    let md5 = calc_md5(&tmp_tar_path)?;
    let timestamp = match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(n) => n.as_secs(),
        Err(_) => 0,
    };

    let id: String = timestamp.to_string() + "-" + &md5;

    let snapshot_metadata = SnapshotMetaFile {
        id: id.clone(),
        full_type: SnapshotFullType::Tar,
        date: timestamp,
        message: None,
        children: Vec::new(),
        parents: Vec::new(),
        diff_children: Vec::new(),
        diff_parents: Vec::new(),
    };

    commit_tmp_snapshot(&tmp_tar_path, &snapshot_metadata)?;

    Ok(snapshot_metadata)
}

/// Creates a `tar` of the current working directly, excluding "./.jbackup".
/// The `tar` is placed in the returned path.
fn create_tmp_tar() -> Result<String, String> {
    let output_path = String::from(JBACKUP_PATH) + "/tmp_snapshot.tar";
    let spawn_result = process::Command::new("tar")
        .arg(String::from("--exclude=") + JBACKUP_PATH)
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

struct CreateXDeltaArgs<'a> {
    from_archive: &'a str,
    to_archive: &'a str,
    output_archive: &'a str,
}

fn create_xdelta(args: CreateXDeltaArgs) -> Result<(), String> {
    let from_path = String::from(SNAPSHOTS_PATH) + "/" + args.from_archive;
    let to_path = String::from(SNAPSHOTS_PATH) + "/" + args.to_archive;
    let output_path = String::from(SNAPSHOTS_PATH) + "/" + args.output_archive;

    let spawn_result = process::Command::new("xdelta")
        .arg("delta")
        .arg(from_path)
        .arg(to_path)
        .arg(output_path)
        .spawn();

    let mut proc = simplify_result(spawn_result)?;
    simplify_result(proc.wait())?;

    Ok(())
}

fn commit_tmp_snapshot(tmp_snapshot_path: &str, data: &SnapshotMetaFile) -> Result<(), String> {
    ensure_snapshots_directory_exists()?;

    let snapshot_path = String::from(SNAPSHOTS_PATH) + "/" + &data.id;

    simplify_result(fs::rename(
        tmp_snapshot_path,
        String::from(SNAPSHOTS_PATH) + "/" + &data.get_full_payload_filename()?,
    ))?;
    simplify_result(fs::write(snapshot_path + ".meta", data.serialize()?))?;
    Ok(())
}

/// Checks if "./.jbackup/snapshots" exists, otherwise, creates the directory
fn ensure_snapshots_directory_exists() -> Result<(), String> {
    match fs::read_dir(SNAPSHOTS_PATH) {
        Err(err) => match err.kind() {
            ErrorKind::NotFound => simplify_result(fs::create_dir(SNAPSHOTS_PATH)),
            ErrorKind::NotADirectory => {
                Err(format!("Expected {} to be a directory", SNAPSHOTS_PATH))
            }
            _ => simplify_result(Err(err)),
        },
        Ok(_) => Ok(()),
    }
}

struct BranchesFile {
    branches: HashMap<String, String>,
}

struct HeadFile {
    curr_snapshot_id: Option<String>,
    curr_branch: String,
}

#[derive(PartialEq, Eq)]
enum SnapshotFullType {
    None,
    Tar,
    TarGz,
}

impl BranchesFile {
    fn write(self) -> Result<(), String> {
        tab_separated_key_value::Contents {
            multi_value: HashMap::new(),
            single_value: self.branches,
        }
        .write_file(BRANCHES_PATH)
    }
}

fn read_branches() -> Result<BranchesFile, String> {
    let contents = tab_separated_key_value::Config::single_value_only().read_file(BRANCHES_PATH)?;
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
        .write_file(HEAD_PATH)
    }
}

fn read_head() -> Result<HeadFile, String> {
    let map = tab_separated_key_value::Config::single_value_only().read_file(HEAD_PATH)?;
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

struct SnapshotMetaFile {
    id: String,
    date: u64,
    message: Option<String>,
    /// if set, the full contents of the snapshot are stored in
    /// `{snapshotId}-full`
    full_type: SnapshotFullType,
    children: Vec<String>,
    parents: Vec<String>,
    /// snapshots (_dchild_) such that this snapshot (_snapshotId_) can be
    /// recovered by applying the delta file `{snapshotId}-diff-{dchild}`
    /// to _dchild_
    diff_children: Vec<String>,
    /// the inverse of 'dchild'. That is: specifies the snapshot (_dparent_)
    /// such that the snapshot (_snapshotId_) can be used to recover _dparent_
    /// by applying the delta file `{dparent}-diff-{snapshotId}` to _dparent_
    diff_parents: Vec<String>,
}

impl SnapshotMetaFile {
    fn read(snapshot_id: &str) -> Result<SnapshotMetaFile, String> {
        let result = tab_separated_key_value::Config {
            multivalue_keys: SnapshotMetaFile::get_multivalue_keys(),
        }
        .read_file(&(String::from(SNAPSHOTS_PATH) + "/" + &snapshot_id + ".meta"))?;

        let snapshot_date = match result.single_value.get("date") {
            Some(s) => simplify_result(u64::from_str_radix(s, 10))?,
            None => {
                return Err(format!(
                    "Missing key 'date' in metadata of snapshot {}",
                    snapshot_id
                ));
            }
        };

        let full_type = match result.single_value.get("full") {
            Some(s) => s.parse::<SnapshotFullType>()?,
            None => SnapshotFullType::None,
        };

        fn get_multivalue(result: &tab_separated_key_value::Contents, key: &str) -> Vec<String> {
            result.multi_value.get(key).cloned().unwrap_or(Vec::new())
        }

        Ok(SnapshotMetaFile {
            id: String::from(snapshot_id),
            date: snapshot_date,
            message: result.single_value.get("message").cloned(),
            full_type,
            children: get_multivalue(&result, "child"),
            parents: get_multivalue(&result, "parent"),
            diff_children: get_multivalue(&result, "dchild"),
            diff_parents: get_multivalue(&result, "dparent"),
        })
    }

    fn get_multivalue_keys() -> HashSet<String> {
        let mut keys = HashSet::new();
        keys.insert(String::from("child"));
        keys.insert(String::from("parent"));
        keys.insert(String::from("dchild"));
        keys.insert(String::from("dparent"));
        keys
    }

    fn serialize(&self) -> Result<String, String> {
        tab_separated_key_value::Contents {
            single_value: {
                let mut m = HashMap::new();
                m.insert(String::from("date"), self.date.to_string());
                self.message
                    .clone()
                    .map(|s| m.insert(String::from("message"), s));
                m.insert(String::from("full"), self.full_type.to_string());
                m
            },
            multi_value: {
                let mut m = HashMap::new();
                m.insert(String::from("child"), self.children.clone());
                m.insert(String::from("parent"), self.parents.clone());
                m.insert(String::from("dchild"), self.diff_children.clone());
                m.insert(String::from("dparent"), self.diff_parents.clone());
                m
            },
        }
        .write_string()
    }

    fn get_full_payload_filename(&self) -> Result<String, String> {
        match &self.full_type {
            SnapshotFullType::None => Err(String::from("A full snapshot payload is not included")),
            _ => Ok(self.id.clone() + "-full." + &self.full_type.to_string()),
        }
    }
}

impl ToString for SnapshotFullType {
    fn to_string(&self) -> String {
        String::from(match self {
            SnapshotFullType::None => "",
            SnapshotFullType::Tar => "tar",
            SnapshotFullType::TarGz => "tar.gz",
        })
    }
}

impl FromStr for SnapshotFullType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "" => Ok(SnapshotFullType::None),
            "tar" => Ok(SnapshotFullType::Tar),
            "tar.gz" => Ok(SnapshotFullType::TarGz),
            _ => Err(String::from("Unrecognized snapshot full type")),
        }
    }
}
