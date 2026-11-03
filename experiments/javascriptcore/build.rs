fn main() {
    // WebKitGTK's JavaScriptCore (Fedora: javascriptcoregtk4.1-devel).
    let output = std::process::Command::new("pkg-config")
        .args(["--libs", "javascriptcoregtk-4.1"])
        .output()
        .expect("pkg-config is required");
    assert!(output.status.success(), "javascriptcoregtk-4.1 not found by pkg-config");
    for flag in String::from_utf8_lossy(&output.stdout).split_whitespace() {
        if let Some(lib) = flag.strip_prefix("-l") {
            println!("cargo:rustc-link-lib={lib}");
        } else if let Some(dir) = flag.strip_prefix("-L") {
            println!("cargo:rustc-link-search={dir}");
        }
    }
}
