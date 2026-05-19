use gpui::{AssetSource, Result as GpuiResult, SharedString};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "./assets"]
#[include = "*.svg"]
#[include = "*.png"]
#[include = "*.ico"]
#[exclude = "*.DS_Store"]
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> GpuiResult<Option<std::borrow::Cow<'static, [u8]>>> {
        Ok(Self::get(path).map(|f| f.data))
    }

    fn list(&self, path: &str) -> GpuiResult<Vec<SharedString>> {
        Ok(Self::iter()
            .filter_map(|p| {
                if p.starts_with(path) {
                    Some(p.into())
                } else {
                    None
                }
            })
            .collect())
    }
}

fn main() {
    println!("Listing all embedded assets:");
    for path in Assets::iter() {
        println!("  - {}", path);
    }

    println!("\nTrying to load more.svg:");
    match Assets::get("more.svg") {
        Some(f) => println!("  Found! size={}", f.data.len()),
        None => println!("  NOT FOUND"),
    }

    println!("\nTrying to load assets/more.svg:");
    match Assets::get("assets/more.svg") {
        Some(f) => println!("  Found! size={}", f.data.len()),
        None => println!("  NOT FOUND"),
    }
}