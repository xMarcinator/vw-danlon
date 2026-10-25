//! Where the app keeps secrets (the calendar link and the Connected Cars
//! device token): the platform's secret store, or, only when the user opts
//! in, a file readable only by them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// A secret the app keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Secret {
    /// The calendar share link (it holds the key to the calendar).
    CalendarLink,
    /// The Connected Cars device token for an account email.
    DeviceToken(String),
}

impl Secret {
    /// Keychain service and user. The device token uses the CLI's entry, so
    /// an account connected with `account connect` is seen by the app.
    #[cfg_attr(target_os = "android", allow(dead_code))]
    fn keychain(&self) -> (&'static str, &str) {
        match self {
            Secret::CalendarLink => ("vw-danlon-calendar", "calendar-link"),
            Secret::DeviceToken(email) => ("vw-danlon", email),
        }
    }

    fn file_key(&self) -> String {
        match self {
            Secret::CalendarLink => "calendar-link".into(),
            Secret::DeviceToken(email) => format!("connected-cars-token:{email}"),
        }
    }
}

pub trait SecretStore {
    /// Where the secrets are kept, for the user.
    fn describe(&self) -> String;
    fn get(&self, secret: &Secret) -> Result<Option<String>>;
    fn set(&self, secret: &Secret, value: &str) -> Result<()>;
}

/// The store for this platform, or `file` when the user opted in to it.
pub fn store(file: Option<PathBuf>) -> Box<dyn SecretStore> {
    match file {
        Some(path) => Box::new(FileStore(path)),
        None => platform_store(),
    }
}

#[cfg(not(target_os = "android"))]
fn platform_store() -> Box<dyn SecretStore> {
    Box::new(desktop::Keychain)
}

#[cfg(target_os = "android")]
fn platform_store() -> Box<dyn SecretStore> {
    Box::new(android::Keystore)
}

#[cfg(not(target_os = "android"))]
mod desktop {
    use super::*;

    /// The operating system's keychain (Secret Service, macOS Keychain,
    /// Windows Credential Manager).
    pub struct Keychain;

    fn entry(secret: &Secret) -> Result<keyring::Entry> {
        let (service, user) = secret.keychain();
        keyring::Entry::new(service, user).map_err(unavailable)
    }

    fn unavailable(e: keyring::Error) -> anyhow::Error {
        anyhow::anyhow!("the system keychain is unavailable: {e}")
    }

    impl SecretStore for Keychain {
        fn describe(&self) -> String {
            "the system keychain".into()
        }

        fn get(&self, secret: &Secret) -> Result<Option<String>> {
            match entry(secret)?.get_password() {
                Ok(v) => Ok(Some(v)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => Err(unavailable(e)),
            }
        }

        fn set(&self, secret: &Secret, value: &str) -> Result<()> {
            entry(secret)?.set_password(value).map_err(unavailable)
        }
    }
}

#[cfg(target_os = "android")]
mod android {
    use std::sync::OnceLock;

    use super::*;

    /// Android SharedPreferences encrypted with a key in the Android
    /// Keystore (android-native-keyring-store).
    pub struct Keystore;

    fn init() -> Result<()> {
        static INIT: OnceLock<Result<(), String>> = OnceLock::new();
        INIT.get_or_init(|| {
            let store = android_native_keyring_store::Store::new().map_err(|e| e.to_string())?;
            keyring_core::set_default_store(store);
            Ok(())
        })
        .clone()
        .map_err(|e| anyhow::anyhow!("the Android Keystore is unavailable: {e}"))
    }

    fn entry(secret: &Secret) -> Result<keyring_core::Entry> {
        init()?;
        keyring_core::Entry::new("vw-danlon", &secret.file_key()).map_err(unavailable)
    }

    fn unavailable(e: keyring_core::Error) -> anyhow::Error {
        anyhow::anyhow!("the Android Keystore is unavailable: {e}")
    }

    impl SecretStore for Keystore {
        fn describe(&self) -> String {
            "the Android Keystore".into()
        }

        fn get(&self, secret: &Secret) -> Result<Option<String>> {
            match entry(secret)?.get_password() {
                Ok(v) => Ok(Some(v)),
                Err(keyring_core::Error::NoEntry) => Ok(None),
                Err(e) => Err(unavailable(e)),
            }
        }

        fn set(&self, secret: &Secret, value: &str) -> Result<()> {
            entry(secret)?.set_password(value).map_err(unavailable)
        }
    }
}

/// Secrets in a JSON file readable only by the user (mode 0600 on Unix),
/// used only when the user opts in.
pub struct FileStore(pub PathBuf);

impl FileStore {
    fn read(&self) -> Result<BTreeMap<String, String>> {
        let text = match std::fs::read_to_string(&self.0) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
            Err(e) => return Err(e).with_context(|| format!("reading {}", self.0.display())),
        };
        serde_json::from_str(&text).with_context(|| format!("parsing {}", self.0.display()))
    }

    fn write(&self, map: &BTreeMap<String, String>) -> Result<()> {
        write_private(&self.0, &serde_json::to_vec_pretty(map)?)
            .with_context(|| format!("writing {}", self.0.display()))
    }
}

impl SecretStore for FileStore {
    fn describe(&self) -> String {
        format!("{} (readable only by you)", self.0.display())
    }

    fn get(&self, secret: &Secret) -> Result<Option<String>> {
        Ok(self.read()?.remove(&secret.file_key()))
    }

    fn set(&self, secret: &Secret, value: &str) -> Result<()> {
        let mut map = self.read()?;
        map.insert(secret.file_key(), value.into());
        self.write(&map)
    }
}

/// Writes `bytes` readable only by the user, through a temp file and a rename.
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    let tmp = path.with_file_name(name);
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    let mut f = opts.open(&tmp)?;
    std::io::Write::write_all(&mut f, bytes)?;
    f.sync_all()?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_store_round_trip_is_private() {
        let dir = crate::setup::tests::scratch("secrets");
        let store = FileStore(dir.join("secrets.json"));
        let link = Secret::CalendarLink;
        let token = Secret::DeviceToken("a@example.com".into());
        assert_eq!(store.get(&link).unwrap(), None);
        store.set(&link, "https://cal/x").unwrap();
        store.set(&token, "dev-1").unwrap();
        assert_eq!(store.get(&link).unwrap().as_deref(), Some("https://cal/x"));
        assert_eq!(store.get(&token).unwrap().as_deref(), Some("dev-1"));
        assert_eq!(
            store
                .get(&Secret::DeviceToken("b@example.com".into()))
                .unwrap(),
            None
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&store.0).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }
}
