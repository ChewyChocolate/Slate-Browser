use slate_blocking::{default_filter_cache_path, BlockingPipeline};

fn main() {
    let path = default_filter_cache_path();
    println!("Rebuilding filter cache at: {}", path.display());
    let pipeline = BlockingPipeline::new_with_default_lists();
    match pipeline.refresh_remote_lists_and_save_cache(&path) {
        Ok(bytes) => {
            println!("Successfully refreshed all remote lists (downloaded {bytes} bytes)!");
            let meta = std::fs::metadata(&path).unwrap();
            println!("New filters.dat size on disk: {} bytes", meta.len());
        }
        Err(e) => {
            eprintln!("Error refreshing remote lists: {:?}", e);
            std::process::exit(1);
        }
    }
}
