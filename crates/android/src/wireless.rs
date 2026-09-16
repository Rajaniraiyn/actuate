//! Direct Android wireless pairing and TLS transport. No adb executable or server.
use crate::connection::Runtime;
use actuate::{Effect, NativeError, Result};
use async_trait::async_trait;
use droidmux::pairing::{
    self, CredentialStore, CredentialStoreError, StoredHostCredential, StoredPairedDevice,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};

fn fail(code: &str, message: impl ToString, effect: Effect) -> NativeError {
    NativeError {
        code: code.into(),
        message: message.to_string(),
        effect,
    }
}
fn store_error(e: impl ToString) -> CredentialStoreError {
    CredentialStoreError::new(e.to_string())
}
#[derive(Serialize, Deserialize)]
struct HostRecord {
    pem: String,
    name: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedPairing {
    pub device_id: String,
    pub host: String,
    pub certificate_fingerprint: String,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum ConnectionPin {
    PublicKeySha256 { fingerprint: String },
}
/// One directory represents one explicit paired device and host identity.
/// Unix permissions are enforced; other hosts need a protected store adapter.
struct FileStore {
    path: PathBuf,
}
impl FileStore {
    fn open(#[cfg_attr(not(unix), allow(unused_variables))] path: &Path) -> Result<Self> {
        #[cfg(not(unix))]
        return Err(fail(
            "credential_store_unsupported",
            "A protected credential store is required on this host",
            Effect::None,
        ));
        #[cfg(unix)]
        {
            use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
            if !path.exists() {
                fs::DirBuilder::new()
                    .recursive(true)
                    .mode(0o700)
                    .create(path)
                    .map_err(|e| fail("credential_store", e, Effect::None))?;
            }
            let metadata = fs::symlink_metadata(path)
                .map_err(|e| fail("credential_store", e, Effect::None))?;
            if !metadata.is_dir() || metadata.permissions().mode() & 0o077 != 0 {
                return Err(fail(
                    "credential_store_permissions",
                    "Credential directory must be a private directory with mode 0700",
                    Effect::None,
                ));
            }
            Ok(Self {
                path: path.to_owned(),
            })
        }
    }
    fn read<T: serde::de::DeserializeOwned>(
        &self,
        name: &str,
    ) -> std::result::Result<Option<T>, CredentialStoreError> {
        let path = self.path.join(name);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(store_error(e)),
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o077 != 0 {
                return Err(store_error("Credential file must have mode 0600"));
            }
        }
        if !metadata.is_file() || metadata.len() > 64 * 1024 {
            return Err(store_error("Invalid credential file"));
        }
        let bytes = fs::read(path).map_err(store_error)?;
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| store_error("Invalid credential record"))
    }
    fn create<T: Serialize>(
        &self,
        name: &str,
        value: &T,
    ) -> std::result::Result<(), CredentialStoreError> {
        let bytes = serde_json::to_vec(value)
            .map_err(|_| store_error("Could not encode credential record"))?;
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut file = opts.open(self.path.join(name)).map_err(store_error)?;
        file.write_all(&bytes).map_err(store_error)?;
        file.sync_all().map_err(store_error)
    }
}
#[async_trait]
impl CredentialStore for FileStore {
    async fn load_host_credential(
        &self,
    ) -> std::result::Result<Option<StoredHostCredential>, CredentialStoreError> {
        Ok(self
            .read::<HostRecord>("host.json")?
            .map(|r| StoredHostCredential::new(pairing::SecretString::from(r.pem), r.name)))
    }
    async fn save_host_credential(
        &self,
        c: &StoredHostCredential,
    ) -> std::result::Result<(), CredentialStoreError> {
        self.create(
            "host.json",
            &HostRecord {
                pem: c.expose_private_key_pem().into(),
                name: c.client_name().into(),
            },
        )
    }
    async fn save_paired_device(
        &self,
        d: &StoredPairedDevice,
    ) -> std::result::Result<(), CredentialStoreError> {
        self.create(
            "device.json",
            &SavedPairing {
                device_id: d.device_id.clone(),
                host: d.host.clone(),
                certificate_fingerprint: d.certificate_fingerprint.clone(),
            },
        )
    }
    async fn load_paired_device(
        &self,
        id: &str,
    ) -> std::result::Result<Option<StoredPairedDevice>, CredentialStoreError> {
        Ok(self
            .read::<SavedPairing>("device.json")?
            .filter(|d| d.device_id == id)
            .map(|d| StoredPairedDevice {
                device_id: d.device_id,
                host: d.host,
                certificate_fingerprint: d.certificate_fingerprint,
            }))
    }
}
#[derive(Debug, Clone, Copy)]
pub enum FirstConnectionPolicy {
    RequireKnownCertificate,
    TrustOnFirstUse,
}

pub struct WirelessHost {
    store: FileStore,
    timeout: Duration,
}
impl WirelessHost {
    pub fn paired_device_id(&self) -> Result<String> {
        self.store
            .read::<SavedPairing>("device.json")
            .map_err(|e| fail("credential_store", e, Effect::None))?
            .map(|d| d.device_id)
            .ok_or_else(|| {
                fail(
                    "device_not_paired",
                    "Pair first using this credential directory",
                    Effect::None,
                )
            })
    }

    pub fn new(path: impl AsRef<Path>, timeout: Duration) -> Result<Self> {
        if timeout.is_zero() {
            return Err(fail(
                "invalid_timeout",
                "Timeout must be positive",
                Effect::None,
            ));
        }
        Ok(Self {
            store: FileStore::open(path.as_ref())?,
            timeout,
        })
    }
    pub fn pair(&self, endpoint: SocketAddr, code: &str) -> Result<SavedPairing> {
        if self
            .store
            .read::<SavedPairing>("device.json")
            .map_err(|e| fail("credential_store", e, Effect::None))?
            .is_some()
        {
            return Err(fail(
                "device_already_paired",
                "Use the existing device or a separate credential directory",
                Effect::None,
            ));
        }
        Runtime::new()?.run(self.timeout, Effect::Unknown, async {
            let result = pairing::pair_device(
                pairing::PairingRequest {
                    host: endpoint.ip().to_string(),
                    port: endpoint.port(),
                    pairing_code: pairing::SecretString::from(code.to_owned()),
                    client_name: "actuate".into(),
                },
                &self.store,
            )
            .await
            .map_err(|e| fail("android_pairing", e, Effect::Unknown))?;
            Ok(SavedPairing {
                device_id: result.device_id,
                host: endpoint.ip().to_string(),
                certificate_fingerprint: result.certificate_fingerprint,
            })
        })
    }
    pub fn connect(
        &self,
        endpoint: SocketAddr,
        policy: FirstConnectionPolicy,
    ) -> Result<WirelessDevice> {
        let device_id = self.paired_device_id()?;
        let expected: Option<ConnectionPin> = self
            .store
            .read("connection.json")
            .map_err(|e| fail("credential_store", e, Effect::None))?;
        let expected = expected.map(|pin| match pin {
            ConnectionPin::PublicKeySha256 { fingerprint } => fingerprint,
        });
        if expected.is_none() && matches!(policy, FirstConnectionPolicy::RequireKnownCertificate) {
            return Err(fail(
                "first_connection_trust_required",
                "First connection certificate is not authenticated by pairing; explicitly trust this endpoint once",
                Effect::None,
            ));
        }
        let observed = std::sync::Arc::new(std::sync::Mutex::new(None));
        let config = pairing::WirelessTransportConfig {
            expected_fingerprint: expected.clone(),
            first_connection_fingerprint: if expected.is_none() {
                Some(observed.clone())
            } else {
                None
            },
            ..Default::default()
        };
        let runtime = Runtime::new()?;
        let client = runtime.run(self.timeout, Effect::None, async {
            pairing::connect_paired_device_with_config(
                &device_id,
                endpoint.ip().to_string(),
                endpoint.port(),
                &self.store,
                config,
            )
            .await
            .map_err(|e| {
                fail(
                    if e.to_string().contains("connection public key changed") {
                        "connection_public_key_changed"
                    } else {
                        "android_connect"
                    },
                    e,
                    Effect::None,
                )
            })
        })?;
        if expected.is_none() {
            let fingerprint = observed
                .lock()
                .map_err(|_| {
                    fail(
                        "certificate_state",
                        "Certificate state poisoned",
                        Effect::None,
                    )
                })?
                .clone()
                .ok_or_else(|| {
                    fail(
                        "certificate_missing",
                        "No TLS certificate was recorded",
                        Effect::None,
                    )
                })?;
            self.store
                .create(
                    "connection.json",
                    &ConnectionPin::PublicKeySha256 { fingerprint },
                )
                .map_err(|e| fail("credential_store", e, Effect::Unknown))?;
        }
        Ok(WirelessDevice {
            client,
            runtime,
            timeout: self.timeout,
        })
    }
}

pub use crate::connection::Connection as WirelessDevice;

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    fn store() -> FileStore {
        let path = std::env::temp_dir().join(format!(
            "actuate-wireless-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        FileStore::open(&path).unwrap()
    }
    #[test]
    fn connection_pin_schema_cannot_reinterpret_legacy_certificate_hash() {
        assert!(serde_json::from_str::<ConnectionPin>(r#""old-cert-fingerprint""#).is_err());
        let value = ConnectionPin::PublicKeySha256 {
            fingerprint: "a".repeat(64),
        };
        let encoded = serde_json::to_value(value).unwrap();
        assert_eq!(encoded["type"], "public_key_sha256");
        assert!(serde_json::from_value::<ConnectionPin>(encoded).is_ok());
    }
    #[test]
    fn saved_identity_cannot_be_replaced() {
        let store = store();
        store
            .create(
                "host.json",
                &HostRecord {
                    pem: "private-test-marker".into(),
                    name: "test".into(),
                },
            )
            .unwrap();
        let error = store
            .create(
                "host.json",
                &HostRecord {
                    pem: "replacement".into(),
                    name: "test".into(),
                },
            )
            .unwrap_err();
        assert!(!error.to_string().contains("private-test-marker"));
        assert_eq!(
            store.read::<HostRecord>("host.json").unwrap().unwrap().pem,
            "private-test-marker"
        );
        fs::remove_dir_all(store.path).unwrap();
    }
    #[test]
    fn unsafe_permission_and_symlink_files_rejected() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let store = store();
        store.create("host.json", &"secret").unwrap();
        fs::set_permissions(
            store.path.join("host.json"),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        assert!(store.read::<String>("host.json").is_err());
        symlink(store.path.join("host.json"), store.path.join("device.json")).unwrap();
        assert!(store.read::<String>("device.json").is_err());
        fs::remove_dir_all(store.path).unwrap();
    }
    #[test]
    fn unknown_certificate_rejected_before_network() {
        let store = store();
        store
            .create(
                "device.json",
                &SavedPairing {
                    device_id: "test".into(),
                    host: "127.0.0.1".into(),
                    certificate_fingerprint: "pairing-only".into(),
                },
            )
            .unwrap();
        let path = store.path.clone();
        let host = WirelessHost {
            store,
            timeout: Duration::from_millis(1),
        };
        let error = host
            .connect(
                "127.0.0.1:1".parse().unwrap(),
                FirstConnectionPolicy::RequireKnownCertificate,
            )
            .err()
            .unwrap();
        assert_eq!(error.code, "first_connection_trust_required");
        fs::remove_dir_all(path).unwrap();
    }
}
