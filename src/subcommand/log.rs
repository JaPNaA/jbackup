use std::fs;

use chrono::{Local, TimeZone};

use crate::{SNAPSHOTS_PATH, file_structure, io_util::simplify_result};

pub fn main() -> Result<(), String> {
    file_structure::ensure_jbackup_snapshots_dir_exists()?;

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

    let timezone = chrono::Local::now().timezone();

    for item in snapshot_ids {
        let meta = file_structure::SnapshotMetaFile::read(&item)?;
        snapshots.push(meta);
    }

    snapshots.sort_by_key(|x| x.date);

    for meta in snapshots {
        let timestamp = match chrono::DateTime::from_timestamp(meta.date, 0) {
            None => String::from("Invalid date"),
            Some(d) => d
                .with_timezone(&timezone)
                .format("%Y/%m/%d %H:%M:%S")
                .to_string(),
        };

        match meta.message {
            None => { },
            Some(s) => println!("Message:   {}", &s)
        }
        println!("Timestamp: {}\nId:        {}\n", timestamp, meta.id);
    }

    Ok(())
}
