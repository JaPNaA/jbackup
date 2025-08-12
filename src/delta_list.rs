use std::{
    fs::File,
    io::{BufReader, ErrorKind, Read, Write},
};

use flate2::{bufread::GzDecoder, write::GzEncoder};

use crate::util::{
    archive_utils::{TarReader, TarWriter},
    io_util::simplify_result,
};

pub fn generate_delta_list(
    mut start_tar: TarReader,
    mut end_tar: TarReader,
    mut delta_list: JBackupFileDeltaListWriter,
) -> Result<(), String> {
    let mut start_entries = simplify_result(start_tar.entries())?;
    let mut end_entries = simplify_result(end_tar.entries())?;

    let mut start_entry = start_entries.next();
    let mut end_entry = end_entries.next();

    loop {
        match (start_entry.take(), end_entry.take()) {
            (Some(Ok(mut start_entry_uw)), Some(Ok(mut end_entry_uw))) => {
                let start_path = get_entry_path(&start_entry_uw)?;
                let end_path = get_entry_path(&end_entry_uw)?;

                if start_path == end_path {
                    let start_buf = get_entry_data(&mut start_entry_uw)?;
                    let end_buf = get_entry_data(&mut end_entry_uw)?;

                    if let Some(res) = xdelta3::encode(&end_buf, &start_buf) {
                        delta_list.add(JBackupDelta {
                            path: start_path,
                            content: JBackupDeltaContent::Modified { xdelta: res },
                        })?;
                    } else {
                        // eprintln!("Warn: no xdelta output for {}", &start_path);
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
                    let buf = get_entry_data(&mut end_entry_uw)?;

                    delta_list.add(JBackupDelta {
                        path: end_path,
                        content: JBackupDeltaContent::Added { content: buf },
                    })?;

                    start_entry = Some(Ok(start_entry_uw));
                    end_entry = end_entries.next();
                }
            }
            (Some(Ok(start_entry_uw)), None) => {
                delta_list.add(JBackupDelta {
                    path: get_entry_path(&start_entry_uw)?,
                    content: JBackupDeltaContent::Deleted,
                })?;

                start_entry = start_entries.next();
            }

            (None, Some(Ok(mut end_entry_uw))) => {
                let buf = get_entry_data(&mut end_entry_uw)?;

                delta_list.add(JBackupDelta {
                    path: get_entry_path(&end_entry_uw)?,
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

pub fn restore_from_delta_list(
    mut start_tar: TarReader,
    mut end_tar: TarWriter,
    mut delta_list: JBackupFileDeltaListReader,
) -> Result<(), String> {
    let mut start_entries = simplify_result(start_tar.entries())?;
    let mut start_entry = start_entries.next();

    let mut delta_entry = delta_list.next()?;

    loop {
        match (start_entry.take(), delta_entry.take()) {
            (Some(Ok(mut start_entry_uw)), Some(delta_entry_uw)) => {
                let start_path = get_entry_path(&start_entry_uw)?;
                let delta_path = delta_entry_uw.path.clone();

                if start_path == delta_path {
                    match delta_entry_uw.content {
                        JBackupDeltaContent::Modified { xdelta } => {
                            let start_buf = get_entry_data(&mut start_entry_uw)?;

                            if let Some(res) = xdelta3::decode(&xdelta, &start_buf) {
                                add_tar_entry(&mut end_tar, &start_path, res)?;
                            } else {
                                add_tar_entry(&mut end_tar, &start_path, start_buf)?;
                                // eprintln!("Warn: No xdelta output for {}", &start_path);
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

                    add_tar_entry(&mut end_tar, &delta_entry_uw.path, content)?;

                    start_entry = Some(Ok(start_entry_uw));
                    delta_entry = delta_list.next()?;
                }
            }

            (Some(Ok(start_entry_uw)), None) => {
                let start_path = get_entry_path(&start_entry_uw)?;

                simplify_result(end_tar.append_data(
                    &mut start_entry_uw.header().clone(),
                    start_path,
                    start_entry_uw,
                ))?;

                start_entry = start_entries.next();
            }

            (None, Some(delta_entry_uw)) => {
                let end_path = delta_entry_uw.path;

                let JBackupDeltaContent::Added { content } = delta_entry_uw.content else {
                    return Err(format!(
                        "Patching conflict: Cannot operate on '{}' since that file doesn't exist.",
                        end_path
                    ));
                };

                add_tar_entry(&mut end_tar, &end_path, content)?;

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

fn get_entry_path(entry: &tar::Entry<'_, GzDecoder<BufReader<File>>>) -> Result<String, String> {
    if let Some(s) = simplify_result(entry.path())?.to_str() {
        Ok(String::from(s))
    } else {
        Err(String::from("Tar entry contains non-UTF-8 characters."))
    }
}

fn get_entry_data(
    entry: &mut tar::Entry<'_, GzDecoder<BufReader<File>>>,
) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    simplify_result(entry.read_to_end(&mut buf))?;
    Ok(buf)
}

fn add_tar_entry(
    archive: &mut tar::Builder<GzEncoder<File>>,
    path: &str,
    content: Vec<u8>,
) -> Result<(), String> {
    let mut header = tar::Header::new_gnu();
    header.set_size(content.len().try_into().unwrap());
    simplify_result(archive.append_data(&mut header, path, content.as_slice()))?;
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
pub struct JBackupFileDeltaListWriter {
    writer: GzEncoder<File>,
}

impl JBackupFileDeltaListWriter {
    pub fn new(mut writer: GzEncoder<File>) -> Result<Self, String> {
        simplify_result(writer.write_all("DL".as_bytes()))?;
        simplify_result(writer.write_all(&1u32.to_be_bytes()))?;
        Ok(JBackupFileDeltaListWriter { writer })
    }

    /// Add a file operation to the delta list
    fn add(&mut self, delta: JBackupDelta) -> Result<(), String> {
        self.add_string(&delta.path)?;

        match delta.content {
            JBackupDeltaContent::Deleted {} => {
                simplify_result(self.writer.write_all(&[1]))?;
            }
            JBackupDeltaContent::Modified { xdelta } => {
                simplify_result(self.writer.write_all(&[2]))?;
                self.add_bytes(&xdelta)?;
            }
            JBackupDeltaContent::Added { content } => {
                simplify_result(self.writer.write_all(&[3]))?;
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
                .write_all(&u64::try_from(bytes.len()).unwrap().to_be_bytes()),
        )?;
        simplify_result(self.writer.write_all(bytes))?;
        Ok(())
    }
}

pub struct JBackupFileDeltaListReader {
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
