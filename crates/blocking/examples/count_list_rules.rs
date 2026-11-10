use adblock::lists::{parse_filters, ParseOptions};
use slate_blocking::REMOTE_FILTER_LIST_URLS;
use std::process::Command;

fn fetch_url(url: &str) -> Result<String, String> {
    let output = Command::new("curl")
        .args([
            "-sSL",
            "--compressed",
            "--proto",
            "=https",
            "--max-time",
            "120",
            url,
        ])
        .output()
        .map_err(|e| format!("Failed to spawn curl: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "curl failed with status {:?}",
            output.status.code()
        ));
    }
    String::from_utf8(output.stdout).map_err(|e| format!("UTF-8 decode failed: {e}"))
}

fn main() {
    println!("Counting exact rules per list in REMOTE_FILTER_LIST_URLS...");
    let mut results = Vec::new();

    for (name, url) in REMOTE_FILTER_LIST_URLS {
        print!("  Fetching and parsing '{}'...", name);
        match fetch_url(url) {
            Ok(content) => {
                let lines: Vec<&str> = content.lines().collect();
                let total_lines = lines.len();
                let (net, cos) = parse_filters(lines, false, ParseOptions::default());
                let net_count = net.len();
                let cos_count = cos.len();
                let total_rules = net_count + cos_count;
                println!(
                    " done (Lines: {}, Rules: {} [Network: {}, Cosmetic: {}])",
                    total_lines, total_rules, net_count, cos_count
                );
                results.push(serde_json::json!({
                    "name": name,
                    "url": url,
                    "raw_lines": total_lines,
                    "network_rules": net_count,
                    "cosmetic_rules": cos_count,
                    "total_valid_rules": total_rules
                }));
            }
            Err(e) => {
                println!(" failed: {e}");
                results.push(serde_json::json!({
                    "name": name,
                    "url": url,
                    "error": e
                }));
            }
        }
    }

    let out_json = serde_json::to_string_pretty(&results).unwrap();
    let out_path =
        "/home/earl/Desktop/Slate/benchmarks/results/realworld/filter_list_rule_counts.json";
    std::fs::write(out_path, &out_json).unwrap();
    println!("\nSaved exact rule counts to: {}", out_path);
}
