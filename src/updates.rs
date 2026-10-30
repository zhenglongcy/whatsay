//! Self-update from GitHub releases, through fastframe-update.
//!
//! The crate checks for a newer release, refuses package-managed copies,
//! downloads and verifies the update against the publisher signature, and
//! hands it to a helper that installs it and rolls back if it does not start.
//! Whatsay keeps its names, its key, its proxy and its interface.

pub use fastframe_update::{
    CHECK_INTERVAL, DownloadState, Installation, Kind, Prepared, Release, Source, Unsupported,
    Updater,
};
use fastframe_update::{MacConfig, ReqwestTransport, UpdateConfig};

/// Our own releases channel, `owner/name`. Update checks query this
/// repository and nothing else. Never point this at the upstream
/// repository: this build must not offer upstream releases as its own
/// updates.
pub const UPDATE_REPO: &str = "zhenglongcy/whatsay";

/// Whatsay's releases and the names its installations have had.
pub const CONFIG: UpdateConfig = UpdateConfig {
    // Cask and bundle names from before the rename. This also accepts
    // fastsapp-* marker files and `fastsapp <version>` answers, which no
    // release produces.
    legacy_names: &["fastsapp"],
    macos: MacConfig {
        bundle_ids: &["me.paolino.fastsapp"],
        executable_names: &[],
        legacy_bundle_names: &["FastsApp.app"],
    },
    publisher_key: Some(include_str!("../assets/update-public-key.hex")),
    // Our own releases are signed with this key only. The private half
    // lives outside the repository and is never published.
    additional_publisher_keys: &[],
    // The repository comes from UPDATE_REPO, our own releases channel.
    // The slug stays "zapfast" so existing installs' marker files and
    // staging folders keep matching.
    ..UpdateConfig::new(UPDATE_REPO, "Whatsay", "zapfast", env!("CARGO_PKG_VERSION"))
};

/// An updater on the proxy-aware reqwest client, pointed at our own
/// releases channel ([`UPDATE_REPO`]).
pub fn updater() -> anyhow::Result<Updater> {
    let mut builder = reqwest::blocking::Client::builder();
    if let Some(proxy) = crate::proxy::reqwest_proxy() {
        builder = builder.proxy(proxy);
    }
    Ok(Updater::new(CONFIG, ReqwestTransport::new(builder)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_config_is_valid() {
        CONFIG.validate().unwrap();
        assert_eq!(CONFIG.current_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(CONFIG.slug, "zapfast");
    }

    #[test]
    fn remote_updates_point_at_our_own_channel() {
        // Our releases live on our own repository, never upstream's.
        assert_eq!(UPDATE_REPO, "zhenglongcy/whatsay");
        assert!(updater().is_ok());
    }
}
