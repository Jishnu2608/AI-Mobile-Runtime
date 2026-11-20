//! Prints the child-process environment the runtime would use, as JSON.
//! Usage: `cargo run -p tumr-env --example print_recipe -- <managed_root> [<android_sdk>]`
//! Used to verify isolation (ENV-008) with real tools.

fn main() {
    let mut args = std::env::args().skip(1);
    let root = std::path::PathBuf::from(args.next().expect("managed root"));
    let sdk = args.next().map(std::path::PathBuf::from);
    let ctx = tumr_env::ScanContext::from_system(root.clone());
    let dirs = tumr_env::recipe::IsolationDirs::under(&root);
    dirs.create().expect("create isolation folders");
    let sel = tumr_env::recipe::Selection {
        android_sdk: sdk,
        ..Default::default()
    };
    let env = tumr_env::recipe::child_env(&ctx, &sel);
    println!("{}", serde_json::to_string_pretty(&env).expect("json"));
}
