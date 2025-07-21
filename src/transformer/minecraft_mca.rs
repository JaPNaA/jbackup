use std::io::{Read, Write};

use flate2::{read::ZlibDecoder, write::ZlibEncoder};

use crate::{transformer::FileTransformer, util::io_util::simplify_result};

const REGION_WIDTH_CHUNK: usize = 32;
const REGION_HEIGHT_CHUNK: usize = 32;
const CHUNKS_IN_REGION: usize = REGION_WIDTH_CHUNK * REGION_HEIGHT_CHUNK;
const SECTOR_SIZE: usize = 4096;

// #[derive(Clone)]
pub struct McaTransformer {}

impl McaTransformer {
    pub fn new() -> McaTransformer {
        McaTransformer {}
    }

    fn accepts_file(file_path: &str) -> bool {
        file_path.ends_with(".mca")
    }
}

impl FileTransformer for McaTransformer {
    fn transform_in(&self, file_path: &str, contents: Vec<u8>) -> Result<Vec<u8>, String> {
        // this transformer only works with .mca files
        if !McaTransformer::accepts_file(file_path) {
            return Ok(contents);
        }

        let region = RegionFileFormatReader::new(contents);
        match transform_region_file_to_uncompressed(&region) {
            Ok(x) => Ok(x),
            Err(err) => Err(format!(
                "Failed to uncompress file '{}': {}",
                file_path, err
            )),
        }
    }

    fn transform_out(
        &self,
        file_path: &str,
        transformed_contents: Vec<u8>,
    ) -> Result<Vec<u8>, String> {
        // this transformer only works with .mca files
        if !McaTransformer::accepts_file(file_path) {
            return Ok(transformed_contents);
        }

        let region = RegionFileFormatReader::new(transformed_contents);
        match transform_region_file_to_compressed(&region) {
            Ok(x) => Ok(x),
            Err(err) => Err(format!("Failed to compress file '{}': {}", file_path, err)),
        }
    }
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

        if desc.is_exists() {
            let payload = reader.read_chunk_uncompressed(&desc)?;
            let mut encoder = ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
            simplify_result(encoder.write_all(&payload))?;
            let compressed_payload = simplify_result(encoder.finish())?;

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

    fn get_chunk_i(&self, i: usize) -> ChunkDescriptor {
        // file too small, must be empty or corrupt
        if self.contents.len() < SECTOR_SIZE * 2 {
            return ChunkDescriptor {
                offset: 0,
                sector_count: 0,
                timestamp: 0,
            };
        }

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
