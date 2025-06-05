use rustc_version::version;
use tracing::info;

fn main() {
    let version_info = version().unwrap();
    info!("cargo:rustc-env=RUSTC_VERSION={version_info}");
    info!("cargo:rerun-if-changed=Cargo.toml");
}
