use slate_blocking::{default_filter_cache_path, BlockingPipeline};
use std::fs;

fn main() {
    let path = default_filter_cache_path();
    println!("Checking cache at: {}", path.display());
    if !path.exists() {
        println!("Cache does not exist!");
        return;
    }

    let bytes = fs::read(&path).expect("Failed to read filters.dat");
    println!("Cache size: {} bytes", bytes.len());

    let pipeline = BlockingPipeline::new_with_cache_and_background_refresh(None);
    println!("App start loaded rule count: {}", pipeline.rule_count());
    let t0 = std::time::Instant::now();
    match pipeline.deserialize_cache(&bytes) {
        Ok(_) => {
            let elapsed = t0.elapsed();
            println!(
                "Successfully deserialized 21.8 MB filters.dat into adblock::Engine in: {:?}",
                elapsed
            );
        }
        Err(e) => {
            println!("Failed to deserialize cache: {:?}", e);
        }
    }
}
