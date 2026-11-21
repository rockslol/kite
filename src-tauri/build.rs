fn main() {
    // Last.fm's shared secret signs scrobbles. It isn't in the repository: it comes from the
    // KITE_LASTFM_SECRET environment variable (CI) or a local `lastfm.secret` file. Builds
    // without it run fine, just without Last.fm.
    println!("cargo:rerun-if-env-changed=KITE_LASTFM_SECRET");
    println!("cargo:rerun-if-changed=lastfm.secret");
    if std::env::var("KITE_LASTFM_SECRET").is_err() {
        if let Ok(secret) = std::fs::read_to_string("lastfm.secret") {
            println!("cargo:rustc-env=KITE_LASTFM_SECRET={}", secret.trim());
        }
    }
    tauri_build::build()
}
