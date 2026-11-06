//! Copies the design tokens into the web views' folder (one source: `design/tokens.css`, D-003), then runs
//! Tauri's build step (Windows resources, the embedded `ui/` files).

fn main() {
    let tokens = std::path::Path::new("../../design/tokens.css");
    println!("cargo:rerun-if-changed={}", tokens.display());
    std::fs::copy(tokens, "ui/tokens.css").expect("design/tokens.css must exist");
    tauri_build::build();
}
