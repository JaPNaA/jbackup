use crate::file_structure;

pub fn main() -> Result<(), String> {
    let mut snapshots = file_structure::get_all_snapshot_meta_files()?;

    let timezone = chrono::Local::now().timezone();

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
            None => {}
            Some(s) => println!("Message:   {}", &s),
        }
        println!("Timestamp: {}\nId:        {}\n", timestamp, meta.id);
    }

    Ok(())
}
