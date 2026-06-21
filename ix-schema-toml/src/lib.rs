//! # ix-schema-toml — the TOML driver
//!
//! A third backend for ix-schema, alongside
//! [`ix-schema-serde`](https://docs.rs/ix-schema-serde) (self-describing JSON)
//! and [`ix-schema-postcard`](https://docs.rs/ix-schema-postcard) (compact
//! binary): the same version-tagging and type-safe migrate/upgrade-on-decode,
//! but over [`toml`]'s config-shaped, table-oriented format. Three structurally
//! different data models on the one [`ix_schema::Manifest`] is the orchestrator
//! claim made concrete — only the codec changes.
#![warn(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links, rustdoc::private_intra_doc_links)]

use ix_schema::{Ix, MigrateFrom, Upgrade};
use serde::{Serialize, de::DeserializeOwned};

/// Errors produced by the TOML driver.
#[derive(Debug)]
pub enum Error {
    /// TOML serialization failed.
    Serialize(toml::ser::Error),
    /// TOML deserialization failed.
    Deserialize(toml::de::Error),
    /// The envelope's schema version did not match the target type's version.
    VersionMismatch {
        /// Version the target type declares in its manifest.
        expected: u32,
        /// Version found in the decoded envelope.
        found: u32,
    },
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::Serialize(err) => write!(f, "toml serialize error: {err}"),
            Error::Deserialize(err) => write!(f, "toml deserialize error: {err}"),
            Error::VersionMismatch { expected, found } => write!(
                f,
                "schema version mismatch: envelope is v{found} but the type is v{expected}"
            ),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Serialize(err) => Some(err),
            Error::Deserialize(err) => Some(err),
            Error::VersionMismatch { .. } => None,
        }
    }
}

impl From<toml::ser::Error> for Error {
    fn from(err: toml::ser::Error) -> Self {
        Error::Serialize(err)
    }
}

impl From<toml::de::Error> for Error {
    fn from(err: toml::de::Error) -> Self {
        Error::Deserialize(err)
    }
}

// `schema_version` (a scalar) is declared before `data` (a table) so the emitted
// TOML is valid — scalars must precede tables at the same level.
#[derive(Serialize)]
struct OutEnvelope<'a, T> {
    schema_version: u32,
    data: &'a T,
}

#[derive(serde::Deserialize)]
struct InEnvelope<T> {
    schema_version: u32,
    data: T,
}

/// Serialize a value to TOML, tagged with its schema version from the manifest.
///
/// # Errors
/// Returns [`Error::Serialize`] if serialization fails.
pub fn to_versioned_toml<T>(value: &T) -> Result<String, Error>
where
    T: Ix + Serialize,
{
    let envelope = OutEnvelope {
        schema_version: T::MANIFEST.schema_version,
        data: value,
    };
    Ok(toml::to_string(&envelope)?)
}

/// Decode a version-tagged TOML envelope into `T`, verifying the tag matches
/// `T`'s declared schema version. Shared by every reader.
fn decode_versioned<T>(toml_str: &str) -> Result<T, Error>
where
    T: Ix + DeserializeOwned,
{
    let envelope: InEnvelope<T> = toml::from_str(toml_str)?;
    let expected = T::MANIFEST.schema_version;
    if envelope.schema_version != expected {
        return Err(Error::VersionMismatch {
            expected,
            found: envelope.schema_version,
        });
    }
    Ok(envelope.data)
}

/// Deserialize a version-tagged TOML value, verifying the version matches `T`.
///
/// # Errors
/// Returns [`Error::Deserialize`] on malformed input, or [`Error::VersionMismatch`]
/// if the envelope's version differs from `T`'s manifest version.
pub fn from_versioned_toml<T>(toml_str: &str) -> Result<T, Error>
where
    T: Ix + DeserializeOwned,
{
    decode_versioned(toml_str)
}

/// Decode a version-tagged TOML value of the *previous* schema and migrate it to
/// the current one in a single hop.
///
/// # Errors
/// [`Error::Deserialize`] on malformed input; [`Error::VersionMismatch`] if the
/// envelope's version is not `Prev`'s.
pub fn migrate_toml<Prev, Cur>(versioned_toml: &str) -> Result<Cur, Error>
where
    Prev: Ix + DeserializeOwned,
    Cur: MigrateFrom<Prev>,
{
    let prev: Prev = decode_versioned(versioned_toml)?;
    Ok(Cur::migrate_from(prev))
}

/// Decode a version-tagged TOML value of an *older* schema and upgrade it across
/// the whole migration chain (see [`ix_schema::migrate_chain!`]).
///
/// # Errors
/// [`Error::Deserialize`] on malformed input; [`Error::VersionMismatch`] if the
/// envelope's version is not `Prev`'s.
pub fn upgrade_toml<Prev, Cur>(versioned_toml: &str) -> Result<Cur, Error>
where
    Prev: Ix + DeserializeOwned + Upgrade<Cur>,
{
    let prev: Prev = decode_versioned(versioned_toml)?;
    Ok(prev.upgrade())
}

/// The TOML driver. Supported for any type that is both [`Ix`] and
/// serde-(de)serializable.
pub struct TomlDriver;

impl<T> ix_schema::Driver<T> for TomlDriver
where
    T: Ix + Serialize + DeserializeOwned,
{
    const SUPPORTED: bool = true;
}

#[cfg(test)]
mod tests {
    use super::*;
    use ix_schema::Ix;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Ix, Serialize, Deserialize)]
    struct ConfigV1 {
        id: u32,
        name: String,
    }

    #[test]
    fn versioned_roundtrip_preserves_data() {
        let cfg = ConfigV1 {
            id: 1,
            name: "alpha".to_owned(),
        };
        let text = to_versioned_toml(&cfg).expect("serialize");
        assert!(text.contains("schema_version = 1"));
        let back: ConfigV1 = from_versioned_toml(&text).expect("deserialize");
        assert_eq!(back, cfg);
    }

    #[test]
    fn wrong_version_is_rejected() {
        let text = "schema_version = 99\n\n[data]\nid = 1\nname = \"x\"\n";
        let err = from_versioned_toml::<ConfigV1>(text).expect_err("must reject");
        assert!(matches!(
            err,
            Error::VersionMismatch {
                expected: 1,
                found: 99
            }
        ));
    }

    #[derive(Ix, Serialize, Deserialize)]
    struct UserV1 {
        id: u32,
    }

    #[derive(Debug, Ix, Serialize, Deserialize)]
    #[ix(version = 2, migrate_from = UserV1)]
    struct UserV2 {
        id: u32,
        #[ix(since = 2, default = 9)]
        level: u8,
    }

    #[test]
    fn migrate_on_decode_lifts_old_payload() {
        let stored = to_versioned_toml(&UserV1 { id: 5 }).expect("store");
        let v2: UserV2 = migrate_toml::<UserV1, UserV2>(&stored).expect("migrate");
        assert_eq!(v2.id, 5);
        assert_eq!(v2.level, 9);
    }
}
