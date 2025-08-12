use std::{fs::File, io::BufReader};

use flate2::{GzBuilder, bufread::GzDecoder, write::GzEncoder};
use gzp::Compression;

use crate::{
    delta_list::{JBackupFileDeltaListReader, JBackupFileDeltaListWriter},
    util::io_util::simplify_result,
};

pub type TarReader = tar::Archive<GzDecoder<BufReader<File>>>;
pub type TarWriter = tar::Builder<GzEncoder<File>>;

pub fn open_tar_gz(filename: &str) -> Result<TarReader, String> {
    let file = simplify_result(File::open(filename))?;
    let gz_dec = GzDecoder::new(BufReader::new(file));
    Ok(tar::Archive::new(gz_dec))
}

pub fn create_tar_gz(filename: &str) -> Result<TarWriter, String> {
    let file = simplify_result(File::create(filename))?;
    let gz_builder = GzBuilder::new().write(file, Compression::fast());
    Ok(tar::Builder::new(gz_builder))
}

pub fn open_delta_list(filename: &str) -> Result<JBackupFileDeltaListReader, String> {
    let file = simplify_result(File::open(filename))?;
    let gz_dec = GzDecoder::new(BufReader::new(file));
    Ok(JBackupFileDeltaListReader::new(gz_dec)?)
}

pub fn create_delta_list(filename: &str) -> Result<JBackupFileDeltaListWriter, String> {
    let output_file = simplify_result(File::create(filename))?;
    let output_builder = GzBuilder::new().write(output_file, Compression::default()); // todo: probably don't need global compression, since xdelta output might already be compressed
    Ok(JBackupFileDeltaListWriter::new(output_builder)?)
}
