use std::path::PathBuf;
fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: inspect_import <source-kind> <path>");
        std::process::exit(2);
    }
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .expect("home directory");
    match astralink_lib::importers::preview_file(&args[1], &PathBuf::from(&args[2]), &home) {
        Ok(preview) => {
            let value = serde_json::json!({"source": preview.source,"total": preview.profiles.len(),"issues": preview.issues.len(),"stats": preview.stats});
            println!("{}", value);
        }
        Err(error) => {
            eprintln!("{}", error);
            std::process::exit(1);
        }
    }
}
