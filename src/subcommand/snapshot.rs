use std::{
    collections::VecDeque,
    ffi::OsString,
    fs::{self, File, Metadata},
    process,
    sync::Arc,
    time::SystemTime,
};

use flate2::Compression;
use gzp::{
    deflate::Gzip,
    par::compress::{ParCompress, ParCompressBuilder},
};

use crate::{
    JBACKUP_PATH, SNAPSHOTS_PATH, arguments,
    file_structure::{self, ConfigFile},
    transformer::get_transformers,
    util::{
        io_util::{self, simplify_result},
        multithreaded_pipeline::MultithreadPipeline,
    },
};

/// Creates a snapshot of the current working directory (excluding .jbackup).
///
/// A user should be able to restore the working directory to when they made
/// a snapshot.
///
/// Will read the arguments to find an optional message for the snapshot.
///
pub fn main(mut args: VecDeque<String>) -> Result<(), String> {
    let mut parsed_args = arguments::Parser::new().option("-m").parse(args.drain(..));
    let mut snapshot_message_arg = parsed_args.options.remove("-m");

    file_structure::ensure_jbackup_snapshots_dir_exists()?;

    let mut files_to_delete = FilesToDelete::new();

    let mut staged_snapshot = create_full_snapshot()?;

    if simplify_result(fs::exists(
        file_structure::SnapshotMetaFile::get_meta_file_path(&staged_snapshot.id),
    ))? {
        return Err(format!(
            "A snapshot with the same id ({}) already exists.",
            &staged_snapshot.id
        ));
    }

    staged_snapshot.message = snapshot_message_arg.take();

    let mut head_file = file_structure::HeadFile::read()?;
    let mut branch_file = file_structure::BranchesFile::read()?;

    match &head_file.curr_snapshot_id {
        None => {
            staged_snapshot.write()?;
        }
        Some(curr_snapshot_id) => {
            let mut curr_snapshot_meta = file_structure::SnapshotMetaFile::read(&curr_snapshot_id)?;
            if curr_snapshot_meta.full_type != file_structure::SnapshotFullType::TarGz {
                todo!("Not implemented: Current snapshot is not a tar.gz snapshot type");
            }

            if staged_snapshot.full_type != file_structure::SnapshotFullType::TarGz {
                todo!("Not implemented: Staged snapshot is not a tar.gz snapshot type");
            }

            // add parent-child relations for staged snapshot
            curr_snapshot_meta.children.push(staged_snapshot.id.clone());
            staged_snapshot.parents.push(curr_snapshot_id.clone());

            // create diff
            let curr_snapshot_payload_full_name = curr_snapshot_meta.get_full_payload_filename()?;

            create_xdelta(CreateXDeltaArgs {
                from_archive: &(staged_snapshot.get_full_payload_filename()?),
                to_archive: &curr_snapshot_payload_full_name,
                output_archive: &curr_snapshot_meta
                    .get_diff_path_from_child_snapshot(&staged_snapshot.id),
            })?;

            curr_snapshot_meta
                .diff_children
                .push(staged_snapshot.id.clone());
            staged_snapshot.diff_parents.push(curr_snapshot_id.clone());

            // mark snapshot as having no full payload, but we will only delete the file
            // after all snapshot metadata have been written
            curr_snapshot_meta.full_type = file_structure::SnapshotFullType::None;
            files_to_delete
                .snapshots_files
                .push(curr_snapshot_payload_full_name);

            staged_snapshot.write()?;
            curr_snapshot_meta.write()?;
        }
    }

    println!("Created snapshot with id: {}", &staged_snapshot.id);

    head_file.curr_snapshot_id = Some(staged_snapshot.id.clone());
    branch_file
        .branches
        .insert(head_file.curr_branch.clone(), staged_snapshot.id.clone());

    head_file.write()?;
    branch_file.write()?;

    files_to_delete.delete_files();

    Ok(())
}

struct FilesToDelete {
    snapshots_files: Vec<String>,
}

impl FilesToDelete {
    fn new() -> FilesToDelete {
        FilesToDelete {
            snapshots_files: Vec::new(),
        }
    }

    /// Wrapper of _delete_files that prints a warning when
    /// the child function fails.
    fn delete_files(&self) {
        match self._delete_files() {
            Ok(_) => {}
            Err(err) => eprintln!("Warn: Error when cleaning files up: {}", err),
        }
    }

    fn _delete_files(&self) -> Result<(), String> {
        for filepath in &self.snapshots_files {
            simplify_result(fs::remove_file(
                String::from(SNAPSHOTS_PATH) + "/" + &filepath,
            ))?;
        }
        Ok(())
    }
}

/// Creates a `tar` of the current working directly, excluding "./.jbackup".
/// The `tar` is placed in the returned path.
fn create_full_snapshot() -> Result<file_structure::SnapshotMetaFile, String> {
    let tmp_tar_path = create_tmp_tar()?;
    let md5 = calc_md5(&tmp_tar_path)?;
    let timestamp = match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(n) => n.as_secs().try_into().unwrap(),
        Err(_) => 0,
    };

    let id: String = timestamp.to_string() + "-" + &md5;

    let snapshot_metadata = file_structure::SnapshotMetaFile {
        id: id.clone(),
        full_type: file_structure::SnapshotFullType::TarGz,
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
    let output_path = String::from(JBACKUP_PATH) + "/tmp_snapshot.tar.gz";
    let output_file = simplify_result(File::create(&output_path))?;

    let gz_builder: ParCompress<Gzip> = ParCompressBuilder::new()
        .compression_level(Compression::fast()) // todo: this should be configurable
        .from_writer(output_file);
    let tar_builder = Box::new(tar::Builder::new(gz_builder));

    let mut transformer_pipeline =
        MultithreadPipeline::<OsString, Result<(Vec<u8>, Metadata, String), String>, _>::new(
            tar_builder,
            Box::new(move |tar_builder, res| match res {
                Ok((transformed_data, file_metadata, file_path)) => {
                    let mut header = tar::Header::new_gnu();
                    header.set_metadata(&file_metadata);
                    header.set_size(transformed_data.len().try_into().unwrap());

                    tar_builder
                        .append_data(&mut header, &file_path[2..], transformed_data.as_slice())
                        .unwrap();
                }
                Err(err) => panic!("{}", err),
            }),
        );

    let transformer_names = ConfigFile::read()?.transformers;
    let transformers_arc = Arc::new(get_transformers(&transformer_names)?);

    transformer_pipeline.spawn_workers(8, transformers_arc, |transformers, file_path| {
        let Some(file_path) = file_path.to_str() else {
            return Err(format!(
                "Failed to convert file path '{:?}' to UTF-8",
                file_path,
            ));
        };

        let Ok(file_metadata) = simplify_result(fs::metadata(&file_path)) else {
            return Err(format!(
                "Failed to read file metadata for file {}",
                file_path
            ));
        };
        let Ok(file_contents) = simplify_result(fs::read(&file_path)) else {
            return Err(format!("Failed to read file {}", file_path));
        };

        println!("Inserting: {}", file_path);

        let mut transformed_data = file_contents;

        for transformer in transformers.iter() {
            transformed_data = transformer.transform_in(&file_path, transformed_data)?;
        }

        Ok((transformed_data, file_metadata, String::from(file_path)))
    });

    walk_file_tree(".".into(), &mut |new_file_path| {
        transformer_pipeline.write(new_file_path);
        transformer_pipeline.poll();
        Ok(())
    })?;

    simplify_result(transformer_pipeline.finalize().into_inner())?;

    Ok(output_path)
}

fn calc_md5(file_path: &str) -> Result<String, String> {
    let output =
        io_util::run_command_handle_failures(process::Command::new("md5sum").arg(&file_path))?;

    let output_str = simplify_result(String::from_utf8(output.stdout))?;
    match output_str.find(' ') {
        Some(index) => Ok(String::from(&output_str[..index])),
        None => Err(String::from(
            "md5sum did not output in the expected format.",
        )),
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

    eprintln!("Creating xdelta... from: {}, to: {}", from_path, to_path);

    // todo: maybe xdelta3 has a better api?
    let result = io_util::run_command_handle_failures(
        process::Command::new("xdelta3")
            .arg("-S")
            .arg("djw")
            .arg("-f")
            .arg("-B2147483648")
            .arg("-s")
            .arg(&from_path)
            .arg(&to_path)
            .arg(&output_path),
    );

    if result.is_err() {
        Err(String::from("xdelta3 exited badly"))
    } else {
        Ok(())
    }
}

fn commit_tmp_snapshot(
    tmp_snapshot_path: &str,
    data: &file_structure::SnapshotMetaFile,
) -> Result<(), String> {
    let snapshot_payload_path =
        String::from(SNAPSHOTS_PATH) + "/" + &data.get_full_payload_filename()?;

    let file_exists = simplify_result(fs::exists(&snapshot_payload_path))?;
    if file_exists {
        Err(format!(
            "Tried to commit snapshot to '{}', but the file already exists",
            &snapshot_payload_path
        ))
    } else {
        simplify_result(fs::rename(tmp_snapshot_path, snapshot_payload_path))?;
        Ok(())
    }
}

/// Walks the file tree for some directory.
///
/// The paths visited are always in UTF-8 ascending order.
///
/// Ignores .jbackup directories that are a direct child of
/// the specified directory.
pub fn walk_file_tree(
    dir_path: OsString,
    file_handler: &mut impl FnMut(OsString) -> Result<(), String>,
) -> Result<(), String> {
    _walk_file_tree(dir_path, 0, file_handler)
}

enum FileType {
    Regular,
    Directory,
}

fn _walk_file_tree(
    dir_path: OsString,
    depth: usize,
    file_handler: &mut impl FnMut(OsString) -> Result<(), String>,
) -> Result<(), String> {
    let files = simplify_result(fs::read_dir(&dir_path))?;
    let mut sorted_files = Vec::new();

    for file in files {
        match file {
            Err(err) => {
                eprint!(
                    "Warning: failed to read file in '{}' due to: {}",
                    dir_path.to_str().unwrap_or("<invalid string>"),
                    err
                );
            }
            Ok(file) => match file.file_type() {
                Err(err) => {
                    eprint!(
                        "Warning: failed to get file type for file '{}/{}' due to: {}",
                        dir_path.to_str().unwrap_or("<invalid string>"),
                        file.file_name().to_str().unwrap_or("<invalid string>"),
                        err
                    )
                }
                Ok(file_type) => {
                    if file_type.is_file() {
                        sorted_files.push((FileType::Regular, file.file_name()))
                    } else if file_type.is_dir() {
                        if depth != 0 || file.file_name() != ".jbackup" {
                            sorted_files.push((FileType::Directory, file.file_name()));
                        }
                    }
                }
            },
        }
    }

    sorted_files.sort_by(|a, b| a.1.cmp(&b.1));

    for (file_type, file) in sorted_files {
        let mut path = dir_path.clone();
        path.push("/");
        path.push(file);
        match file_type {
            FileType::Regular => {
                file_handler(path)?;
            }
            FileType::Directory => {
                _walk_file_tree(path, depth + 1, file_handler)?;
            }
        };
    }

    Ok(())
}
