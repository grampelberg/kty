//! Build script for kty.

static PH_VAR: &str = "POSTHOG_API_KEY";

fn main() {
    if let Some(key) = std::env::var_os(PH_VAR) {
        println!("cargo:rustc-env={}={}", PH_VAR, key.to_string_lossy());
    }
}
