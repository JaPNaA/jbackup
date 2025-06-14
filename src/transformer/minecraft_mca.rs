use std::{
    collections::VecDeque,
    fs,
    io::{Read, Write},
};

use flate2::{read::ZlibDecoder, write::ZlibEncoder};

use crate::io_util::simplify_result;

const REGION_WIDTH_CHUNK: usize = 32;
const REGION_HEIGHT_CHUNK: usize = 32;
const CHUNKS_IN_REGION: usize = REGION_WIDTH_CHUNK * REGION_HEIGHT_CHUNK;
const SECTOR_SIZE: usize = 4096;

pub fn __debug_transform(mut args: VecDeque<String>) -> Result<(), String> {
    let dir_path = match args.pop_front() {
        None => {
            return Err(String::from("Path to file to transform not provided"));
        }
        Some(x) => x,
    };

    // get list of region files from directory
    // let files = simplify_result(fs::read_dir(&dir_path))?;
    // let mut region_files = HashSet::new();

    // for file in files {
    //     match file {
    //         Err(_) => {}
    //         Ok(file) => match file.file_name().to_str() {
    //             None => {}
    //             Some(file_name) => {
    //                 let mut parts = file_name.split('.');

    //                 match (parts.next(), parts.next(), parts.next(), parts.next()) {
    //                     (Some("r"), Some(x), Some(y), Some("mca")) => {
    //                         let x = i32::from_str_radix(x, 10);
    //                         let y = i32::from_str_radix(y, 10);

    //                         match (x, y) {
    //                             (Ok(x), Ok(y)) => {
    //                                 region_files.insert((x, y));
    //                             }
    //                             _ => {}
    //                         }
    //                     }
    //                     _ => {}
    //                 }
    //             }
    //         },
    //     }
    // }

    let region_file = (0, 0);

    // for region_file in region_files {
    let contents = simplify_result(fs::read(
        String::from(&dir_path)
            + "/r."
            + &region_file.0.to_string()
            + "."
            + &region_file.1.to_string()
            + ".mca",
    ))?;

    let region = RegionFileFormatReader::new(contents);
    // simplify_result(std::io::stdout().write_all(&transform_region_file_to_uncompressed(&region)?))?;
    simplify_result(std::io::stdout().write_all(&transform_region_file_to_compressed(&region)?))?;
    // println!("{}", nbt::to_human_readable(&mut data.iter()));
    // }

    Ok(())
}

fn transform_region_file_to_uncompressed(
    reader: &RegionFileFormatReader,
) -> Result<Vec<u8>, String> {
    let mut writer = RegionFileFormatWriter::new();

    for i in 0..CHUNKS_IN_REGION {
        let desc = reader.get_chunk_i(i);
        if desc.is_exists() {
            writer.add_chunk(i, desc.timestamp, 3, reader.read_chunk_uncompressed(&desc)?);
        }
    }

    writer.serialize()
}

fn transform_region_file_to_compressed(reader: &RegionFileFormatReader) -> Result<Vec<u8>, String> {
    let mut writer = RegionFileFormatWriter::new();

    for i in 0..CHUNKS_IN_REGION {
        let desc = reader.get_chunk_i(i);

        let payload = reader.read_chunk_uncompressed(&desc)?;
        let mut encoder = ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        simplify_result(encoder.write_all(&payload))?;
        let compressed_payload = simplify_result(encoder.finish())?;

        if desc.is_exists() {
            // compression scheme 2 = zlib
            writer.add_chunk(i, desc.timestamp, 2, compressed_payload);
        }
    }

    writer.serialize()
}

struct RegionFileFormatReader {
    contents: Vec<u8>,
}

impl RegionFileFormatReader {
    fn new(contents: Vec<u8>) -> RegionFileFormatReader {
        RegionFileFormatReader { contents }
    }

    pub fn read_chunk_uncompressed(&self, descriptor: &ChunkDescriptor) -> Result<Vec<u8>, String> {
        if !descriptor.is_exists() {
            return Err(String::from(
                "Descriptor does not point to an existing chunk",
            ));
        }

        let offset_bytes = descriptor.offset as usize * SECTOR_SIZE;
        let length = i32::from_be_bytes([
            self.contents[offset_bytes],
            self.contents[offset_bytes + 1],
            self.contents[offset_bytes + 2],
            self.contents[offset_bytes + 3],
        ]);
        if length <= 0 {
            return Err(String::from("Length must be a positive number"));
        }
        let length = length as usize;
        if length > (descriptor.sector_count as usize) * SECTOR_SIZE {
            return Err(String::from("Chunk length is larger than the sector count"));
        }

        let compression_type = self.contents[offset_bytes + 4];
        let data = &self.contents[offset_bytes + 5..offset_bytes + 5 + length - 1];

        match compression_type {
            2 => {
                let mut vec = Vec::new();
                let mut dec = ZlibDecoder::new(data);
                simplify_result(dec.read_to_end(&mut vec))?;
                Ok(vec)
            }
            3 => Ok(data.to_vec()),
            _ => Err(String::from("Unsupported compression algorithm")),
        }
    }

    pub fn get_chunk_xy(&self, local_x: u8, local_y: u8) -> ChunkDescriptor {
        let i = local_y as usize * REGION_WIDTH_CHUNK + local_x as usize;

        let offset = u32::from_be_bytes([
            0,
            self.contents[i * 4],
            self.contents[i * 4 + 1],
            self.contents[i * 4 + 2],
        ]);
        let sector_count = self.contents[i * 4 + 3];
        let timestamp = u32::from_be_bytes([
            self.contents[4096 + i * 4],
            self.contents[4096 + i * 4 + 1],
            self.contents[4096 + i * 4 + 2],
            self.contents[4096 + i * 4 + 3],
        ]);

        ChunkDescriptor {
            offset,
            sector_count,
            timestamp,
        }
    }

    fn get_chunk_i(&self, i: usize) -> ChunkDescriptor {
        let offset = u32::from_be_bytes([
            0,
            self.contents[i * 4],
            self.contents[i * 4 + 1],
            self.contents[i * 4 + 2],
        ]);
        let sector_count = self.contents[i * 4 + 3];
        let timestamp = u32::from_be_bytes([
            self.contents[4096 + i * 4],
            self.contents[4096 + i * 4 + 1],
            self.contents[4096 + i * 4 + 2],
            self.contents[4096 + i * 4 + 3],
        ]);

        ChunkDescriptor {
            offset,
            sector_count,
            timestamp,
        }
    }

    // fn read_chunk_location_table(&self) -> Vec<bool> {
    //     let mut grid = Vec::with_capacity(1024);

    //     if self.contents.len() < 4 * 1024 {
    //         // invalid file
    //         grid.resize(1024, false);
    //         return grid;
    //     }

    //     for i in 0..1024 {
    //         let x = i % 32;
    //         let y = i / 32;

    //         let offset = u32::from_be_bytes([
    //             0,
    //             self.contents[i * 4],
    //             self.contents[i * 4 + 1],
    //             self.contents[i * 4 + 2],
    //         ]);
    //         let sector_count = self.contents[i * 4 + 3];

    //         if offset == 0 && sector_count == 0 {
    //             grid.push(false);
    //         } else {
    //             grid.push(true)
    //         }
    //     }

    //     grid
    // }
}

#[derive(Clone, Copy)]
struct ChunkDescriptor {
    /// in number of sectors
    offset: u32,
    /// a sector is 4096 bytes
    sector_count: u8,
    /// last modified timestamp in unix epoch seconds
    timestamp: u32,
}

impl ChunkDescriptor {
    pub fn is_exists(&self) -> bool {
        self.offset != 0 && self.sector_count != 0
    }
}

struct RegionFileFormatWriter {
    /// Tracks the chunk index (y * 32 + x) to ensure that
    /// chunks are being added in ascending chunk index order
    /// (arbitrary constraint to help reduce diffs.)
    next_chunk_i_must_be_ge: usize,
    chunks: Vec<ChunkDescriptor>,
    next_sector_i: usize,
    payload: Vec<u8>,
}

impl RegionFileFormatWriter {
    fn new() -> RegionFileFormatWriter {
        RegionFileFormatWriter {
            next_chunk_i_must_be_ge: 0,
            chunks: {
                let mut vec = Vec::new();
                vec.resize(
                    CHUNKS_IN_REGION,
                    ChunkDescriptor {
                        offset: 0,
                        sector_count: 0,
                        timestamp: 0,
                    },
                );
                vec
            },
            next_sector_i: 2, // start at 1, since header takes first two sectors
            payload: Vec::new(),
        }
    }

    /// Add a chunk to the region file. This method must be called on chunks
    /// in left-to-right, then up-down order (English reading direction).
    ///
    /// This limitation is required to help reduce diffs between worlds.
    fn add_chunk(
        &mut self,
        chunk_i: usize,
        timestamp: u32,
        compression_scheme: u8,
        payload: Vec<u8>,
    ) {
        // let chunk_i = (local_chunk_y as usize) * REGION_WIDTH_CHUNK + (local_chunk_x as usize);
        if chunk_i < self.next_chunk_i_must_be_ge {
            panic!(
                "RegionFileFormatWriter only accepts chunks in left-to-right, then up-down order."
            )
        }

        let total_chunk_payload_size = payload.len() + 5;
        let sector_count = total_chunk_payload_size.div_ceil(SECTOR_SIZE);

        self.chunks[chunk_i] = ChunkDescriptor {
            offset: self.next_sector_i as u32,
            sector_count: sector_count as u8,
            timestamp: timestamp,
        };

        self.next_sector_i += sector_count;

        // Calculate the new target payload length. Each chunk must be sector-aligned.
        let new_payload_len = self.payload.len() + sector_count * SECTOR_SIZE;

        self.payload
            .extend_from_slice(&((payload.len() + 1) as i32).to_be_bytes());
        self.payload.push(compression_scheme);
        self.payload.extend(payload);

        for _ in 0..(new_payload_len - self.payload.len()) {
            self.payload.push(0);
        }
    }

    fn serialize(&self) -> Result<Vec<u8>, String> {
        let mut result = Vec::new();

        // Chunk location header
        for i in 0..CHUNKS_IN_REGION {
            let chunk = self.chunks[i];
            if chunk.offset > 0xFFFFFF {
                return Err(String::from("Chunk offset is too large."));
            }
            result.extend_from_slice(&chunk.offset.to_be_bytes()[1..]);
            result.push(chunk.sector_count);
        }

        // Chunk timestamp header
        for i in 0..CHUNKS_IN_REGION {
            let chunk = self.chunks[i];
            result.extend_from_slice(&chunk.timestamp.to_be_bytes());
        }

        // include payload
        result.extend(&self.payload);

        Ok(result)
    }
}

/// Little module to read NBT data and verify data is there
/// For testing only
mod nbt {
    use std::slice;

    pub fn to_human_readable(mut nbt_data: &mut slice::Iter<u8>) -> String {
        let mut result = String::new();

        let mut reached_end = false;

        while !reached_end {
            let s = match nbt_data.next() {
                // End
                Some(0) => {
                    reached_end = true;
                    String::from("}")
                }
                Some(type_id) => {
                    let tag_name = get_nbt_str(&mut nbt_data);
                    String::from("\"")
                        + &tag_name
                        + "\": "
                        + &payload_to_human_readable(*type_id, &mut nbt_data)
                        + ", "
                }
                None => {
                    reached_end = true;
                    String::from("EOF")
                }
            };

            result.push_str(&s);
        }

        result
    }

    pub fn payload_to_human_readable(type_id: u8, mut nbt_data: &mut slice::Iter<u8>) -> String {
        match type_id {
            1 => {
                // 1 byte int
                let v = get_nbt_i8(&mut nbt_data);
                match v {
                    Some(v) => v.to_string(),
                    None => String::from("<Invalid i8>"),
                }
            }
            2 => {
                // 2 byte int
                let v = get_nbt_i16(&mut nbt_data);
                match v {
                    Some(v) => v.to_string(),
                    None => String::from("<Invalid i16>"),
                }
            }
            3 => {
                // 32 bit int
                let v = get_nbt_i32(&mut nbt_data);
                match v {
                    Some(v) => v.to_string(),
                    None => String::from("<Invalid i32>"),
                }
            }
            4 => {
                // 64 bit int
                let v = get_nbt_i64(&mut nbt_data);
                match v {
                    Some(v) => v.to_string(),
                    None => String::from("<Invalid i64>"),
                }
            }
            5 => {
                // 32 bit float
                let v = get_nbt_f32(&mut nbt_data);
                match v {
                    Some(v) => v.to_string(),
                    None => String::from("<Invalid f32>"),
                }
            }
            6 => {
                // 64 bit float
                let v = get_nbt_f64(&mut nbt_data);
                match v {
                    Some(v) => v.to_string(),
                    None => String::from("<Invalid f64>"),
                }
            }
            7 => {
                // Byte Array
                let list_length = get_nbt_i32(&mut nbt_data);

                match list_length {
                    Some(list_length) => {
                        let mut s = String::from("[");

                        for _ in 0..list_length {
                            match nbt_data.next() {
                                Some(v) => {
                                    s.push_str(&(v.to_string() + ", "));
                                }
                                None => {
                                    s.push_str("<Unexpected EOF in Byte Array>");
                                    break;
                                }
                            }
                        }

                        s + "]"
                    }
                    None => String::from("<Expected Array Length, got EOF>"),
                }
            }
            8 => {
                // String
                String::from("\"") + &get_nbt_str(&mut nbt_data) + &"\""
            }
            9 => {
                // List
                let id = nbt_data.next();
                let list_length = get_nbt_i32(&mut nbt_data);

                match (id, list_length) {
                    (Some(id), Some(list_length)) => {
                        let mut s = String::from("[");

                        for _ in 0..list_length {
                            s.push_str(&(payload_to_human_readable(*id, nbt_data) + ", "));
                        }

                        s + "]"
                    }
                    _ => String::from("<Invalid list>"),
                }
            }
            10 => {
                // Compound
                String::from("{") + &to_human_readable(&mut nbt_data)
            }
            11 => {
                // Int Array
                let list_length = get_nbt_i32(&mut nbt_data);

                match list_length {
                    Some(list_length) => {
                        let mut s = String::from("[");

                        for _ in 0..list_length {
                            match get_nbt_i32(nbt_data) {
                                Some(v) => {
                                    s.push_str(&(v.to_string() + ", "));
                                }
                                None => {
                                    s.push_str("<Unexpected EOF in Int Array>");
                                    break;
                                }
                            }
                        }

                        s + "]"
                    }
                    None => String::from("<Expected Array Length, got EOF>"),
                }
            }
            12 => {
                // Int Array
                let list_length = get_nbt_i32(&mut nbt_data);

                match list_length {
                    Some(list_length) => {
                        let mut s = String::from("[");

                        for _ in 0..list_length {
                            match get_nbt_i64(nbt_data) {
                                Some(v) => {
                                    s.push_str(&(v.to_string() + ", "));
                                }
                                None => {
                                    s.push_str("<Unexpected EOF in Long Array>");
                                    break;
                                }
                            }
                        }

                        s + "]"
                    }
                    None => String::from("<Expected Array Length, got EOF>"),
                }
            }
            x => String::from("Unknown tag #") + &x.to_string(),
        }
    }

    /// Reads a string, then returns the number of bytes to advance by
    fn get_nbt_str(nbt_data: &mut slice::Iter<u8>) -> String {
        match (nbt_data.next(), nbt_data.next()) {
            (Some(b1), Some(b2)) => {
                let name_len = u16::from_be_bytes([*b1, *b2]) as usize;
                let mut s = Vec::with_capacity(name_len);
                for _ in 0..name_len {
                    match nbt_data.next() {
                        Some(c) => s.push(*c),
                        None => {
                            return String::from("<Unexpected EOF in String>");
                        }
                    }
                }

                match str::from_utf8(&s) {
                    Ok(name) => String::from(name),
                    Err(_) => String::from("<Invalid string>"),
                }
            }
            _ => String::from("<Invalid string>"),
        }
    }

    fn get_nbt_i8(nbt_data: &mut slice::Iter<u8>) -> Option<i8> {
        let b1 = nbt_data.next();

        match b1 {
            Some(b1) => Some(i8::from_be_bytes([*b1])),
            _ => None,
        }
    }

    fn get_nbt_i16(nbt_data: &mut slice::Iter<u8>) -> Option<i16> {
        let b1 = nbt_data.next();
        let b2 = nbt_data.next();

        match (b1, b2) {
            (Some(b1), Some(b2)) => Some(i16::from_be_bytes([*b1, *b2])),
            _ => None,
        }
    }

    fn get_nbt_i32(nbt_data: &mut slice::Iter<u8>) -> Option<i32> {
        let b1 = nbt_data.next();
        let b2 = nbt_data.next();
        let b3 = nbt_data.next();
        let b4 = nbt_data.next();

        match (b1, b2, b3, b4) {
            (Some(b1), Some(b2), Some(b3), Some(b4)) => {
                Some(i32::from_be_bytes([*b1, *b2, *b3, *b4]))
            }
            _ => None,
        }
    }

    fn get_nbt_i64(nbt_data: &mut slice::Iter<u8>) -> Option<i64> {
        let b1 = nbt_data.next();
        let b2 = nbt_data.next();
        let b3 = nbt_data.next();
        let b4 = nbt_data.next();
        let b5 = nbt_data.next();
        let b6 = nbt_data.next();
        let b7 = nbt_data.next();
        let b8 = nbt_data.next();

        match (b1, b2, b3, b4, b5, b6, b7, b8) {
            (Some(b1), Some(b2), Some(b3), Some(b4), Some(b5), Some(b6), Some(b7), Some(b8)) => {
                Some(i64::from_be_bytes([*b1, *b2, *b3, *b4, *b5, *b6, *b7, *b8]))
            }
            _ => None,
        }
    }

    fn get_nbt_f32(nbt_data: &mut slice::Iter<u8>) -> Option<f32> {
        let b1 = nbt_data.next();
        let b2 = nbt_data.next();
        let b3 = nbt_data.next();
        let b4 = nbt_data.next();

        match (b1, b2, b3, b4) {
            (Some(b1), Some(b2), Some(b3), Some(b4)) => {
                Some(f32::from_be_bytes([*b1, *b2, *b3, *b4]))
            }
            _ => None,
        }
    }

    fn get_nbt_f64(nbt_data: &mut slice::Iter<u8>) -> Option<f64> {
        let b1 = nbt_data.next();
        let b2 = nbt_data.next();
        let b3 = nbt_data.next();
        let b4 = nbt_data.next();
        let b5 = nbt_data.next();
        let b6 = nbt_data.next();
        let b7 = nbt_data.next();
        let b8 = nbt_data.next();

        match (b1, b2, b3, b4, b5, b6, b7, b8) {
            (Some(b1), Some(b2), Some(b3), Some(b4), Some(b5), Some(b6), Some(b7), Some(b8)) => {
                Some(f64::from_be_bytes([*b1, *b2, *b3, *b4, *b5, *b6, *b7, *b8]))
            }
            _ => None,
        }
    }
}
