pub mod minecraft_mca;

// todo: return the transformer, not just a bool
pub fn get_transformer(name: &str) -> bool {
    match name {
        "minecraft_mca" => true,
        _ => false,
    }
}
