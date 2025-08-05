use std::{
    collections::VecDeque,
    fs::File,
    io::{BufReader, ErrorKind, Read, Write},
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
    let output_builder = GzBuilder::new().write(output_file, Compression::default()); // todo: probably don't need global compression, since xdelta output might already be compressed
    let mut delta_list = JBackupFileDeltaListWriter::new(output_builder)?;

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
                        delta_list.add(JBackupDelta {
                            path: start_path,
                            content: JBackupDeltaContent::Modified { xdelta: res },
                        })?;
                    } else {
                        eprintln!("No xdelta output for {}", &start_path);
                    }

                    start_entry = start_entries.next();
                    end_entry = end_entries.next();
                } else if start_path < end_path {
                    delta_list.add(JBackupDelta {
                        path: start_path.to_string(),
                        content: JBackupDeltaContent::Deleted,
                    })?;

                    start_entry = start_entries.next();
                    end_entry = Some(Ok(end_entry_uw));
                } else {
                    let mut buf = Vec::new();
                    let end_path = end_path.to_string();
                    simplify_result(end_entry_uw.read_to_end(&mut buf))?;
                    delta_list.add(JBackupDelta {
                        path: end_path,
                        content: JBackupDeltaContent::Added { content: buf },
                    })?;

                    start_entry = Some(Ok(start_entry_uw));
                    end_entry = end_entries.next();
                }
            }
            (Some(Ok(start_entry_uw)), None) => {
                // todo: duplicated code
                let start_path = simplify_result(start_entry_uw.path())?;
                let start_path = start_path.to_string_lossy();

                delta_list.add(JBackupDelta {
                    path: start_path.to_string(),
                    content: JBackupDeltaContent::Deleted,
                })?;

                start_entry = start_entries.next();
            }

            (None, Some(Ok(mut end_entry_uw))) => {
                // todo: duplicated code
                let end_path = simplify_result(end_entry_uw.path())?;
                let end_path = end_path.to_string_lossy();

                let mut buf = Vec::new();
                let end_path = end_path.to_string();
                simplify_result(end_entry_uw.read_to_end(&mut buf))?;
                delta_list.add(JBackupDelta {
                    path: end_path,
                    content: JBackupDeltaContent::Added { content: buf },
                })?;

                end_entry = end_entries.next();
            }
            (None, None) => {
                break;
            }
            _ => {
                return Err(String::from(
                    "Unknown error occurred while reading input archives",
                ));
            }
        }
    }

    delta_list.try_finish()?;

    Ok(())
}

pub fn inverse(mut args: VecDeque<String>) -> Result<(), String> {
    let Some(start_filename) = args.pop_front() else {
        return Err(String::from("Must provide start file"));
    };
    let Some(end_filename) = args.pop_front() else {
        return Err(String::from("Must provide ending file"));
    };
    let Some(delta_list_filename) = args.pop_front() else {
        return Err(String::from("Must provide output file"));
    };

    let start_file = simplify_result(File::open(start_filename))?;
    let end_file = simplify_result(File::create(end_filename))?;
    let delta_list_file = simplify_result(File::open(delta_list_filename))?;

    let start_dec = GzDecoder::new(BufReader::new(start_file));
    let end_enc = GzBuilder::new().write(end_file, Compression::default());
    let output_dec = GzDecoder::new(BufReader::new(delta_list_file));
    let mut delta_list = JBackupFileDeltaListReader::new(output_dec)?;

    let mut start_tar = tar::Archive::new(start_dec);
    let mut end_tar = tar::Builder::new(end_enc);

    let mut start_entries = simplify_result(start_tar.entries())?;
    let mut start_entry = start_entries.next();

    let mut delta_entry = delta_list.next()?;

    loop {
        match (start_entry.take(), delta_entry.take()) {
            (Some(Ok(mut start_entry_uw)), Some(delta_entry_uw)) => {
                let start_path = simplify_result(start_entry_uw.path())?;
                let start_path = start_path.to_string_lossy().to_string();
                let delta_path = delta_entry_uw.path.clone();

                if start_path == delta_path {
                    match delta_entry_uw.content {
                        JBackupDeltaContent::Modified { xdelta } => {
                            println!("Applying delta to {}", start_path);

                            let mut start_buf = Vec::new();
                            simplify_result(start_entry_uw.read_to_end(&mut start_buf))?;

                            if let Some(res) = xdelta3::decode(&xdelta, &start_buf) {
                                simplify_result(end_tar.append_data(
                                    &mut start_entry_uw.header().clone(),
                                    start_path,
                                    res.as_slice(),
                                ))?;
                            } else {
                                eprintln!("No xdelta output for {}", &start_path);
                            }
                        }
                        JBackupDeltaContent::Deleted => {
                            // do nothing
                        }
                        JBackupDeltaContent::Added { content: _ } => {
                            return Err(format!(
                                "Patching conflict: Delta contains an Add operation on '{}' that already exists.",
                                start_path
                            ));
                        }
                    };

                    start_entry = start_entries.next();
                    delta_entry = delta_list.next()?;
                } else if start_path < delta_path {
                    simplify_result(end_tar.append_data(
                        &mut start_entry_uw.header().clone(),
                        start_path,
                        start_entry_uw,
                    ))?;

                    start_entry = start_entries.next();
                    delta_entry = Some(delta_entry_uw);
                } else {
                    let JBackupDeltaContent::Added { content } = delta_entry_uw.content else {
                        return Err(format!(
                            "Patching conflict: Cannot operate on '{}' since that file doesn't exist.",
                            delta_entry_uw.path
                        ));
                    };

                    let mut header = tar::Header::new_gnu();
                    header.set_size(content.len().try_into().unwrap());

                    simplify_result(end_tar.append_data(
                        &mut header,
                        delta_entry_uw.path,
                        content.as_slice(),
                    ))?;

                    start_entry = Some(Ok(start_entry_uw));
                    delta_entry = delta_list.next()?;
                }
            }

            (Some(Ok(start_entry_uw)), None) => {
                // todo: duplicated code
                let start_path = simplify_result(start_entry_uw.path())?;
                let start_path = start_path.to_string_lossy().to_string();

                simplify_result(end_tar.append_data(
                    &mut start_entry_uw.header().clone(),
                    start_path,
                    start_entry_uw,
                ))?;

                start_entry = start_entries.next();
            }

            (None, Some(delta_entry_uw)) => {
                // todo: duplicated code
                let end_path = delta_entry_uw.path;

                let JBackupDeltaContent::Added { content } = delta_entry_uw.content else {
                    return Err(format!(
                        "Patching conflict: Cannot operate on '{}' since that file doesn't exist.",
                        end_path
                    ));
                };

                let mut header = tar::Header::new_gnu();
                header.set_size(content.len().try_into().unwrap());

                simplify_result(end_tar.append_data(&mut header, end_path, content.as_slice()))?;

                delta_entry = delta_list.next()?;
            }

            (None, None) => {
                break;
            }
            _ => {}
        }
    }

    simplify_result(end_tar.into_inner())?;

    Ok(())
}

struct JBackupDelta {
    path: String,
    content: JBackupDeltaContent,
}

enum JBackupDeltaContent {
    /// Serialized id: 1
    Deleted,
    /// Serialized id: 2
    Modified { xdelta: Vec<u8> },
    /// Serialized id: 3
    Added { content: Vec<u8> },
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
struct JBackupFileDeltaListWriter {
    writer: GzEncoder<File>,
}

impl JBackupFileDeltaListWriter {
    pub fn new(mut writer: GzEncoder<File>) -> Result<Self, String> {
        simplify_result(writer.write("DL".as_bytes()))?;
        simplify_result(writer.write(&1u32.to_be_bytes()))?;
        Ok(JBackupFileDeltaListWriter { writer })
    }

    /// Add a file operation to the delta list
    pub fn add(&mut self, delta: JBackupDelta) -> Result<(), String> {
        self.add_string(&delta.path)?;

        match delta.content {
            JBackupDeltaContent::Deleted {} => {
                simplify_result(self.writer.write(&[1]))?;
            }
            JBackupDeltaContent::Modified { xdelta } => {
                simplify_result(self.writer.write(&[2]))?;
                self.add_bytes(&xdelta)?;
            }
            JBackupDeltaContent::Added { content } => {
                simplify_result(self.writer.write(&[3]))?;
                self.add_bytes(&content)?;
            }
        };

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

struct JBackupFileDeltaListReader {
    reader: GzDecoder<BufReader<File>>,
}

impl JBackupFileDeltaListReader {
    pub fn new(mut reader: GzDecoder<BufReader<File>>) -> Result<Self, String> {
        let mut header = [0u8; 2 + 4];
        if let Some(e) = reader.read_exact(&mut header).err() {
            if e.kind() == ErrorKind::UnexpectedEof {
                return Err(String::from("File too short, cannot be a delta list."));
            } else {
                return Err(format!(
                    "Unexpected IO Error when reading delta list: {}",
                    e.to_string()
                ));
            }
        }

        if header == [b'D', b'L', 0, 0, 0, 1] {
            Ok(JBackupFileDeltaListReader { reader })
        } else {
            Err(String::from(
                "Header magic number doesn't match. Input file is not a delta list.",
            ))
        }
    }

    fn next(&mut self) -> Result<Option<JBackupDelta>, String> {
        let Ok(path) = self.read_string() else {
            return Ok(None);
        };

        println!("Reading path: {}", path);

        let op_type = self.read_u8()?;

        let content: JBackupDeltaContent = match op_type {
            1 => JBackupDeltaContent::Deleted,
            2 => JBackupDeltaContent::Modified {
                xdelta: self.read_bytes()?,
            },
            3 => JBackupDeltaContent::Added {
                content: self.read_bytes()?,
            },
            _ => return Err(format!("Unexpected operation with number '{}'", op_type)),
        };

        Ok(Some(JBackupDelta { path, content }))
    }

    fn read_string(&mut self) -> Result<String, String> {
        simplify_result(String::from_utf8(self.read_bytes()?))
    }

    fn read_bytes(&mut self) -> Result<Vec<u8>, String> {
        let mut bytes_len_buff = [0u8; 8];
        simplify_result(self.reader.read_exact(&mut bytes_len_buff))?;

        let bytes_len = u64::from_be_bytes(bytes_len_buff);
        if bytes_len > 1_000_000_000 {
            panic!("Trying to read '{}' bytes", bytes_len);
        }

        let mut v = vec![0u8; bytes_len.try_into().unwrap()];
        simplify_result(self.reader.read_exact(&mut v))?;

        Ok(v)
    }

    fn read_u8(&mut self) -> Result<u8, String> {
        let mut bytes = [0u8; 1];
        simplify_result(self.reader.read_exact(&mut bytes))?;
        Ok(bytes[0])
    }
}
