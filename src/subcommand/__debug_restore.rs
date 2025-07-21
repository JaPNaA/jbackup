use std::{
    collections::{HashMap, VecDeque},
    fs::{self, File},
    io::{BufReader, Read},
    process,
};

use flate2::bufread::GzDecoder;

use crate::{
    JBACKUP_PATH, SNAPSHOTS_PATH,
    file_structure::{self, ConfigFile, SnapshotFullType, SnapshotMetaFile},
    transformer::get_transformers,
    util::io_util::{self, simplify_result},
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

        let mut curr = Vec::new();
        simplify_result(entry.read_to_end(&mut curr))?;

        for transformer in &transformers {
            curr = transformer.transform_out(&path, curr)?;
        }

        simplify_result(fs::write(
            String::from(".jbackup/tmp-restored/") + &path.replace("/", "-"),
            curr,
        ))?;
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
    let mut prev_tar_path =
        String::from(SNAPSHOTS_PATH) + "/" + &first_snapshot.get_full_payload_filename()?;
    let mut delete_prev_tar_path = false; // don't delete first

    for next_snapshot in path.iter().skip(1) {
        let new_tar_path = String::from(JBACKUP_PATH) + "/tmp-restored-" + &next_snapshot.id;

        xdelta_patch(XDeltaPatchArgs {
            from_path: prev_tar_path.clone(),
            patch_file_path: String::from(SNAPSHOTS_PATH)
                + "/"
                + &next_snapshot.get_diff_path_from_child_snapshot(&prev_snapshot_id),
            output_path: new_tar_path.clone(),
        })?;

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

struct XDeltaPatchArgs {
    from_path: String,
    patch_file_path: String,
    output_path: String,
}

fn xdelta_patch(args: XDeltaPatchArgs) -> Result<(), String> {
    // todo: maybe xdelta3 has a better api?
    let result = io_util::run_command_handle_failures(
        process::Command::new("xdelta3")
            .env("GZIP", "-1")
            .arg("-d")
            .arg("-f")
            .arg("-B500000000") // must match the buffer size when encoding
            .arg("-s")
            .arg(&args.from_path)
            .arg(&args.patch_file_path)
            .arg(&args.output_path),
    );

    if result.is_err() {
        Err(String::from("xdelta3 exited badly"))
    } else {
        Ok(())
    }
}
