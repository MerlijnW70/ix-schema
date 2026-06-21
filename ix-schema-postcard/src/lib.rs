//! # ix-schema-postcard — the postcard (binary) driver
//!
//! A second backend for ix-schema, alongside [`ix-schema-serde`](https://docs.rs/ix-schema-serde):
//! where that one tags and migrates over self-describing JSON, this one does the
//! same over [`postcard`]'s compact binary format. Two structurally different
//! formats driven by the one [`ix_schema::Manifest`] is the orchestrator claim made
//! concrete — the version and migration logic is format-agnostic, only the codec
//! changes.
//!
//! As with the serde driver, the migrate/upgrade readers consume the same
//! version-tagged envelope the writer produces, so stored bytes compose directly
//! with the migration path. The same caveat applies: the envelope tags the schema
//! *version*, not the type's *identity*, so two different types at the same
//! version with a compatible layout can decode without error — and because
//! postcard is non-self-describing, that mismatch is even easier to hit than with
//! JSON. The version tag is not a type-safety boundary.
#![warn(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links, rustdoc::private_intra_doc_links)]

use ix_schema::{Ix, MigrateFrom, Upgrade};
use serde::{Serialize, de::DeserializeOwned};

/// Errors produced by the postcard driver.
#[derive(Debug)]
pub enum Error {
    /// The underlying postcard operation failed.
    Postcard(postcard::Error),
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
            Error::Postcard(err) => write!(f, "postcard error: {err}"),
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
            Error::Postcard(err) => Some(err),
            Error::VersionMismatch { .. } => None,
        }
    }
}

impl From<postcard::Error> for Error {
    fn from(err: postcard::Error) -> Self {
        Error::Postcard(err)
    }
}

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

/// Serialize a value to postcard bytes, tagged with its schema version.
///
/// # Errors
/// Returns [`Error::Postcard`] if serialization fails.
pub fn to_versioned_postcard<T>(value: &T) -> Result<Vec<u8>, Error>
where
    T: Ix + Serialize,
{
    let envelope = OutEnvelope {
        schema_version: T::MANIFEST.schema_version,
        data: value,
    };
    Ok(postcard::to_allocvec(&envelope)?)
}

/// Decode a version-tagged envelope into `T`, verifying the tag matches `T`'s
/// declared schema version. Shared by every reader so they agree on the format.
fn decode_versioned<T>(bytes: &[u8]) -> Result<T, Error>
where
    T: Ix + DeserializeOwned,
{
    let envelope: InEnvelope<T> = postcard::from_bytes(bytes)?;
    let expected = T::MANIFEST.schema_version;
    if envelope.schema_version != expected {
        return Err(Error::VersionMismatch {
            expected,
            found: envelope.schema_version,
        });
    }
    Ok(envelope.data)
}

/// Deserialize version-tagged postcard bytes, verifying the version matches `T`.
///
/// # Errors
/// Returns [`Error::Postcard`] on malformed input, or [`Error::VersionMismatch`]
/// if the envelope's version differs from `T`'s manifest version.
pub fn from_versioned_postcard<T>(bytes: &[u8]) -> Result<T, Error>
where
    T: Ix + DeserializeOwned,
{
    decode_versioned(bytes)
}

/// Decode version-tagged bytes of the *previous* schema and migrate them to the
/// current one in a single hop. Consumes the [`to_versioned_postcard`] envelope
/// and verifies the tag is `Prev`'s version.
///
/// # Errors
/// [`Error::Postcard`] on malformed input; [`Error::VersionMismatch`] if the
/// envelope's version is not `Prev`'s.
pub fn migrate_postcard<Prev, Cur>(versioned_bytes: &[u8]) -> Result<Cur, Error>
where
    Prev: Ix + DeserializeOwned,
    Cur: MigrateFrom<Prev>,
{
    let prev: Prev = decode_versioned(versioned_bytes)?;
    Ok(Cur::migrate_from(prev))
}

/// Decode version-tagged bytes of an *older* schema and upgrade them across the
/// whole migration chain to the current one (see [`ix_schema::migrate_chain!`]).
///
/// # Errors
/// [`Error::Postcard`] on malformed input; [`Error::VersionMismatch`] if the
/// envelope's version is not `Prev`'s.
pub fn upgrade_postcard<Prev, Cur>(versioned_bytes: &[u8]) -> Result<Cur, Error>
where
    Prev: Ix + DeserializeOwned + Upgrade<Cur>,
{
    let prev: Prev = decode_versioned(versioned_bytes)?;
    Ok(prev.upgrade())
}

/// The postcard driver. Supported for any type that is both [`Ix`] and
/// serde-(de)serializable.
pub struct PostcardDriver;

impl<T> ix_schema::Driver<T> for PostcardDriver
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
        let bytes = to_versioned_postcard(&cfg).expect("serialize");
        let back: ConfigV1 = from_versioned_postcard(&bytes).expect("deserialize");
        assert_eq!(back, cfg);
    }

    #[test]
    fn wrong_version_is_rejected() {
        // A hand-built envelope tagged v99, byte-compatible with InEnvelope.
        #[derive(Serialize)]
        struct FakeEnvelope {
            schema_version: u32,
            data: ConfigV1,
        }
        let bytes = postcard::to_allocvec(&FakeEnvelope {
            schema_version: 99,
            data: ConfigV1 {
                id: 1,
                name: "x".to_owned(),
            },
        })
        .expect("encode");
        let err = from_versioned_postcard::<ConfigV1>(&bytes).expect_err("must reject");
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
        // Store a v1 value, then migrate that exact stored blob to v2.
        let stored = to_versioned_postcard(&UserV1 { id: 5 }).expect("store");
        let v2: UserV2 = migrate_postcard::<UserV1, UserV2>(&stored).expect("migrate");
        assert_eq!(v2.id, 5);
        assert_eq!(v2.level, 9); // default supplied by the migration
    }
}
