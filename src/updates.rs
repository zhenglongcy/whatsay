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

/// Our own releases repository, `owner/name`, that remote update checks
/// read. While our own releases channel does not exist yet, this stays
/// empty and remote update checks are off: `updater()` refuses, so the
/// interface never offers a release from anywhere and no update banner
/// ever appears. When our own channel is live, put its `owner/name`
/// here (and flip the default of `Settings::check_for_updates` back on);
/// nothing else needs to change. Never point this at the upstream
/// repository: this build must not offer upstream releases as its own
/// updates.
pub const UPDATE_REPO: &str = "";

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
    // The next release key, backed up outside GitHub. Releases stay signed
    // with the current key until installs trust this one too.
    additional_publisher_keys: &[include_str!("../assets/update-public-key-next.hex")],
    // The repository comes from UPDATE_REPO once configured. Until then
    // this placeholder keeps CONFIG.validate() honest about the shape;
    // it is never queried, because updater() refuses while UPDATE_REPO
    // is empty. The slug stays "zapfast" so existing installs' marker
    // files and staging folders keep matching.
    ..if UPDATE_REPO.is_empty() {
        UpdateConfig::new("whatsay/whatsay", "Whatsay", "zapfast", env!("CARGO_PKG_VERSION"))
    } else {
        UpdateConfig::new(UPDATE_REPO, "Whatsay", "zapfast", env!("CARGO_PKG_VERSION"))
    }
};

/// An updater on the proxy-aware reqwest client.
///
/// Refuses while [`UPDATE_REPO`] is empty: this build has no update
/// channel of its own yet, and it must never check the upstream
/// repository for releases to offer as its own.
pub fn updater() -> anyhow::Result<Updater> {
    anyhow::ensure!(
        !UPDATE_REPO.is_empty(),
        "remote updates are not configured in this build"
    );
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
    fn remote_updates_are_off_until_our_channel_exists() {
        // UPDATE_REPO is empty until our own releases channel is live.
        // When it is filled in, replace this with an assertion that the
        // updater starts on our own repository.
        assert!(UPDATE_REPO.is_empty());
        assert!(updater().is_err());
    }
}
