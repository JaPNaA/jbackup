use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{self, ErrorKind},
    str::FromStr,
};

use crate::{
    BRANCHES_PATH, CONFIG_PATH, HEAD_PATH, JBACKUP_PATH, SNAPSHOTS_PATH, io_util::simplify_result,
    string_set, tab_separated_key_value,
};

pub struct BranchesFile {
    pub branches: HashMap<String, String>,
}

impl BranchesFile {
    pub fn read() -> Result<BranchesFile, String> {
        let contents =
            tab_separated_key_value::Config::single_value_only().read_file(BRANCHES_PATH)?;
        Ok(BranchesFile {
            branches: contents.single_value,
        })
    }

    pub fn write(self) -> Result<(), String> {
        tab_separated_key_value::Contents {
            multi_value: HashMap::new(),
            single_value: self.branches,
        }
        .write_file(BRANCHES_PATH)
    }
}

pub struct HeadFile {
    pub curr_snapshot_id: Option<String>,
    pub curr_branch: String,
}

impl HeadFile {
    pub fn read() -> Result<HeadFile, String> {
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

    pub fn write(self) -> Result<(), String> {
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

pub struct SnapshotMetaFile {
    pub id: String,
    pub date: i64,
    pub message: Option<String>,
    /// if set, the full contents of the snapshot are stored in
    /// `{snapshotId}-full`
    pub full_type: SnapshotFullType,
    pub children: Vec<String>,
    pub parents: Vec<String>,
    /// snapshots (_dchild_) such that this snapshot (_snapshotId_) can be
    /// recovered by applying the delta file `{snapshotId}-diff-{dchild}`
    /// to _dchild_
    pub diff_children: Vec<String>,
    /// the inverse of 'dchild'. That is: specifies the snapshot (_dparent_)
    /// such that the snapshot (_snapshotId_) can be used to recover _dparent_
    /// by applying the delta file `{dparent}-diff-{snapshotId}` to _dparent_
    pub diff_parents: Vec<String>,
}

impl SnapshotMetaFile {
    pub fn read(snapshot_id: &str) -> Result<SnapshotMetaFile, String> {
        let result = tab_separated_key_value::Config {
            multivalue_keys: SnapshotMetaFile::get_multivalue_keys(),
        }
        .read_file(&(SnapshotMetaFile::get_meta_file_path(&snapshot_id)))?;

        let snapshot_date = match result.single_value.get("date") {
            Some(s) => simplify_result(i64::from_str_radix(s, 10))?,
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

    pub fn write(&self) -> Result<(), String> {
        simplify_result(fs::write(
            SnapshotMetaFile::get_meta_file_path(&self.id),
            self.serialize()?,
        ))
    }

    pub fn get_meta_file_path(id: &str) -> String {
        String::from(SNAPSHOTS_PATH) + "/" + id + ".meta"
    }

    pub fn get_full_payload_filename(&self) -> Result<String, String> {
        match &self.full_type {
            SnapshotFullType::None => Err(String::from("A full snapshot payload is not included")),
            _ => Ok(self.id.clone() + "-full." + &self.full_type.to_string()),
        }
    }

    pub fn get_diff_path_from_child_snapshot(&self, snapshot_id: &str) -> String {
        self.id.clone() + "-diff-" + &snapshot_id
    }

    fn get_multivalue_keys() -> HashSet<String> {
        string_set!["child", "parent", "dchild", "dparent"]
    }

    fn serialize(&self) -> Result<String, String> {
        tab_separated_key_value::Contents {
            single_value: {
                let mut m = HashMap::new();
                m.insert(String::from("date"), self.date.to_string());

                self.message
                    .clone()
                    .map(|s| m.insert(String::from("message"), s));

                if self.full_type != SnapshotFullType::None {
                    m.insert(String::from("full"), self.full_type.to_string());
                }

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
}

#[derive(PartialEq, Eq)]
pub enum SnapshotFullType {
    None,
    Tar,
    TarGz,
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

/// Retrieves all snapshot metadata files in the current repository.
/// This function parses all files and returns the files in arbitrary order.
pub fn get_all_snapshot_meta_files() -> Result<Vec<SnapshotMetaFile>, String> {
    ensure_jbackup_snapshots_dir_exists()?;

    let mut snapshot_ids = Vec::new();

    let dir = simplify_result(fs::read_dir(SNAPSHOTS_PATH))?;

    for item in dir {
        match item {
            Err(_) => {}
            Ok(entry) => match entry.file_name().into_string() {
                Err(_) => {}
                Ok(file_name) => match file_name.strip_suffix(".meta") {
                    None => {}
                    Some(x) => snapshot_ids.push(String::from(x)),
                },
            },
        }
    }

    let mut snapshots = Vec::new();

    for item in snapshot_ids {
        let meta = SnapshotMetaFile::read(&item)?;
        snapshots.push(meta);
    }

    Ok(snapshots)
}

pub struct ConfigFile {
    pub transformers: Vec<String>,
}

impl ConfigFile {
    pub fn read() -> Result<ConfigFile, String> {
        let contents = tab_separated_key_value::Config {
            multivalue_keys: string_set!["transformer"],
        }
        .read_file(CONFIG_PATH)?;
        Ok(ConfigFile {
            transformers: match contents.multi_value.get("transformer") {
                Some(x) => x.clone(),
                None => Vec::new(),
            },
        })
    }

    pub fn write(self) -> Result<(), String> {
        tab_separated_key_value::Contents {
            multi_value: {
                let mut m = HashMap::new();
                m.insert(String::from("transformer"), self.transformers);
                m
            },
            single_value: HashMap::new(),
        }
        .write_file(CONFIG_PATH)
    }
}

/// Checks if .jbackup is in the current directory, then checks
/// if the snapshot directory exists.
///
/// If .jbackup is not in the current directory, an error is returned.
///
/// If snapshot directory is created if it doesn't exist.
///
/// Otherwise, the function returns Ok
pub fn ensure_jbackup_snapshots_dir_exists() -> Result<(), String> {
    if !simplify_result(is_jbackup_in_working_dir())? {
        return Err(String::from(
            "Error: a valid jbackup was not found in current working directory. (To make a new backup for this directory, do 'jbackup init')",
        ));
    }

    ensure_snapshots_directory_exists()?;

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
            let mut found_config = false;

            for item in result {
                match item.ok() {
                    None => {}
                    Some(entry) => match entry.file_name().into_string() {
                        Ok(s) => match s.as_str() {
                            "branches" => found_branches = true,
                            "head" => found_head = true,
                            "config" => found_config = true,
                            _ => {}
                        },
                        Err(_) => {}
                    },
                }
            }

            if found_branches && found_head && found_config {
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
