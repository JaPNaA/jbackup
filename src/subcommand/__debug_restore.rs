use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs::{self, File},
    io::{BufReader, Read},
};

use flate2::bufread::GzDecoder;
use tar::EntryType;

use crate::{
    JBACKUP_PATH,
    delta_list::restore_from_delta_list,
    file_structure::{self, ConfigFile, SnapshotFullType, SnapshotMetaFile},
    prepend_snapshot_path,
    transformer::get_transformers,
    util::{
        archive_utils::{create_tar_gz, open_delta_list, open_tar_gz},
        io_util::simplify_result,
    },
};

pub fn main(mut args: VecDeque<String>) -> Result<(), String> {
    let snapshot_id = match args.pop_front() {
        None => {
            return Err(String::from("Please specify a snapshot"));
        }
        Some(x) => x,
    };

    let mut snapshots = HashMap::new();
    for snapshot in file_structure::get_all_snapshot_meta_files()? {
        snapshots.insert(String::from(&snapshot.id), snapshot);
    }

    if snapshots.is_empty() {
        return Err(String::from("There are no snapshots in this repository."));
    }

    let mut path = Vec::new();
    let mut path_found = false;

    let mut curr = snapshots.remove(&snapshot_id);

    // very simple algorithm of following the child until we find a full snapshot
    loop {
        match curr.take() {
            Some(snapshot) => {
                curr = snapshot
                    .diff_children
                    .first()
                    .and_then(|x| snapshots.remove(x));

                let is_full_type = snapshot.full_type != SnapshotFullType::None;

                path.push(snapshot);

                if is_full_type {
                    path_found = true;
                    break;
                }
            }
            None => {
                break;
            }
        }
    }

    path.reverse();
    // for item in path {
    //     println!("{}", item.id);
    // }

    if path_found {
        println!("Restored to: {}", follow_path(path)?);
    } else {
        println!("Path not found to {}", snapshot_id);
    }

    Ok(())
}

pub fn main2(mut args: VecDeque<String>) -> Result<(), String> {
    let archive_path = match args.pop_front() {
        None => {
            return Err(String::from("Please specify an archive to transform out"));
        }
        Some(x) => x,
    };

    let transformer_names = ConfigFile::read()?.transformers;
    let transformers = get_transformers(&transformer_names)?;

    let archive_file = simplify_result(File::open(archive_path))?;
    let gzdec = GzDecoder::new(BufReader::new(archive_file));
    let mut tar_reader = tar::Archive::new(gzdec);
    let mut dir_tree_builder = DirectoryTreeBuilder::new();

    for entry in simplify_result(tar_reader.entries())? {
        let mut entry = match entry {
            Ok(x) => x,
            Err(err) => {
                eprintln!("Warn: failed to read tar entry: {:?}", err);
                continue;
            }
        };
        let path = match entry.path() {
            Ok(x) => String::from(x.to_string_lossy()),
            Err(err) => {
                eprintln!("Warn: failed to get path for tar entry: {:?}", err);
                continue;
            }
        };

        if entry.header().entry_type() != EntryType::Regular {
            eprintln!(
                "Warn: Ignoring item: '{}' since it's not a regular file",
                &path
            );
            continue;
        }

        validate_no_parent_references(&path)?;

        let mut curr = Vec::new();
        simplify_result(entry.read_to_end(&mut curr))?;

        for transformer in &transformers {
            curr = transformer.transform_out(&path, curr)?;
        }

        let output_path = String::from(".jbackup/tmp-restored/") + &path;
        let parent_dir_path = dir_name(&output_path);

        dir_tree_builder.prepare_dir(&parent_dir_path)?;

        simplify_result(fs::write(output_path, curr))?;
    }

    Ok(())
}

/// Returns a string with the final generated file
fn follow_path(path: Vec<SnapshotMetaFile>) -> Result<String, String> {
    if path.is_empty() {
        return Err(String::from("Generated snapshot path was empty"));
    }

    let first_snapshot = path.first().expect("Path should not be empty");

    if first_snapshot.full_type != SnapshotFullType::TarGz {
        todo!("Not implemented: full type must be tar.gz");
    }

    let mut prev_snapshot_id = first_snapshot.id.clone();
    let mut prev_tar_path = prepend_snapshot_path(&first_snapshot.get_full_payload_filename()?);
    let mut delete_prev_tar_path = false; // don't delete first

    for next_snapshot in path.iter().skip(1) {
        let new_tar_path = String::from(JBACKUP_PATH) + "/tmp-restored-" + &next_snapshot.id;

        restore_from_delta_list(
            open_tar_gz(&prev_tar_path)?,
            create_tar_gz(&new_tar_path)?,
            open_delta_list(&prepend_snapshot_path(
                &next_snapshot.get_diff_path_from_child_snapshot(&prev_snapshot_id),
            ))?,
        )?;

        eprintln!("Restored {}", &new_tar_path);

        if delete_prev_tar_path {
            eprintln!("Deleting {}", &prev_tar_path);
            simplify_result(fs::remove_file(prev_tar_path))?;
        }

        prev_snapshot_id = next_snapshot.id.clone();
        prev_tar_path = new_tar_path;
        delete_prev_tar_path = true;
    }

    return Ok(prev_tar_path);
}

fn dir_name(path: &str) -> String {
    let mut clean_path = path;
    if path.ends_with('/') {
        clean_path = &path[0..path.len() - 1];
    }

    let idx = clean_path.rfind("/");
    String::from(match idx {
        None => "",
        Some(x) => &clean_path[0..x],
    })
}

fn all_parent_directories(path: &str) -> Vec<String> {
    let mut parent_dirs = Vec::new();

    for (i, _slice) in path.match_indices("/") {
        if i == 0 {
            continue;
        } // don't split at leading '/'
        if i >= path.len() - 1 {
            continue;
        } // don't split at tailing '/'
        parent_dirs.push(String::from(&path[0..i]));
    }

    parent_dirs
}

/// Validate the path does not contain any ".." directories.
/// We should refuse to extract these files.
fn validate_no_parent_references(path: &str) -> Result<(), String> {
    if path.split("/").any(|x| x == "..") {
        return Err(format!(
            "Archive entry has path '{}', which attempts to reference a parent directory. The archive may be malicious, so extraction was canceled.",
            path
        ));
    }
    Ok(())
}

/// Given directory tree specified by a collection of paths,
/// performs the minimum amount of `mkdir` syscalls to construct the directory
/// tree.
struct DirectoryTreeBuilder(HashSet<String>);

impl DirectoryTreeBuilder {
    pub fn new() -> DirectoryTreeBuilder {
        DirectoryTreeBuilder(HashSet::new())
    }

    pub fn prepare_dir(&mut self, dir_path: &str) -> Result<(), String> {
        let dir_path = String::from(dir_path);
        if self.0.contains(&dir_path) {
            return Ok(());
        }

        simplify_result(fs::create_dir_all(&dir_path))?;

        let all_parents = all_parent_directories(&dir_path);

        self.0.insert(dir_path);

        for parent in all_parents {
            self.0.insert(parent);
        }

        Ok(())
    }
}
