pub mod minecraft_mca;

pub fn get_transformer(name: &str) -> Option<Box<dyn FileTransformer + Sync + Send>> {
    match name {
        "minecraft_mca" => Some(Box::from(minecraft_mca::McaTransformer::new())),
        _ => None,
    }
}

pub trait FileTransformer: Sync + Send {
    /// Transform a file before it's inserted into the archive.
    fn transform_in(&self, file_path: &str, raw_contents: Vec<u8>) -> Result<Vec<u8>, String>;

    /// Transform a file from an archive to the contents to be restored.
    fn transform_out(
        &self,
        file_path: &str,
        transformed_contents: Vec<u8>,
    ) -> Result<Vec<u8>, String>;
}
