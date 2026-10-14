//! Where Whatsay keeps its files.
//!
//! Configuration, session state, and caches use separate standard platform
//! directories. Clearing a cache does not remove device keys.

use std::path::{Path, PathBuf};

use directories::ProjectDirs;

use crate::model::AccountId;

#[derive(Clone, Debug)]
pub struct AppDirs {
    pub config: PathBuf,
    pub state: PathBuf,
    pub cache: PathBuf,
    /// Per-user directory for the single-instance lock and control socket.
    pub runtime: PathBuf,
}

impl AppDirs {
    pub fn discover() -> Self {
        // An explicit data root wins over the platform directories, so the
        // installer can offer a data location and portable setups can point
        // elsewhere without touching the registry.
        if let Ok(dir) = std::env::var("WHATSAY_DATA_DIR") {
            let dir = dir.trim();
            if !dir.is_empty() {
                return Self::from_data_root(Path::new(dir));
            }
        }
        if let Some((DataRoot::Custom(root), _)) = data_root_override() {
            return Self::from_data_root(&root);
        }
        match Self::of("zapfast") {
            Some(dirs) => dirs,
            None => {
                let fallback = std::env::current_dir().unwrap_or_default();
                Self {
                    config: fallback.join("zapfast-config"),
                    state: fallback.join("zapfast-state"),
                    cache: fallback.join("zapfast-cache"),
                    runtime: fallback.join("zapfast-run"),
                }
            }
        }
    }

    /// All app data under one user-chosen directory: config, chat history
    /// and session state (`data`), downloaded files and caches (`cache`),
    /// and the single-instance lock (`run`).
    fn from_data_root(root: &Path) -> Self {
        Self {
            config: root.join("config"),
            state: root.join("data"),
            cache: root.join("cache"),
            runtime: root.join("run"),
        }
    }

    /// Whether this location already holds app data worth keeping.
    fn has_data(&self) -> bool {
        self.state.join("archive.db").exists()
            || self.state.join("session.db").exists()
            || self.config.join("settings.json").exists()
    }

    /// Moves all data to the data root chosen in `datadir.txt`, when the
    /// data is not already there. The archive's keyring key is copied before
    /// anything moves, so an encrypted archive stays readable under the new
    /// folder's keyring identity.
    ///
    /// Call only after acquiring the instance guard, and before `ensure()`.
    pub fn adopt_data_root(&self) -> std::io::Result<()> {
        let Some((desired, prev)) = data_root_override() else {
            return Ok(());
        };
        let Some((from, to)) = Self::data_root_migration(self, &desired, &prev) else {
            return Ok(());
        };
        from.migrate_to_with(&to, |from, to| {
            crate::archive::copy_archive_key(from, to)
                .map_err(|error| std::io::Error::other(format!("{error:#}")))
        })
    }

    /// Decides whether data needs moving: the desired root lacks data while
    /// a previous location still has it. Returns the move's source and
    /// destination.
    fn data_root_migration(
        current: &AppDirs,
        desired: &DataRoot,
        prev: &DataRoot,
    ) -> Option<(AppDirs, AppDirs)> {
        let target = match desired {
            DataRoot::Custom(root) => Self::from_data_root(root),
            DataRoot::Default => Self::of("zapfast")?,
        };
        // discover() already pointed us here; anything else means an env
        // override is in play, which never migrates.
        if current.state != target.state || target.has_data() {
            return None;
        }
        let from = match prev {
            DataRoot::Custom(root) => Self::from_data_root(root),
            DataRoot::Default => Self::of("zapfast")?,
        };
        if !from.has_data() || from.state == target.state {
            return None;
        }
        Some((from, target))
    }

    fn migrate_to_with(
        &self,
        target: &AppDirs,
        copy_key: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        let from_archive = self.state.join("archive.db");
        if from_archive.exists() {
            copy_key(&from_archive, &target.state.join("archive.db"))?;
        }
        for (from, to) in [
            (&self.config, &target.config),
            (&self.state, &target.state),
            (&self.cache, &target.cache),
            (&self.runtime, &target.runtime),
        ] {
            adopt_directory(from, to)?;
        }
        Ok(())
    }
}

/// Where the app should keep its data: the platform directories, or one
/// user-chosen root.
#[derive(Debug, PartialEq)]
enum DataRoot {
    Default,
    Custom(PathBuf),
}

/// Reads the data-location choice: `WHATSAY_DATA_DIR` wins, otherwise
/// `datadir.txt` next to the executable (written by the installer).
/// Returns the desired root and the previously chosen one, if any.
fn data_root_override() -> Option<(DataRoot, DataRoot)> {
    let exe_dir = std::env::current_exe()
        .ok()?
        .parent()
        .map(Path::to_path_buf)?;
    data_root_override_in(&exe_dir)
}

fn data_root_override_in(dir: &Path) -> Option<(DataRoot, DataRoot)> {
    let text = std::fs::read_to_string(dir.join("datadir.txt")).ok()?;
    let mut lines = text.lines().map(str::trim);
    let desired = parse_data_root(lines.next()?)?;
    let prev = lines
        .next()
        .map(parse_data_root)
        .unwrap_or(Some(DataRoot::Default));
    Some((desired, prev?))
}

/// An absolute path, or the word `default`. Anything else is ignored so a
/// hand-edited file cannot send the app somewhere surprising.
fn parse_data_root(text: &str) -> Option<DataRoot> {
    if text.is_empty() || text.eq_ignore_ascii_case("default") {
        return Some(DataRoot::Default);
    }
    let path = PathBuf::from(text);
    if path.is_absolute() {
        Some(DataRoot::Custom(path))
    } else {
        None
    }
}

impl AppDirs {
    /// Standard platform directories for the app.
    fn of(name: &str) -> Option<Self> {
        let project = ProjectDirs::from("me", "paolino", name)?;
        let state = project
            .state_dir()
            .map(|path| path.to_path_buf())
            .unwrap_or_else(|| project.data_local_dir().to_path_buf());
        Some(Self {
            config: project.config_dir().to_path_buf(),
            runtime: runtime_dir(&project, &state),
            state,
            cache: project.cache_dir().to_path_buf(),
        })
    }

    /// Adopts earlier names, newest first, without replacing existing data.
    /// Call only after acquiring the instance guard, and never for demo runs.
    pub fn adopt_previous_names(&self) -> std::io::Result<()> {
        for name in ["fastsapp", "fastwhatsapp"] {
            if let Some(old) = Self::of(name) {
                self.adopt(&old)?;
            }
            if let (Some(from), Some(to)) =
                (eframe::storage_dir(name), eframe::storage_dir("zapfast"))
            {
                adopt_directory(&from, &to)?;
            }
        }
        Ok(())
    }

    fn adopt(&self, old: &Self) -> std::io::Result<()> {
        for (from, to) in [
            (&old.config, &self.config),
            (&old.state, &self.state),
            (&old.cache, &self.cache),
        ] {
            adopt_directory(from, to)?;
        }
        Ok(())
    }

    /// Places all data under one directory for tests and temporary runs.
    pub fn under(root: &std::path::Path) -> Self {
        Self {
            config: root.join("config"),
            state: root.join("state"),
            cache: root.join("cache"),
            runtime: root.join("run"),
        }
    }

    pub fn settings_file(&self) -> PathBuf {
        self.config.join("settings.json")
    }

    /// Ordered list of account ids and the active one.
    pub fn accounts_file(&self) -> PathBuf {
        self.config.join("accounts.json")
    }

    pub fn account(&self, id: &AccountId) -> AccountDirs {
        assert!(
            AccountId::is_safe(id.as_str()),
            "account id is not a safe folder name"
        );
        AccountDirs {
            id: id.clone(),
            state: self.state.join("accounts").join(id.as_str()),
            cache: self.cache.join("accounts").join(id.as_str()),
        }
    }

    /// Session files at the app root, used by tests that have not moved into
    /// `accounts/<id>/`.
    pub fn as_account(&self) -> AccountDirs {
        AccountDirs {
            id: AccountId::first(),
            state: self.state.clone(),
            cache: self.cache.clone(),
        }
    }

    /// Moves a single-account layout (databases at the root of the state
    /// directory) into `accounts/1/`.
    ///
    /// The archive is the only copy of the history, so nothing moves unless
    /// all of it can: a file already waiting at the destination stops the
    /// move before anything changes, and an encrypted archive moves only once
    /// its keyring key has been copied to the new folder's identity and read
    /// back. SQLite's side files move before their database, so an
    /// interrupted move is finished by the next start rather than leaving a
    /// write-ahead log behind.
    pub fn adopt_single_account(&self) -> std::io::Result<()> {
        self.adopt_single_account_with(|from, to| {
            crate::archive::copy_archive_key(from, to)
                .map_err(|error| std::io::Error::other(format!("{error:#}")))
        })
    }

    fn adopt_single_account_with(
        &self,
        copy_key: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        // Side files first: a database is only moved after its log.
        const DATABASES: [&str; 8] = [
            "session.db-wal",
            "session.db-shm",
            "session.db-journal",
            "session.db",
            "archive.db-wal",
            "archive.db-shm",
            "archive.db-journal",
            "archive.db",
        ];
        let pending: Vec<&str> = DATABASES
            .into_iter()
            .filter(|name| self.state.join(name).exists())
            .collect();
        let dest = self.account(&AccountId::first());
        if !pending.is_empty() {
            self.adopt_databases(&dest, &pending, copy_key)?;
        }
        // Caches and pictures carry no key; an interrupted move finishes here.
        adopt_directory(&self.state.join("stickers"), &dest.saved_sticker_dir())?;
        adopt_directory(&self.cache.join("media"), &dest.media_cache_dir())?;
        adopt_directory(&self.cache.join("avatars"), &dest.avatar_cache_dir())?;
        adopt_directory(&self.cache.join("stickers"), &dest.sticker_cache_dir())?;
        for extension in ["jpg", "png", "webp", "gif"] {
            move_file(
                &self.state.join(format!("wallpaper.{extension}")),
                &dest.wallpaper_file(extension),
            )?;
        }
        Ok(())
    }

    fn adopt_databases(
        &self,
        dest: &AccountDirs,
        pending: &[&str],
        copy_key: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        dest.ensure()?;
        // Refuse to mix two setups. Only an unrelated earlier file can be in
        // the way: a rename leaves nothing behind at its source.
        let mut blocked: Vec<PathBuf> = pending
            .iter()
            .map(|name| dest.state.join(name))
            .filter(|to| to.exists())
            .collect();
        if self.state.join("stickers").is_dir() && dest.saved_sticker_dir().exists() {
            blocked.push(dest.saved_sticker_dir());
        }
        if !blocked.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!(
                    "{} holds a single-account setup, but {} already exists. Nothing was moved; move one of them away and start Whatsay again",
                    self.state.display(),
                    blocked[0].display()
                ),
            ));
        }
        let legacy_archive = self.state.join("archive.db");
        if legacy_archive.exists() {
            copy_key(&legacy_archive, &dest.archive_db()).map_err(|error| {
                std::io::Error::other(format!(
                    "Could not move the archive's key to the new account folder, so the archive was left where it is: {error}"
                ))
            })?;
        }
        for name in pending {
            std::fs::rename(self.state.join(name), dest.state.join(name))?;
        }
        // Make the renames durable before anything opens the databases.
        #[cfg(unix)]
        for dir in [&self.state, &dest.state] {
            std::fs::File::open(dir)?.sync_all()?;
        }
        Ok(())
    }

    /// whatsapp-rust device identity, Signal sessions, and state keys.
    /// Deleting this database unlinks the computer.
    pub fn session_db(&self) -> PathBuf {
        self.state.join("session.db")
    }

    /// Local message archive.
    pub fn archive_db(&self) -> PathBuf {
        self.state.join("archive.db")
    }

    /// Current-run log, replaced at startup.
    pub fn log_file(&self) -> PathBuf {
        self.state.join("zapfast.log")
    }

    /// Panic log written before process exit.
    pub fn panic_log(&self) -> PathBuf {
        self.state.join("panic.log")
    }

    /// Downloaded attachments keyed by message id.
    pub fn media_cache_dir(&self) -> PathBuf {
        self.cache.join("media")
    }

    /// Profile pictures keyed by chat.
    pub fn avatar_cache_dir(&self) -> PathBuf {
        self.cache.join("avatars")
    }

    /// Recent phone stickers keyed by file hash.
    pub fn sticker_cache_dir(&self) -> PathBuf {
        self.cache.join("stickers")
    }

    /// Saved stickers keyed by content hash. These are user data, not cache.
    pub fn saved_sticker_dir(&self) -> PathBuf {
        self.state.join("stickers")
    }

    /// Whatsay's copy of the chosen chat wallpaper image. User data, so it
    /// sits beside saved stickers rather than in the cache.
    pub fn wallpaper_file(&self, extension: &str) -> PathBuf {
        self.state.join(format!("wallpaper.{extension}"))
    }

    /// Cached profile-picture path. `full` selects the info-dialog size.
    pub fn avatar_file(&self, id: &str, full: bool) -> PathBuf {
        let stem: String = id
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        self.avatar_cache_dir()
            .join(format!("{stem}{}.jpg", if full { "-full" } else { "" }))
    }

    pub fn ensure(&self) -> std::io::Result<()> {
        for dir in [&self.config, &self.state, &self.cache] {
            let mut builder = std::fs::DirBuilder::new();
            builder.recursive(true);
            // Create new directories privately, even with a permissive umask.
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder.create(dir)?;
            restrict_directory(dir)?;
        }
        Ok(())
    }
}

/// Per-account session, archive, stickers, and caches.
#[derive(Clone, Debug)]
pub struct AccountDirs {
    pub id: AccountId,
    pub state: PathBuf,
    pub cache: PathBuf,
}

impl AccountDirs {
    pub fn ensure(&self) -> std::io::Result<()> {
        for dir in [&self.state, &self.cache] {
            let mut builder = std::fs::DirBuilder::new();
            builder.recursive(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder.create(dir)?;
            restrict_directory(dir)?;
        }
        Ok(())
    }

    /// whatsapp-rust device identity. Deleting this database unlinks the account.
    pub fn session_db(&self) -> PathBuf {
        self.state.join("session.db")
    }

    pub fn archive_db(&self) -> PathBuf {
        self.state.join("archive.db")
    }

    pub fn settings_file(&self) -> PathBuf {
        self.state.join("settings.json")
    }

    pub fn media_cache_dir(&self) -> PathBuf {
        self.cache.join("media")
    }

    pub fn avatar_cache_dir(&self) -> PathBuf {
        self.cache.join("avatars")
    }

    pub fn sticker_cache_dir(&self) -> PathBuf {
        self.cache.join("stickers")
    }

    pub fn saved_sticker_dir(&self) -> PathBuf {
        self.state.join("stickers")
    }

    /// Whatsay's copy of the chosen chat wallpaper image for this account.
    pub fn wallpaper_file(&self, extension: &str) -> PathBuf {
        self.state.join(format!("wallpaper.{extension}"))
    }

    pub fn avatar_file(&self, id: &str, full: bool) -> PathBuf {
        let stem: String = id
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        self.avatar_cache_dir()
            .join(format!("{stem}{}.jpg", if full { "-full" } else { "" }))
    }
}

fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.exists() && !to.try_exists()? {
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::rename(from, to)?;
    }
    Ok(())
}

#[cfg(unix)]
fn restrict_directory(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn restrict_directory(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Private directory for the instance lock and control channel. Uses
/// `$XDG_RUNTIME_DIR` where there is one. Elsewhere the directory sits
/// beside the state directory rather than inside it, because the state
/// directory must not exist before an earlier name's data is adopted.
fn runtime_dir(project: &ProjectDirs, state: &Path) -> PathBuf {
    // Flatpak gives each sandbox a private runtime directory and shares only
    // this one between instances of the app.
    #[cfg(target_os = "linux")]
    if let (Some(runtime), Some(id)) = (
        project.runtime_dir().and_then(Path::parent),
        std::env::var_os("FLATPAK_ID"),
    ) {
        return runtime.join("app").join(id);
    }
    if let Some(runtime) = project.runtime_dir() {
        return runtime.to_path_buf();
    }
    let mut name = state.file_name().unwrap_or_default().to_os_string();
    name.push(".run");
    state.with_file_name(name)
}

/// Rename whole directories so SQLite databases travel with their WAL files.
/// A failed move stops startup before empty replacement directories are made.
fn adopt_directory(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_dir() && !to.try_exists()? {
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::rename(from, to)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("zapfast-paths-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[cfg(unix)]
    #[test]
    fn ensure_restricts_base_directories() {
        use std::os::unix::fs::PermissionsExt;

        let root = root("permissions");
        let dirs = AppDirs::under(&root);
        dirs.ensure().unwrap();
        for path in [&dirs.config, &dirs.state, &dirs.cache] {
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn ensure_repairs_existing_directory_permissions_without_changing_data() {
        use std::os::unix::fs::PermissionsExt;

        let root = root("existing-permissions");
        let dirs = AppDirs::under(&root);
        for path in [&dirs.config, &dirs.state, &dirs.cache] {
            std::fs::create_dir_all(path).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
            std::fs::write(path.join("fixture"), b"preserved").unwrap();
        }
        dirs.ensure().unwrap();
        for path in [&dirs.config, &dirs.state, &dirs.cache] {
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(std::fs::read(path.join("fixture")).unwrap(), b"preserved");
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ensure_stops_when_an_application_directory_cannot_be_created() {
        let root = root("blocked-directory");
        let dirs = AppDirs::under(&root);
        std::fs::write(&dirs.state, b"existing file").unwrap();
        assert!(dirs.ensure().is_err());
        assert!(!dirs.cache.exists());
        assert_eq!(std::fs::read(&dirs.state).unwrap(), b"existing file");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rename_preserves_session_archive_settings_and_cached_files() {
        for name in ["fastsapp", "fastwhatsapp"] {
            let root = root(name);
            let old = AppDirs::under(&root.join(name));
            let new = AppDirs::under(&root.join("zapfast"));
            old.ensure().unwrap();
            for path in [
                old.settings_file(),
                old.session_db(),
                old.state.join("session.db-wal"),
                old.archive_db(),
                old.state.join("archive.db-wal"),
                old.saved_sticker_dir().join("pack/sticker.webp"),
                old.media_cache_dir().join("photo.jpg"),
            ] {
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, b"preserved").unwrap();
            }
            new.adopt(&old).unwrap();
            new.adopt(&old).unwrap(); // A second launch is a no-op.
            for path in [
                new.settings_file(),
                new.session_db(),
                new.state.join("session.db-wal"),
                new.archive_db(),
                new.state.join("archive.db-wal"),
                new.saved_sticker_dir().join("pack/sticker.webp"),
                new.media_cache_dir().join("photo.jpg"),
            ] {
                assert_eq!(std::fs::read(path).unwrap(), b"preserved");
            }
            assert!(!old.config.exists());
            assert!(!old.state.exists());
            assert!(!old.cache.exists());
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn newest_data_wins_without_merging_archives() {
        let root = root("precedence");
        let new = AppDirs::under(&root.join("zapfast"));
        let recent = AppDirs::under(&root.join("fastsapp"));
        let oldest = AppDirs::under(&root.join("fastwhatsapp"));
        recent.ensure().unwrap();
        oldest.ensure().unwrap();
        std::fs::create_dir_all(&new.config).unwrap();
        std::fs::write(new.settings_file(), b"new settings").unwrap();
        std::fs::write(recent.settings_file(), b"old settings").unwrap();
        std::fs::write(recent.archive_db(), b"recent archive").unwrap();
        std::fs::write(oldest.archive_db(), b"oldest archive").unwrap();
        new.adopt(&recent).unwrap();
        new.adopt(&oldest).unwrap();
        assert_eq!(std::fs::read(new.settings_file()).unwrap(), b"new settings");
        assert_eq!(
            std::fs::read(recent.settings_file()).unwrap(),
            b"old settings"
        );
        assert_eq!(std::fs::read(new.archive_db()).unwrap(), b"recent archive");
        assert_eq!(
            std::fs::read(oldest.archive_db()).unwrap(),
            b"oldest archive"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn shared_config_and_state_directory_moves_once() {
        let root = root("shared");
        let old = AppDirs {
            config: root.join("old/data"),
            state: root.join("old/data"),
            cache: root.join("old/cache"),
            runtime: root.join("old/run"),
        };
        let new = AppDirs {
            config: root.join("new/data"),
            state: root.join("new/data"),
            cache: root.join("new/cache"),
            runtime: root.join("new/run"),
        };
        old.ensure().unwrap();
        std::fs::write(old.session_db(), b"session").unwrap();
        new.adopt(&old).unwrap();
        assert_eq!(std::fs::read(new.session_db()).unwrap(), b"session");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_migration_leaves_source_available_for_retry() {
        let root = root("failure");
        let old = AppDirs::under(&root.join("old"));
        let new = AppDirs::under(&root.join("blocked/new"));
        old.ensure().unwrap();
        std::fs::write(old.session_db(), b"session").unwrap();
        std::fs::write(root.join("blocked"), b"not a directory").unwrap();
        assert!(new.adopt(&old).is_err());
        assert_eq!(std::fs::read(old.session_db()).unwrap(), b"session");
        std::fs::remove_file(root.join("blocked")).unwrap();
        new.adopt(&old).unwrap();
        assert_eq!(std::fs::read(new.session_db()).unwrap(), b"session");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_legacy_session_moves_into_the_first_account_folder() {
        let root = root("single-account");
        let dirs = AppDirs::under(&root);
        dirs.ensure().unwrap();
        std::fs::write(dirs.session_db(), b"session").unwrap();
        std::fs::write(dirs.archive_db(), b"SQLite format 3\0").unwrap();
        std::fs::create_dir_all(dirs.media_cache_dir()).unwrap();
        std::fs::write(dirs.media_cache_dir().join("photo.jpg"), b"photo").unwrap();
        std::fs::write(dirs.state.join("wallpaper.png"), b"wall").unwrap();
        dirs.adopt_single_account().unwrap();
        let account = dirs.account(&AccountId::first());
        assert_eq!(std::fs::read(account.session_db()).unwrap(), b"session");
        assert_eq!(
            std::fs::read(account.archive_db()).unwrap(),
            b"SQLite format 3\0"
        );
        assert_eq!(
            std::fs::read(account.media_cache_dir().join("photo.jpg")).unwrap(),
            b"photo"
        );
        assert!(!dirs.session_db().exists());
        assert!(!dirs.archive_db().exists());
        assert_eq!(
            std::fs::read(account.wallpaper_file("png")).unwrap(),
            b"wall"
        );
        assert!(!dirs.state.join("wallpaper.png").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    /// A single-account setup with an encrypted-looking archive and its log.
    fn legacy(name: &str) -> (PathBuf, AppDirs) {
        let root = root(name);
        let dirs = AppDirs::under(&root);
        dirs.ensure().unwrap();
        std::fs::write(dirs.session_db(), b"session").unwrap();
        std::fs::write(dirs.archive_db(), b"encrypted pages").unwrap();
        std::fs::write(dirs.state.join("archive.db-wal"), b"committed").unwrap();
        (root, dirs)
    }

    #[test]
    fn an_archive_whose_key_cannot_move_stays_where_it_is() {
        let (root, dirs) = legacy("key-refused");
        let error = dirs
            .adopt_single_account_with(|_, _| Err(std::io::Error::other("keyring locked")))
            .unwrap_err();
        assert!(error.to_string().contains("keyring locked"), "{error}");
        assert_eq!(
            std::fs::read(dirs.archive_db()).unwrap(),
            b"encrypted pages"
        );
        assert_eq!(
            std::fs::read(dirs.state.join("archive.db-wal")).unwrap(),
            b"committed"
        );
        assert_eq!(std::fs::read(dirs.session_db()).unwrap(), b"session");
        let account = dirs.account(&AccountId::first());
        assert!(!account.archive_db().exists());
        assert!(!account.session_db().exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_key_is_copied_before_the_archive_moves() {
        let (root, dirs) = legacy("key-first");
        let account = dirs.account(&AccountId::first());
        let mut seen = None;
        dirs.adopt_single_account_with(|from, to| {
            // The archive is still at its old place while its key is copied.
            assert!(from.exists() && !to.exists());
            seen = Some((from.to_path_buf(), to.to_path_buf()));
            Ok(())
        })
        .unwrap();
        assert_eq!(seen, Some((dirs.archive_db(), account.archive_db())));
        assert_eq!(
            std::fs::read(account.archive_db()).unwrap(),
            b"encrypted pages"
        );
        assert_eq!(
            std::fs::read(account.state.join("archive.db-wal")).unwrap(),
            b"committed"
        );
        // A second start finds nothing left to move and asks for no key.
        dirs.adopt_single_account_with(|_, _| panic!("nothing to copy"))
            .unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_interrupted_move_finishes_with_its_log() {
        let (root, dirs) = legacy("interrupted");
        let account = dirs.account(&AccountId::first());
        account.ensure().unwrap();
        // The log went first, then the process stopped.
        std::fs::rename(
            dirs.state.join("archive.db-wal"),
            account.state.join("archive.db-wal"),
        )
        .unwrap();
        dirs.adopt_single_account_with(|_, _| Ok(())).unwrap();
        assert_eq!(
            std::fs::read(account.archive_db()).unwrap(),
            b"encrypted pages"
        );
        assert_eq!(
            std::fs::read(account.state.join("archive.db-wal")).unwrap(),
            b"committed"
        );
        assert!(!dirs.archive_db().exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_account_folder_in_the_way_stops_the_move_before_anything_changes() {
        let (root, dirs) = legacy("in-the-way");
        let account = dirs.account(&AccountId::first());
        account.ensure().unwrap();
        std::fs::write(account.archive_db(), b"another history").unwrap();
        let error = dirs
            .adopt_single_account_with(|_, _| panic!("no key is copied"))
            .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(
            std::fs::read(dirs.archive_db()).unwrap(),
            b"encrypted pages"
        );
        assert_eq!(std::fs::read(dirs.session_db()).unwrap(), b"session");
        assert_eq!(
            std::fs::read(account.archive_db()).unwrap(),
            b"another history"
        );
        assert!(!account.session_db().exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn data_root_override_parses_the_installer_file() {
        let root = root("datadir-parse");
        assert_eq!(data_root_override_in(&root), None);
        std::fs::write(root.join("datadir.txt"), "").unwrap();
        assert_eq!(data_root_override_in(&root), None);
        std::fs::write(root.join("datadir.txt"), "relative/path").unwrap();
        assert_eq!(data_root_override_in(&root), None);

        let custom = root.join("WhatsayData");
        std::fs::write(root.join("datadir.txt"), format!("{}\n", custom.display())).unwrap();
        assert_eq!(
            data_root_override_in(&root),
            Some((DataRoot::Custom(custom.clone()), DataRoot::Default))
        );

        let prev = root.join("OldData");
        std::fs::write(
            root.join("datadir.txt"),
            format!("{}\n{}\n", custom.display(), prev.display()),
        )
        .unwrap();
        assert_eq!(
            data_root_override_in(&root),
            Some((DataRoot::Custom(custom), DataRoot::Custom(prev)))
        );

        std::fs::write(root.join("datadir.txt"), "default\n").unwrap();
        assert_eq!(
            data_root_override_in(&root),
            Some((DataRoot::Default, DataRoot::Default))
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn data_root_migration_moves_data_only_once() {
        let root = root("datadir-migrate");
        let from = AppDirs::from_data_root(&root.join("from"));
        let to = AppDirs::from_data_root(&root.join("to"));
        // Nothing anywhere: no move.
        assert!(
            data_root_migration(&to, &DataRoot::Custom(to_path(&to)), &DataRoot::Default).is_none()
        );
        // Data already at the destination: no move.
        std::fs::create_dir_all(&to.state).unwrap();
        std::fs::write(to.state.join("archive.db"), b"history").unwrap();
        assert!(
            data_root_migration(&to, &DataRoot::Custom(to_path(&to)), &DataRoot::Default).is_none()
        );
        std::fs::remove_dir_all(&to.state).unwrap();
        // Data at the previous root: move.
        std::fs::create_dir_all(&from.state).unwrap();
        std::fs::write(from.state.join("archive.db"), b"history").unwrap();
        std::fs::create_dir_all(&from.config).unwrap();
        std::fs::write(from.config.join("settings.json"), b"{}").unwrap();
        let (got_from, got_to) = data_root_migration(
            &to,
            &DataRoot::Custom(to_path(&to)),
            &DataRoot::Custom(from_path(&from)),
        )
        .expect("should migrate");
        assert_eq!(got_from.state, from.state);
        assert_eq!(got_to.state, to.state);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn data_root_migrate_moves_folders_and_copies_the_key_first() {
        use std::sync::Mutex;
        let root = root("datadir-move");
        let from = AppDirs::from_data_root(&root.join("from"));
        let to = AppDirs::from_data_root(&root.join("to"));
        std::fs::create_dir_all(&from.state).unwrap();
        std::fs::write(from.state.join("archive.db"), b"encrypted pages").unwrap();
        std::fs::create_dir_all(&from.config).unwrap();
        std::fs::write(from.config.join("settings.json"), b"{}").unwrap();

        let copied: Mutex<Vec<(PathBuf, PathBuf)>> = Mutex::new(Vec::new());
        from.migrate_to_with(&to, |f, t| {
            copied
                .lock()
                .unwrap()
                .push((f.to_path_buf(), t.to_path_buf()));
            Ok(())
        })
        .unwrap();

        // The key is copied before the archive moves.
        assert_eq!(
            copied.lock().unwrap().as_slice(),
            &[(from.state.join("archive.db"), to.state.join("archive.db"))]
        );
        assert_eq!(
            std::fs::read(to.state.join("archive.db")).unwrap(),
            b"encrypted pages"
        );
        assert_eq!(
            std::fs::read(to.config.join("settings.json")).unwrap(),
            b"{}"
        );
        assert!(!from.state.exists());
        assert!(!from.config.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    fn to_path(dirs: &AppDirs) -> PathBuf {
        dirs.state.parent().unwrap().to_path_buf()
    }

    fn from_path(dirs: &AppDirs) -> PathBuf {
        to_path(dirs)
    }
}
