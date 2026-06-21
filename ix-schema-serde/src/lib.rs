//! # ix-schema-serde — the serde driver
//!
//! ix does not serialize anything itself; it *orchestrates* [`serde`]. This
//! driver adds the two things plain serde cannot express on its own:
//!
//! * **version tagging** — values are wrapped in an envelope carrying the
//!   schema version straight from the type's [`ix_schema::Manifest`], so a decoder
//!   detects an *older-version* payload instead of silently misreading it;
//! * **type-safe migrate-on-decode** — [`migrate_json`] reads that same envelope
//!   for a previous schema and lifts it through ix's compile-time [`MigrateFrom`]
//!   edge, so stored data composes directly with the migration path.
//!
//! serde does the encoding; ix supplies the version and the migration proof.
//!
//! **Scope of the tag.** The envelope carries the schema *version*, not the
//! type's *identity*. It catches version skew (reading v1 data as v2), but two
//! *different* types at the same version whose shapes happen to be compatible
//! will still deserialize without error — the tag is not a type-safety boundary.
//! Keep your own type discipline (distinct endpoints/keys) where that matters.
#![warn(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links, rustdoc::private_intra_doc_links)]

use ix_schema::{Ix, MigrateFrom, Upgrade};
use serde::{Serialize, de::DeserializeOwned};

/// Errors produced by the serde driver.
#[derive(Debug)]
pub enum Error {
    /// The underlying serde_json operation failed.
    Json(serde_json::Error),
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
            Error::Json(err) => write!(f, "serde_json error: {err}"),
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
            Error::Json(err) => Some(err),
            Error::VersionMismatch { .. } => None,
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::Json(err)
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

/// Serialize a value to JSON, tagged with its schema version from the manifest.
///
/// # Errors
/// Returns [`Error::Json`] if serialization fails.
pub fn to_versioned_json<T>(value: &T) -> Result<String, Error>
where
    T: Ix + Serialize,
{
    let envelope = OutEnvelope {
        schema_version: T::MANIFEST.schema_version,
        data: value,
    };
    Ok(serde_json::to_string(&envelope)?)
}

/// Decode a version-tagged envelope into `T`, verifying the tag matches `T`'s
/// declared schema version.
///
/// This is the one place the envelope format is understood, shared by
/// [`from_versioned_json`] and the migrate/upgrade readers so every entry point
/// agrees on what a stored, version-tagged value looks like.
fn decode_versioned<T>(json: &str) -> Result<T, Error>
where
    T: Ix + DeserializeOwned,
{
    let envelope: InEnvelope<T> = serde_json::from_str(json)?;
    let expected = T::MANIFEST.schema_version;
    if envelope.schema_version != expected {
        return Err(Error::VersionMismatch {
            expected,
            found: envelope.schema_version,
        });
    }
    Ok(envelope.data)
}

/// Deserialize a version-tagged JSON value, verifying the version matches `T`.
///
/// # Errors
/// Returns [`Error::Json`] on malformed input, or [`Error::VersionMismatch`] if
/// the envelope's version differs from `T`'s manifest version.
pub fn from_versioned_json<T>(json: &str) -> Result<T, Error>
where
    T: Ix + DeserializeOwned,
{
    decode_versioned(json)
}

/// Decode a version-tagged value of the *previous* schema and migrate it to the
/// current one in a single hop.
///
/// The input is the same envelope [`to_versioned_json`] produces, so stored data
/// composes directly with the migration path: the envelope's tag is checked
/// against `Prev`'s declared version (a wrong tag is an [`Error::VersionMismatch`]),
/// then ix's compile-time [`MigrateFrom`] edge lifts the value — an impossible
/// transformation is a type error, not a runtime failure.
///
/// # Errors
/// [`Error::Json`] on malformed input; [`Error::VersionMismatch`] if the
/// envelope's version is not `Prev`'s.
pub fn migrate_json<Prev, Cur>(versioned_json: &str) -> Result<Cur, Error>
where
    Prev: Ix + DeserializeOwned,
    Cur: MigrateFrom<Prev>,
{
    let prev: Prev = decode_versioned(versioned_json)?;
    Ok(Cur::migrate_from(prev))
}

/// Decode a version-tagged value of an *older* schema and upgrade it across a
/// whole migration chain to the current one.
///
/// Like [`migrate_json`] it consumes the [`to_versioned_json`] envelope and
/// verifies the tag is `Prev`'s version, but where `migrate_json` applies a
/// single [`MigrateFrom`] edge this walks the full [`Upgrade`] path wired by
/// [`ix_schema::migrate_chain!`] — so a `v1` payload decodes straight into `v3`,
/// applying every intermediate version's defaults in order, type-checked end to
/// end with no per-hop allocation.
///
/// # Errors
/// [`Error::Json`] on malformed input; [`Error::VersionMismatch`] if the
/// envelope's version is not `Prev`'s.
pub fn upgrade_json<Prev, Cur>(versioned_json: &str) -> Result<Cur, Error>
where
    Prev: Ix + DeserializeOwned + Upgrade<Cur>,
{
    let prev: Prev = decode_versioned(versioned_json)?;
    Ok(prev.upgrade())
}

/// The serde driver. Supported for any type that is both [`Ix`] and
/// serde-(de)serializable.
pub struct SerdeDriver;

impl<T> ix_schema::Driver<T> for SerdeDriver
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

    #[derive(Debug, Ix, Serialize, Deserialize)]
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
        let json = to_versioned_json(&cfg).expect("serialize");
        assert!(json.contains("\"schema_version\":1"));

        let back: ConfigV1 = from_versioned_json(&json).expect("deserialize");
        assert_eq!(back.id, 1);
        assert_eq!(back.name, "alpha");
    }

    #[test]
    fn wrong_version_is_rejected() {
        let json = r#"{"schema_version":99,"data":{"id":1,"name":"x"}}"#;
        let err = from_versioned_json::<ConfigV1>(json).expect_err("must reject");
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
        // Compose with the writer: store a v1 value as a versioned envelope, then
        // migrate that exact stored blob — no manual envelope stripping.
        let stored = to_versioned_json(&UserV1 { id: 5 }).expect("store");
        let v2: UserV2 = migrate_json::<UserV1, UserV2>(&stored).expect("migrate");
        assert_eq!(v2.id, 5);
        assert_eq!(v2.level, 9); // default supplied by the migration
        assert_eq!(UserV2::MANIFEST.evolution.migrates_from, Some(1));
    }

    #[test]
    fn migrate_rejects_a_wrongly_tagged_envelope() {
        // The envelope claims v2 but we ask to migrate it *as* v1 -> v2. The tag
        // is checked against the source version, so this is a clean mismatch
        // rather than a silent misread.
        let mistagged = r#"{"schema_version":2,"data":{"id":5}}"#;
        let err = migrate_json::<UserV1, UserV2>(mistagged).expect_err("must reject");
        assert!(matches!(
            err,
            Error::VersionMismatch {
                expected: 1,
                found: 2
            }
        ));
    }

    #[derive(Ix, Serialize, Deserialize)]
    #[ix(version = 3, migrate_from = UserV2)]
    struct UserV3 {
        id: u32,
        level: u8,
        #[ix(since = 3, default = false)]
        admin: bool,
    }

    // Wire the transitive closure so a v1 value can jump straight to v3.
    ix_schema::migrate_chain!(UserV1 => UserV2 => UserV3);

    #[test]
    fn upgrade_on_decode_walks_the_whole_chain() {
        // Store a v1 value as a versioned envelope, then upgrade that stored blob
        // straight into v3, applying each hop's defaults.
        let stored = to_versioned_json(&UserV1 { id: 5 }).expect("store");
        let v3: UserV3 = upgrade_json::<UserV1, UserV3>(&stored).expect("upgrade");
        assert_eq!(v3.id, 5);
        assert_eq!(v3.level, 9); // default from the v1 -> v2 edge
        assert!(!v3.admin); // default from the v2 -> v3 edge
        assert_eq!(UserV3::MANIFEST.evolution.migrates_from, Some(2));
    }

    // Two DIFFERENT types, both schema version 1, with the same JSON shape.
    #[derive(Ix, Serialize, Deserialize)]
    struct Account {
        id: u32,
        name: String,
    }
    #[derive(Debug, PartialEq, Ix, Serialize, Deserialize)]
    struct Product {
        id: u32,
        name: String,
    }

    #[test]
    fn the_tag_is_version_level_not_type_level() {
        // Documented limitation: the envelope tags the *version*, not the type.
        // An Account blob (v1) reads cleanly as a Product (v1) because the
        // version matches and the shapes coincide — no error is raised.
        let blob = to_versioned_json(&Account {
            id: 1,
            name: "alice".to_owned(),
        })
        .expect("store");
        let cross: Product = from_versioned_json(&blob).expect("same version, compatible shape");
        assert_eq!(
            cross,
            Product {
                id: 1,
                name: "alice".to_owned()
            }
        );
    }
}
