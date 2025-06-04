use std::{collections::VecDeque, fs, process, time::SystemTime};

use crate::{
    JBACKUP_PATH, SNAPSHOTS_PATH, arguments, file_structure,
    io_util::{self, simplify_result},
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
            if curr_snapshot_meta.full_type != file_structure::SnapshotFullType::Tar {
                todo!("Not implemented: Current snapshot is not a tar snapshot type");
            }

            if staged_snapshot.full_type != file_structure::SnapshotFullType::Tar {
                todo!("Not implemented: Staged snapshot is not a tar snapshot type");
            }

            // add parent-child relations for staged snapshot
            curr_snapshot_meta.children.push(staged_snapshot.id.clone());
            staged_snapshot.parents.push(curr_snapshot_id.clone());

            // create diff
            let curr_snapshot_payload_full_name = curr_snapshot_meta.get_full_payload_filename()?;

            create_xdelta(CreateXDeltaArgs {
                from_archive: &(staged_snapshot.get_full_payload_filename()?),
                to_archive: &curr_snapshot_payload_full_name,
                output_archive: &curr_snapshot_meta.get_diff_path_from_child_snapshot(&staged_snapshot.id),
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
        full_type: file_structure::SnapshotFullType::Tar,
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
    io_util::run_command_handle_failures(
        process::Command::new("tar")
            .arg(String::from("--exclude=") + JBACKUP_PATH)
            .arg("-cf")
            .arg(&output_path)
            .arg("."),
    )?;

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

    // todo: maybe xdelta3 has a better api?
    let result = io_util::run_command_handle_failures(
        process::Command::new("xdelta")
            .arg("delta")
            .arg(&from_path)
            .arg(&to_path)
            .arg(&output_path),
    );

    if result.is_err() {
        eprintln!("Warn: xdelta exited badly");
    }

    Ok(())
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
