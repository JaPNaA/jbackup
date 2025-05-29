use std::{
    collections::HashMap,
    env,
    fmt::Display,
    fs,
    io::{self, ErrorKind},
    process::{self, Stdio},
    time::SystemTime,
};

fn main() {
    let mut args_iter = env::args();
    args_iter.next(); // ignore path

    let command = args_iter.next().unwrap_or_default();

    let result = match command.as_str() {
        "" => Err(String::from("Error: no command specified")),
        "init" => match init_repo() {
            Err(error) => Err(format!("Failed to initalize repository: IO Error: {error}")),
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

    write_branches(&BranchesFile {
        branches: {
            let mut branches: HashMap<String, Option<String>> = HashMap::new();
            branches.insert(String::from("main"), None);
            branches
        },
    })?;

    write_head(&HeadFile {
        curr_snapshot_id: None,
        curr_branch: String::from("main"),
    })?;

    println!("Successfully initalized jbackup in the current working directory.");
    Ok(())
}

/// Converts the error type in a Result into a string.
fn simplify_result<T>(io_result: Result<T, impl Display>) -> Result<T, String> {
    match io_result {
        Ok(v) => Ok(v),
        Err(err) => Err(format!("IO Error: {err}")),
    }
}

fn snapshot_repo() -> Result<(), String> {
    if !simplify_result(is_jbackup_in_working_dir())? {
        return Err(String::from(
            "Error: jbackup not found in current working directory. (To make a new backup for this directory, do 'jbackup init')",
        ));
    }

    let new_id = create_full_snapshot()?;
    print!("Created snapshot with id: {}", new_id);

    let mut head_file = read_head()?;
    let mut branch_file = read_branches()?;

    let head_tar_path = get_head_tar(&head_file)?;

    match head_tar_path {
        None => {
            head_file.curr_snapshot_id = Some(new_id.clone());
            branch_file
                .branches
                .insert(head_file.curr_branch.clone(), Some(new_id));
        }
        Some(p) => {
            todo!();
        }
    }

    write_head(&head_file)?;
    write_branches(&branch_file)?;

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

    let id = timestamp.to_string() + "-" + &md5;

    commit_tmp_snapshot(
        &tmp_snapshot_path,
        SnapshotMetaFile {
            id: String::clone(&id),
            snapshot_type: SnapshotType::Full,
            date: timestamp,
            message: None,
            children: Vec::new(),
            parents: Vec::new(),
        },
    )?;

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

fn calc_md5(file_path: &String) -> Result<String, String> {
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

fn commit_tmp_snapshot(tmp_snapshot_path: &String, data: SnapshotMetaFile) -> Result<(), String> {
    ensure_snapshots_directory_exists()?;

    let snapshot_path = String::from("./.jbackup/snapshots/") + &data.id;

    simplify_result(fs::rename(
        tmp_snapshot_path,
        String::clone(&snapshot_path) + "-" + data.snapshot_type.to_string().as_str(),
    ))?;
    simplify_result(fs::write(snapshot_path + ".meta", data.to_string()))?;
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
}

fn write_branches(branches: &BranchesFile) -> Result<(), String> {
    simplify_result(fs::write(".jbackup/branches", branches.to_string()))
}

fn write_head(head: &HeadFile) -> Result<(), String> {
    simplify_result(fs::write(".jbackup/head", head.to_string()))
}

struct BranchesFile {
    branches: HashMap<String, Option<String>>,
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

impl ToString for BranchesFile {
    fn to_string(&self) -> String {
        let mut sorted = self.branches.iter().collect::<Vec<_>>();
        sorted.sort();

        let mut result = String::new();

        for item in sorted {
            result.push_str(item.0);
            result.push('\t');
            result.push_str(match item.1 {
                None => "NULL",
                Some(s) => s,
            });
            result.push('\n');
        }

        result
    }
}

fn read_branches() -> Result<BranchesFile, String> {
    Ok(BranchesFile {
        branches: read_simple_tab_separated_file("./.jbackup/branches")?,
    })
}

impl ToString for HeadFile {
    fn to_string(&self) -> String {
        let mut result = String::new();
        result.push_str("snapshotid\t");
        result.push_str(match &self.curr_snapshot_id {
            None => "NULL",
            Some(s) => s,
        });
        result.push_str("\nbranch\t");
        result.push_str(&self.curr_branch);
        result.push('\n');
        return result;
    }
}

fn read_head() -> Result<HeadFile, String> {
    let map = read_simple_tab_separated_file("./.jbackup/head")?;
    let curr_snapshot_id = map.get("snapshotid");
    let curr_branch = map.get("branch");
    if curr_branch.is_none() || curr_snapshot_id.is_none() {
        return Err(String::from(
            "The head file is missing required values (snapshotid, branch)",
        ));
    }

    Ok(HeadFile {
        curr_snapshot_id: curr_snapshot_id
            .expect("snapshot id should have been validated to have a value")
            .clone(),
        curr_branch: curr_branch
            .expect("branch should have been validated to have a value")
            .clone()
            .expect("branch should not be NULL")
            .clone(),
    })
}

/// Reads a simple tab separated file and inserts the key/value pairs in a
/// HashMap.
///
/// Simple tab separated files:
///   - do not contain '\n' in the value
///   - do not contain '\t' or '\n' in the key
///   - do not contain multiple values for the same key
///   - if the value is exactly "NULL", then the value is stored as None
fn read_simple_tab_separated_file(path: &str) -> Result<HashMap<String, Option<String>>, String> {
    let data = simplify_result(String::from_utf8(simplify_result(fs::read(path))?))?;

    let mut map: HashMap<String, Option<String>> = HashMap::new();

    for line in data.split('\n') {
        if line.is_empty() {
            continue;
        }

        match line.find('\t') {
            None => return Err(format!("File '{path}' is corrupted")),
            Some(i) => {
                let key = line[..i].to_string();
                let val = &line[i + 1..];
                if val == "NULL" {
                    map.insert(key, None);
                } else {
                    map.insert(key, Some(val.to_string()));
                }
            }
        }
    }

    Ok(map)
}

impl ToString for SnapshotMetaFile {
    fn to_string(&self) -> String {
        let mut result = String::new();

        result.push_str("type\t");
        result.push_str(self.snapshot_type.to_string().as_str());
        result.push_str("\ndate\t");
        result.push_str(self.date.to_string().as_str());
        result.push('\n');

        match &self.message {
            Some(s) => {
                result.push_str("message\t");
                result.push_str(escape_string_for_meta(s).as_str());
                result.push('\n');
            }
            None => {}
        };

        for parent in self.parents.iter() {
            result.push_str("parent\t");
            result.push_str(parent.as_str());
            result.push('\n');
        }

        for child in self.children.iter() {
            result.push_str("child\t");
            result.push_str(child.as_str());
            result.push('\n');
        }

        result
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

fn escape_string_for_meta(s: &String) -> String {
    s.replace('\\', "\\").replace('\n', "\\n")
}
