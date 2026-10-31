//! Bounded, versioned state for SPSP-owned external persistence.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::de::Error as DeserializeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};

use crate::PowerMode;

/// Maximum sidecar size accepted by the loader.
pub const MAX_STATE_FILE_BYTES: usize = 256 * 1024;
const MAX_PROFILES: usize = 16;
const MAX_SHIPS_PER_PROFILE: usize = 64;
const MAX_KEY_COMPONENTS: usize = 16;
const MAX_KEY_COMPONENT_BYTES: usize = 4096;
const ID_BYTES: usize = 32;
const ID_HEX_BYTES: usize = ID_BYTES * 2;
const TEMP_FILE_ATTEMPTS: usize = 16;
static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A name-independent hash of the active save profile's stable identity.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ProfileId([u8; ID_BYTES]);

impl ProfileId {
    /// Derive a stable profile key from ordered, non-empty identity components.
    ///
    /// The caller must supply fields that remain stable for the save slot. Display names and
    /// mutable ship data must not be used as profile components.
    ///
    /// # Errors
    ///
    /// Returns an error when the identity has no components, an empty component, or exceeds the
    /// component count or size limits.
    pub fn derive(components: &[&[u8]]) -> Result<Self, IdentifierError> {
        Ok(Self(derive_digest(b"profile", components)?))
    }

    /// Return the canonical lowercase hexadecimal representation used in the sidecar.
    #[must_use]
    pub fn to_hex(self) -> String {
        encode_hex(&self.0)
    }
}

impl Serialize for ProfileId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&encode_hex(&self.0))
    }
}

impl<'de> Deserialize<'de> for ProfileId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        decode_hex(&value).map(Self).map_err(D::Error::custom)
    }
}

/// A name-independent hash of a player-owned ship's verified stable identity.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ShipId([u8; ID_BYTES]);

impl ShipId {
    /// Derive a stable ship key from ordered, non-empty identity components.
    ///
    /// Do not include the ship's display name. Only runtime-verified, persistent identity fields
    /// belong in `components`.
    ///
    /// # Errors
    ///
    /// Returns an error when the identity has no components, an empty component, or exceeds the
    /// component count or size limits.
    pub fn derive(components: &[&[u8]]) -> Result<Self, IdentifierError> {
        Ok(Self(derive_digest(b"ship", components)?))
    }

    /// Return the canonical lowercase hexadecimal representation used in the sidecar.
    #[must_use]
    pub fn to_hex(self) -> String {
        encode_hex(&self.0)
    }
}

impl Serialize for ShipId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&encode_hex(&self.0))
    }
}

impl<'de> Deserialize<'de> for ShipId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        decode_hex(&value).map(Self).map_err(D::Error::custom)
    }
}

/// Failure to derive a stable profile or ship key.
#[derive(Debug, Eq, PartialEq)]
pub enum IdentifierError {
    NoComponents,
    EmptyComponent,
    TooManyComponents,
    ComponentTooLarge,
}

impl std::fmt::Display for IdentifierError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoComponents => formatter.write_str("an identity needs at least one component"),
            Self::EmptyComponent => formatter.write_str("identity components cannot be empty"),
            Self::TooManyComponents => formatter.write_str("identity has too many components"),
            Self::ComponentTooLarge => formatter.write_str("an identity component is too large"),
        }
    }
}

impl std::error::Error for IdentifierError {}

/// Errors returned while reading, validating, or atomically replacing SPSP state.
#[derive(Debug)]
pub enum StateError {
    Io(io::Error),
    Json(serde_json::Error),
    UnsupportedSchema(u32),
    FileTooLarge,
    TooManyProfiles,
    TooManyShips,
    EmptyProfile,
    DuplicateProfile,
    DuplicateShip,
    InvalidStatePath,
    TemporaryPathExhausted,
}

impl std::fmt::Display for StateError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "sidecar I/O failed: {error}"),
            Self::Json(error) => write!(formatter, "sidecar data is invalid: {error}"),
            Self::UnsupportedSchema(version) => {
                write!(formatter, "sidecar schema version {version} is unsupported")
            }
            Self::FileTooLarge => formatter.write_str("sidecar exceeds the size limit"),
            Self::TooManyProfiles => formatter.write_str("sidecar contains too many profiles"),
            Self::TooManyShips => formatter.write_str("sidecar contains too many ships"),
            Self::EmptyProfile => formatter.write_str("sidecar contains an empty profile"),
            Self::DuplicateProfile => formatter.write_str("sidecar contains a duplicate profile"),
            Self::DuplicateShip => formatter.write_str("sidecar contains a duplicate ship"),
            Self::InvalidStatePath => {
                formatter.write_str("sidecar path must refer to a regular file")
            }
            Self::TemporaryPathExhausted => {
                formatter.write_str("could not reserve a temporary sidecar path")
            }
        }
    }
}

impl std::error::Error for StateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for StateError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for StateError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

/// In-memory mode mappings scoped by profile and ship identity.
#[derive(Default, Eq, PartialEq)]
pub struct StateStore {
    profiles: BTreeMap<ProfileId, BTreeMap<ShipId, PowerMode>>,
}

impl StateStore {
    /// Return a remembered mode, if one exists for this exact profile and ship.
    #[must_use]
    pub fn get(&self, profile: ProfileId, ship: ShipId) -> Option<PowerMode> {
        self.profiles
            .get(&profile)
            .and_then(|ships| ships.get(&ship))
            .copied()
    }

    /// Record a player-selected mode. Returns whether the stored value changed.
    ///
    /// # Errors
    ///
    /// Returns an error if the sidecar would exceed its profile or per-profile ship limits.
    pub fn set(
        &mut self,
        profile: ProfileId,
        ship: ShipId,
        mode: PowerMode,
    ) -> Result<bool, StateError> {
        if !self.profiles.contains_key(&profile) && self.profiles.len() >= MAX_PROFILES {
            return Err(StateError::TooManyProfiles);
        }

        let ships = self.profiles.entry(profile).or_default();
        if let Some(previous) = ships.get(&ship) {
            if *previous == mode {
                return Ok(false);
            }
        } else if ships.len() >= MAX_SHIPS_PER_PROFILE {
            return Err(StateError::TooManyShips);
        }

        ships.insert(ship, mode);
        Ok(true)
    }

    /// Decode and validate a complete sidecar without modifying it.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed, oversized, unsupported, or duplicate state data.
    pub fn decode(bytes: &[u8]) -> Result<Self, StateError> {
        if bytes.len() > MAX_STATE_FILE_BYTES {
            return Err(StateError::FileTooLarge);
        }

        let document: StateDocument = serde_json::from_slice(bytes)?;
        if document.schema_version != 1 {
            return Err(StateError::UnsupportedSchema(document.schema_version));
        }
        if document.profiles.len() > MAX_PROFILES {
            return Err(StateError::TooManyProfiles);
        }

        let mut store = Self::default();
        for profile in document.profiles {
            if store.profiles.contains_key(&profile.id) {
                return Err(StateError::DuplicateProfile);
            }
            if profile.ships.is_empty() {
                return Err(StateError::EmptyProfile);
            }
            if profile.ships.len() > MAX_SHIPS_PER_PROFILE {
                return Err(StateError::TooManyShips);
            }
            let mut ships = BTreeMap::new();
            for record in profile.ships {
                if ships.insert(record.id, record.mode).is_some() {
                    return Err(StateError::DuplicateShip);
                }
            }
            store.profiles.insert(profile.id, ships);
        }
        Ok(store)
    }

    /// Serialize a deterministic, bounded representation of this store.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails or the encoded sidecar exceeds the size limit.
    pub fn encode(&self) -> Result<Vec<u8>, StateError> {
        let document = StateDocument {
            schema_version: 1,
            profiles: self
                .profiles
                .iter()
                .map(|(id, ships)| ProfileRecord {
                    id: *id,
                    ships: ships
                        .iter()
                        .map(|(id, mode)| ShipRecord {
                            id: *id,
                            mode: *mode,
                        })
                        .collect(),
                })
                .collect(),
        };
        let mut bytes = serde_json::to_vec_pretty(&document)?;
        bytes.push(b'\n');
        if bytes.len() > MAX_STATE_FILE_BYTES {
            return Err(StateError::FileTooLarge);
        }
        Ok(bytes)
    }

    /// Load a regular sidecar file without changing or deleting it on failure.
    ///
    /// # Errors
    ///
    /// Returns an error if the path is missing, is not a regular file, cannot be read, or contains
    /// invalid or unsupported sidecar data.
    pub fn load(path: &Path) -> Result<Self, StateError> {
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
            return Err(StateError::InvalidStatePath);
        }
        if metadata.len() > MAX_STATE_FILE_BYTES as u64 {
            return Err(StateError::FileTooLarge);
        }

        let file = File::open(path)?;
        let mut bytes = Vec::new();
        file.take((MAX_STATE_FILE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        Self::decode(&bytes)
    }

    /// Write a complete sidecar through a same-directory temporary file and atomic rename.
    ///
    /// The parent directory must already exist. Temporary files are private to the current user
    /// on Unix. A failed write or rename leaves the prior sidecar untouched.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization, temporary-file creation, writing, syncing, or atomic
    /// replacement fails.
    pub fn save_atomic(&self, path: &Path) -> Result<(), StateError> {
        let bytes = self.encode()?;
        match fs::symlink_metadata(path) {
            Ok(metadata)
                if metadata.file_type().is_symlink() || !metadata.file_type().is_file() =>
            {
                return Err(StateError::InvalidStatePath);
            }
            Ok(_) => {
                Self::load(path)?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(StateError::Io(error)),
        }

        let parent = path
            .parent()
            .filter(|directory| !directory.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut temporary_path = None;
        let mut temporary_file = None;

        for _ in 0..TEMP_FILE_ATTEMPTS {
            let counter = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
            let candidate =
                parent.join(format!(".spsp-state-{}-{counter}.tmp", std::process::id()));
            match create_private_file(&candidate) {
                Ok(file) => {
                    temporary_path = Some(candidate);
                    temporary_file = Some(file);
                    break;
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(StateError::Io(error)),
            }
        }

        let Some(temporary_path) = temporary_path else {
            return Err(StateError::TemporaryPathExhausted);
        };
        let Some(mut temporary_file) = temporary_file else {
            return Err(StateError::TemporaryPathExhausted);
        };

        let write_result = temporary_file
            .write_all(&bytes)
            .and_then(|()| temporary_file.sync_all());
        drop(temporary_file);
        if let Err(error) = write_result {
            let _ = fs::remove_file(&temporary_path);
            return Err(StateError::Io(error));
        }

        if let Err(error) = fs::rename(&temporary_path, path) {
            let _ = fs::remove_file(&temporary_path);
            return Err(StateError::Io(error));
        }
        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StateDocument {
    schema_version: u32,
    profiles: Vec<ProfileRecord>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProfileRecord {
    id: ProfileId,
    ships: Vec<ShipRecord>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ShipRecord {
    id: ShipId,
    mode: PowerMode,
}

fn derive_digest(domain: &[u8], components: &[&[u8]]) -> Result<[u8; ID_BYTES], IdentifierError> {
    if components.is_empty() {
        return Err(IdentifierError::NoComponents);
    }
    if components.len() > MAX_KEY_COMPONENTS {
        return Err(IdentifierError::TooManyComponents);
    }
    if components.iter().any(|component| component.is_empty()) {
        return Err(IdentifierError::EmptyComponent);
    }
    if components
        .iter()
        .any(|component| component.len() > MAX_KEY_COMPONENT_BYTES)
    {
        return Err(IdentifierError::ComponentTooLarge);
    }

    let mut hasher = Sha256::new();
    hasher.update(b"SPSP\0v1\0");
    hasher.update(domain);
    hasher.update(
        u64::try_from(components.len())
            .map_err(|_| IdentifierError::TooManyComponents)?
            .to_le_bytes(),
    );
    for component in components {
        hasher.update(
            u64::try_from(component.len())
                .map_err(|_| IdentifierError::ComponentTooLarge)?
                .to_le_bytes(),
        );
        hasher.update(component);
    }

    let digest = hasher.finalize();
    let mut result = [0; ID_BYTES];
    result.copy_from_slice(&digest);
    Ok(result)
}

fn encode_hex(bytes: &[u8; ID_BYTES]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(ID_HEX_BYTES);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn decode_hex(value: &str) -> Result<[u8; ID_BYTES], &'static str> {
    if value.len() != ID_HEX_BYTES {
        return Err("identifier must contain 64 lowercase hexadecimal characters");
    }

    let mut decoded = [0; ID_BYTES];
    let (pairs, remainder) = value.as_bytes().as_chunks::<2>();
    if !remainder.is_empty() {
        return Err("identifier must contain 64 lowercase hexadecimal characters");
    }
    for (index, pair) in pairs.iter().enumerate() {
        let high = decode_nibble(pair[0])
            .ok_or("identifier must contain 64 lowercase hexadecimal characters")?;
        let low = decode_nibble(pair[1])
            .ok_or("identifier must contain 64 lowercase hexadecimal characters")?;
        decoded[index] = (high << 4) | low;
    }
    Ok(decoded)
}

fn decode_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn create_private_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{IdentifierError, ProfileId, ShipId, StateError, StateStore};
    use crate::PowerMode;

    fn profile(value: &[u8]) -> ProfileId {
        ProfileId::derive(&[value]).expect("test profile identity is valid")
    }

    fn ship(value: &[u8]) -> ShipId {
        ShipId::derive(&[value]).expect("test ship identity is valid")
    }

    fn temporary_directory() -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock follows Unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("spsp-state-test-{}-{nonce}", std::process::id()))
    }

    #[test]
    fn identifiers_are_stable_domain_separated_and_name_independent() {
        let components: &[&[u8]] = &[b"resource-seed", b"resource-filename"];
        let first = ShipId::derive(components).expect("valid ship identity");
        let second = ShipId::derive(components).expect("same ship identity");
        let profile = ProfileId::derive(components).expect("valid profile identity");

        assert!(first == second);
        assert_eq!(
            first.to_hex(),
            "7f58d9b5388f0cc464ec231b6f0c1cfa4f00864dacd0165a54b56447f4df0830"
        );
        assert_ne!(first.to_hex(), profile.to_hex());
        assert_ne!(
            ShipId::derive(&[b"a", b"bc"]).unwrap().to_hex(),
            ShipId::derive(&[b"ab", b"c"]).unwrap().to_hex()
        );
        assert_eq!(first.to_hex().len(), 64);
        assert!(
            first
                .to_hex()
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        );
    }

    #[test]
    fn identity_derivation_rejects_missing_or_unbounded_components() {
        assert!(matches!(
            ProfileId::derive(&[]),
            Err(IdentifierError::NoComponents)
        ));
        assert!(matches!(
            ShipId::derive(&[b""]),
            Err(IdentifierError::EmptyComponent)
        ));
        assert!(matches!(
            ShipId::derive(&[&[b'x'; 4097]]),
            Err(IdentifierError::ComponentTooLarge)
        ));
    }

    #[test]
    fn profiles_and_ships_are_independently_scoped() {
        let profile_one = profile(b"save-one");
        let profile_two = profile(b"save-two");
        let same_ship = ship(b"same-ship-resource");
        let mut store = StateStore::default();

        assert!(
            store
                .set(profile_one, same_ship, PowerMode::Engine)
                .unwrap()
        );
        assert!(
            store
                .set(profile_two, same_ship, PowerMode::Shield)
                .unwrap()
        );
        assert_eq!(store.get(profile_one, same_ship), Some(PowerMode::Engine));
        assert_eq!(store.get(profile_two, same_ship), Some(PowerMode::Shield));
        assert_eq!(store.get(profile_one, ship(b"another-ship")), None);
    }

    #[test]
    fn unchanged_modes_do_not_dirty_the_store() {
        let profile = profile(b"save");
        let ship = ship(b"owned-ship");
        let mut store = StateStore::default();

        assert!(store.set(profile, ship, PowerMode::Balanced).unwrap());
        assert!(!store.set(profile, ship, PowerMode::Balanced).unwrap());
        assert!(store.set(profile, ship, PowerMode::Guns).unwrap());
    }

    #[test]
    fn profile_and_ship_limits_fail_without_discarding_existing_values() {
        let profile_id = profile(b"save");
        let mut ships = StateStore::default();
        for index in 0..super::MAX_SHIPS_PER_PROFILE {
            let material = index.to_le_bytes();
            ships
                .set(
                    profile_id,
                    ShipId::derive(&[&material]).unwrap(),
                    PowerMode::Balanced,
                )
                .unwrap();
        }
        let extra_ship = ship(b"the-65th-ship");
        assert!(matches!(
            ships.set(profile_id, extra_ship, PowerMode::Shield),
            Err(StateError::TooManyShips)
        ));
        assert_eq!(ships.get(profile_id, extra_ship), None);

        let mut profiles = StateStore::default();
        let same_ship = ship(b"ship");
        for index in 0..super::MAX_PROFILES {
            let material = index.to_le_bytes();
            profiles
                .set(
                    ProfileId::derive(&[&material]).unwrap(),
                    same_ship,
                    PowerMode::Balanced,
                )
                .unwrap();
        }
        let extra_profile = profile(b"the-17th-profile");
        assert!(matches!(
            profiles.set(extra_profile, same_ship, PowerMode::Shield),
            Err(StateError::TooManyProfiles)
        ));
        assert_eq!(profiles.get(extra_profile, same_ship), None);
    }

    #[test]
    fn encoding_is_deterministic_and_round_trips() {
        let mut store = StateStore::default();
        store
            .set(profile(b"profile-b"), ship(b"ship-z"), PowerMode::Shield)
            .unwrap();
        store
            .set(profile(b"profile-a"), ship(b"ship-a"), PowerMode::Engine)
            .unwrap();

        let encoded = store.encode().unwrap();
        assert_eq!(encoded, store.encode().unwrap());
        assert!(StateStore::decode(&encoded).unwrap() == store);
        assert_eq!(encoded.last(), Some(&b'\n'));
    }

    #[test]
    fn sidecar_does_not_contain_identity_material() {
        let mut store = StateStore::default();
        store
            .set(
                profile(b"private-save-slot-name"),
                ship(b"private-ship-resource-name"),
                PowerMode::Guns,
            )
            .unwrap();
        let encoded = store.encode().unwrap();

        assert!(
            !encoded
                .windows(b"private-save-slot-name".len())
                .any(|window| window == b"private-save-slot-name")
        );
        assert!(
            !encoded
                .windows(b"private-ship-resource-name".len())
                .any(|window| window == b"private-ship-resource-name")
        );
    }

    #[test]
    fn corrupt_future_and_oversized_files_fail_without_normalization() {
        assert!(matches!(StateStore::decode(b"{"), Err(StateError::Json(_))));
        assert!(matches!(
            StateStore::decode(br#"{"schema_version":2,"profiles":[]}"#),
            Err(StateError::UnsupportedSchema(2))
        ));
        assert!(matches!(
            StateStore::decode(&vec![b' '; super::MAX_STATE_FILE_BYTES + 1]),
            Err(StateError::FileTooLarge)
        ));
    }

    #[test]
    fn duplicate_records_and_unknown_fields_are_rejected() {
        let profile_id = "00".repeat(32);
        let ship_id = "11".repeat(32);
        let duplicate_ship = format!(
            "{{\"schema_version\":1,\"profiles\":[{{\"id\":\"{profile_id}\",\"ships\":[{{\"id\":\"{ship_id}\",\"mode\":\"engine\"}},{{\"id\":\"{ship_id}\",\"mode\":\"shield\"}}]}}]}}"
        );
        assert!(matches!(
            StateStore::decode(duplicate_ship.as_bytes()),
            Err(StateError::DuplicateShip)
        ));
        let duplicate_profile = format!(
            "{{\"schema_version\":1,\"profiles\":[{{\"id\":\"{profile_id}\",\"ships\":[{{\"id\":\"{ship_id}\",\"mode\":\"engine\"}}]}},{{\"id\":\"{profile_id}\",\"ships\":[{{\"id\":\"{}\",\"mode\":\"shield\"}}]}}]}}",
            "22".repeat(32)
        );
        assert!(matches!(
            StateStore::decode(duplicate_profile.as_bytes()),
            Err(StateError::DuplicateProfile)
        ));
        assert!(matches!(
            StateStore::decode(br#"{"schema_version":1,"profiles":[],"extra":true}"#),
            Err(StateError::Json(_))
        ));
        assert!(matches!(
            StateStore::decode(br#"{"schema_version":1,"profiles":[{"id":"0000000000000000000000000000000000000000000000000000000000000000","ships":[]}]}"#),
            Err(StateError::EmptyProfile)
        ));
    }

    #[test]
    fn atomic_save_replaces_existing_state_and_loads_the_new_value() {
        let directory = temporary_directory();
        fs::create_dir(&directory).unwrap();
        let path = directory.join("spsp-power-state.json");
        let profile = profile(b"save");
        let ship = ship(b"ship");

        let mut original = StateStore::default();
        original.set(profile, ship, PowerMode::Balanced).unwrap();
        original.save_atomic(&path).unwrap();

        let mut updated = StateStore::default();
        updated.set(profile, ship, PowerMode::Guns).unwrap();
        updated.save_atomic(&path).unwrap();

        assert!(StateStore::load(&path).unwrap() == updated);
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn invalid_existing_state_is_preserved() {
        let directory = temporary_directory();
        fs::create_dir(&directory).unwrap();
        let path = directory.join("spsp-power-state.json");
        let invalid = b"not-json";
        fs::write(&path, invalid).unwrap();

        assert!(matches!(StateStore::load(&path), Err(StateError::Json(_))));
        assert!(matches!(
            StateStore::default().save_atomic(&path),
            Err(StateError::Json(_))
        ));
        assert_eq!(fs::read(&path).unwrap(), invalid);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn non_regular_sidecar_paths_are_rejected() {
        let directory = temporary_directory();
        fs::create_dir_all(&directory).unwrap();
        assert!(matches!(
            StateStore::load(&directory),
            Err(StateError::InvalidStatePath)
        ));
        assert!(matches!(
            StateStore::default().save_atomic(&directory),
            Err(StateError::InvalidStatePath)
        ));
        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn atomic_sidecar_is_private_to_the_current_user() {
        use std::os::unix::fs::PermissionsExt;

        let directory = temporary_directory();
        fs::create_dir(&directory).unwrap();
        let path = directory.join("spsp-power-state.json");
        StateStore::default().save_atomic(&path).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;

        assert_eq!(mode, 0o600);
        fs::remove_dir_all(directory).unwrap();
    }
}
