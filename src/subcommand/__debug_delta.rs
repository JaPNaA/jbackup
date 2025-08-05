use std::{
    collections::VecDeque,
    fs::File,
    io::{BufReader, Read, Write},
};

use flate2::{GzBuilder, bufread::GzDecoder, write::GzEncoder};
use gzp::Compression;

use crate::util::io_util::simplify_result;

pub fn main(mut args: VecDeque<String>) -> Result<(), String> {
    let Some(start_filename) = args.pop_front() else {
        return Err(String::from("Must provide start file"));
    };
    let Some(end_filename) = args.pop_front() else {
        return Err(String::from("Must provide ending file"));
    };
    let Some(output_filename) = args.pop_front() else {
        return Err(String::from("Must provide output file"));
    };

    let start_file = simplify_result(File::open(start_filename))?;
    let end_file = simplify_result(File::open(end_filename))?;
    let output_file = simplify_result(File::create(output_filename))?;

    let start_dec = GzDecoder::new(BufReader::new(start_file));
    let end_dec = GzDecoder::new(BufReader::new(end_file));
    let output_builder = GzBuilder::new().write(output_file, Compression::default());
    let mut delta_list = JBackupFileDeltaList::new(output_builder)?;

    let mut start_tar = tar::Archive::new(start_dec);
    let mut end_tar = tar::Archive::new(end_dec);

    let mut start_entries = simplify_result(start_tar.entries())?;
    let mut end_entries = simplify_result(end_tar.entries())?;

    let mut start_entry = start_entries.next();
    let mut end_entry = end_entries.next();

    loop {
        match (start_entry.take(), end_entry.take()) {
            (Some(Ok(mut start_entry_uw)), Some(Ok(mut end_entry_uw))) => {
                let start_path = simplify_result(start_entry_uw.path())?;
                let start_path = start_path.to_string_lossy();
                let end_path = simplify_result(end_entry_uw.path())?;
                let end_path = end_path.to_string_lossy();

                if start_path == end_path {
                    println!("{} / {}", start_path, end_path);

                    let start_path = String::from(start_path);

                    let mut start_buf = Vec::new();
                    simplify_result(start_entry_uw.read_to_end(&mut start_buf))?;

                    let mut end_buf = Vec::new();
                    simplify_result(end_entry_uw.read_to_end(&mut end_buf))?;

                    if let Some(res) = xdelta3::encode(&end_buf, &start_buf) {
                        eprintln!(
                            "Generated delta for {} with size: {}",
                            &start_path,
                            res.len()
                        );
                        delta_list.add_modify(&start_path, &res)?;
                    } else {
                        eprintln!("No xdelta output for {}", &start_path);
                    }

                    start_entry = start_entries.next();
                    end_entry = end_entries.next();
                } else if start_path < end_path {
                    delta_list.add_delete(&start_path)?;

                    start_entry = start_entries.next();
                    end_entry = Some(Ok(end_entry_uw));
                } else {
                    let mut buf = Vec::new();
                    let end_path = end_path.to_string();
                    simplify_result(end_entry_uw.read_to_end(&mut buf))?;
                    delta_list.add_add(&end_path, &buf)?;

                    start_entry = Some(Ok(start_entry_uw));
                    end_entry = end_entries.next();
                }
            }
            (None, None) => {
                break;
            }
            _ => {}
        }
    }

    delta_list.try_finish()?;

    Ok(())
}

enum FileChangeType {
    Deleted,
    Modified,
    Added,
}

impl FileChangeType {
    pub fn to_bytes(&self) -> [u8; 8] {
        self.to_u64().to_be_bytes()
    }

    fn to_u64(&self) -> u64 {
        match self {
            FileChangeType::Deleted => 1,
            FileChangeType::Modified => 2,
            FileChangeType::Added => 3,
        }
    }
}

/// A delta list. Files should always be added in UTF-8-byte-ascending order.
///
/// The format is as follows:
///
/// - Magic bytes: 'DL'
/// - Version number: 1u32
/// - (string length: u64, char[], Delta)[]
///   - Delta is one of the following:
///     - [Deleted]
///     - [Modified, xdelta length: u64, xdelta: byte[]]
///     - [Add, content length: u64, content: byte[]]
///
/// All numbers are encoded in big-endian.
struct JBackupFileDeltaList {
    writer: GzEncoder<File>,
}

impl JBackupFileDeltaList {
    pub fn new(mut writer: GzEncoder<File>) -> Result<Self, String> {
        simplify_result(writer.write("DL".as_bytes()))?;
        simplify_result(writer.write(&1u32.to_be_bytes()))?;
        Ok(JBackupFileDeltaList { writer })
    }

    /// Add a file delete operation to the delta list
    pub fn add_delete(&mut self, path: &str) -> Result<(), String> {
        self.add_string(path)?;
        simplify_result(self.writer.write(&FileChangeType::Deleted.to_bytes()))?;
        Ok(())
    }

    /// Add a file add operation to the delta list
    pub fn add_add(&mut self, path: &str, contents: &[u8]) -> Result<(), String> {
        self.add_string(path)?;
        simplify_result(self.writer.write(&FileChangeType::Added.to_bytes()))?;
        self.add_bytes(contents)?;
        Ok(())
    }

    /// Add a file modify operation to the delta list
    pub fn add_modify(&mut self, path: &str, xdelta: &[u8]) -> Result<(), String> {
        self.add_string(path)?;
        simplify_result(self.writer.write(&FileChangeType::Modified.to_bytes()))?;
        self.add_bytes(xdelta)?;
        Ok(())
    }

    pub fn try_finish(&mut self) -> Result<(), String> {
        simplify_result(self.writer.try_finish())?;
        Ok(())
    }

    fn add_string(&mut self, s: &str) -> Result<(), String> {
        self.add_bytes(s.as_bytes())
    }

    fn add_bytes(&mut self, bytes: &[u8]) -> Result<(), String> {
        simplify_result(
            self.writer
                .write(&u64::try_from(bytes.len()).unwrap().to_be_bytes()),
        )?;
        simplify_result(self.writer.write(bytes))?;
        Ok(())
    }
}
