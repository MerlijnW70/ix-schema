//! End-to-end dogfooding of the ix stack, written the way an external consumer
//! would: declare versioned types with `#[derive(Ix)]`, then drive them through
//! the serde and zerocopy adapters and a type-safe migration chain.
//!
//! Run it with `cargo run -p ix-schema-example`; the same journey is asserted as a test.
#![deny(rustdoc::broken_intra_doc_links, rustdoc::private_intra_doc_links)]

use ix_schema::{Driver, Ix, Repr, assert_compatible, migrate_chain};
use ix_schema_serde::{from_versioned_json, to_versioned_json, upgrade_json};
use ix_schema_zerocopy::{ByteView, FromByteView};
use serde::{Deserialize, Serialize};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

// v1: a dense, gap-free reading. It is both serde- and zerocopy-safe.
#[repr(C)]
#[derive(
    Ix,
    Serialize,
    Deserialize,
    IntoBytes,
    FromBytes,
    Immutable,
    KnownLayout,
    Clone,
    Copy,
    Debug,
    PartialEq,
)]
struct SensorV1 {
    id: u32,
    reading: u32,
}

// v2: appends a `scale`. Old readers stay valid (see `assert_compatible!` below).
#[repr(C)]
#[derive(Ix, Serialize, Deserialize, Clone, Debug, PartialEq)]
#[ix(version = 2, migrate_from = SensorV1)]
struct SensorV2 {
    id: u32,
    reading: u32,
    #[ix(since = 2, default = 1)]
    scale: u32,
}

// v3: adds a `calibrated` flag. The trailing bool introduces padding, so this
// version is no longer zerocopy-safe — the capability gate below proves it.
#[repr(C)]
#[derive(Ix, Serialize, Deserialize, Clone, Debug, PartialEq)]
#[ix(version = 3, migrate_from = SensorV2)]
struct SensorV3 {
    id: u32,
    reading: u32,
    scale: u32,
    #[ix(since = 3, default = false)]
    calibrated: bool,
}

// Wire the transitive closure so a v1 value can upgrade straight to v3.
migrate_chain!(SensorV1 => SensorV2 => SensorV3);

// Compile-time proof: v2 only appends `scale`, so it is a layout-compatible,
// append-only extension of v1. Breaking this fails the build.
assert_compatible!(SensorV2 : SensorV1);

// A real driver: accept only stable, gap-free layouts. The decision is `const`
// and folds away at every call site.
struct ZerocopyDriver;
impl<T: Ix> Driver<T> for ZerocopyDriver {
    const SUPPORTED: bool =
        matches!(T::MANIFEST.layout.repr, Repr::C | Repr::Transparent) && T::MANIFEST.is_gap_free();
}

/// Everything the journey produced, returned so `main` can narrate it and the
/// test can assert on it.
struct Outcome {
    bytes_len: usize,
    zerocopy_roundtrip: SensorV1,
    json: String,
    json_roundtrip: SensorV1,
    upgraded: SensorV3,
    v1_supported: bool,
    v3_supported: bool,
}

/// Walk the full stack once, returning what each driver produced.
fn journey() -> Result<Outcome, ix_schema_serde::Error> {
    let v1 = SensorV1 {
        id: 7,
        reading: 100,
    };

    // 1. Zerocopy driver: borrow the value as raw bytes and reconstruct it.
    let bytes = v1.as_ix_bytes();
    let back = SensorV1::from_ix_bytes(bytes).expect("8 bytes is exactly one SensorV1");

    // 2. Serde driver: store as a version-tagged envelope, read it back.
    let json = to_versioned_json(&v1)?;
    let same: SensorV1 = from_versioned_json(&json)?;

    // 3. Migration: upgrade the *stored v1 blob* straight to v3 across the chain,
    //    applying each hop's defaults in order.
    let upgraded: SensorV3 = upgrade_json::<SensorV1, SensorV3>(&json)?;

    // 4. Capability gate, decided entirely at compile time.
    let v1_supported = <ZerocopyDriver as Driver<SensorV1>>::SUPPORTED;
    let v3_supported = <ZerocopyDriver as Driver<SensorV3>>::SUPPORTED;

    Ok(Outcome {
        bytes_len: bytes.len(),
        zerocopy_roundtrip: back,
        json,
        json_roundtrip: same,
        upgraded,
        v1_supported,
        v3_supported,
    })
}

fn main() {
    let o = journey().expect("the ix journey runs cleanly");
    println!(
        "zerocopy : {} bytes -> {:?}",
        o.bytes_len, o.zerocopy_roundtrip
    );
    println!("serde    : {}", o.json);
    println!("serde rt : {:?}", o.json_roundtrip);
    println!("upgrade  : v1 blob -> {:?}", o.upgraded);
    println!(
        "capability: zerocopy supports SensorV1={}, SensorV3={}",
        o.v1_supported, o.v3_supported
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_stack_journey_holds() {
        let o = journey().expect("journey");

        // Zerocopy roundtrip is lossless and exactly the struct's width.
        assert_eq!(o.bytes_len, 8);
        assert_eq!(
            o.zerocopy_roundtrip,
            SensorV1 {
                id: 7,
                reading: 100
            }
        );

        // Serde envelope is version-tagged and roundtrips unchanged.
        assert!(o.json.contains("\"schema_version\":1"));
        assert_eq!(
            o.json_roundtrip,
            SensorV1 {
                id: 7,
                reading: 100
            }
        );

        // Migration carried data across and applied each hop's default.
        assert_eq!(
            o.upgraded,
            SensorV3 {
                id: 7,
                reading: 100,
                scale: 1,          // default introduced at v2
                calibrated: false, // default introduced at v3
            }
        );

        // Capability gate: dense v1 is zerocopy-safe, padded v3 is not.
        assert!(o.v1_supported);
        assert!(!o.v3_supported);
    }

    // A fieldless (C-like) enum — newly supported by `#[derive(Ix)]`.
    #[repr(u8)]
    #[derive(Ix, Clone, Copy, Debug, PartialEq)]
    enum Mode {
        Idle,
        Active = 5,
        Faulted,
    }

    #[test]
    fn fieldless_enum_manifest_records_variants() {
        use ix_schema::VariantKind;
        let m = Mode::MANIFEST;
        // An enum carries variants, not fields.
        assert!(m.fields.is_empty());
        assert_eq!(m.variants.len(), 3);
        // Fieldless variants are unit-kind with no payload.
        assert!(m.variants.iter().all(|v| v.kind == VariantKind::Unit));
        assert!(m.variants.iter().all(|v| v.fields.is_empty()));
        // Discriminants are compiler-computed (`Self::Variant as i64`), so the
        // explicit `= 5` and the implicit `+1` after it are both exact.
        assert_eq!(m.variants[0].name, "Idle");
        assert_eq!(m.variants[0].discriminant, Some(0));
        assert_eq!(m.variants[1].name, "Active");
        assert_eq!(m.variants[1].discriminant, Some(5));
        assert_eq!(m.variants[2].name, "Faulted");
        assert_eq!(m.variants[2].discriminant, Some(6));
    }

    // A data-carrying enum — variants with tuple and struct payloads.
    #[derive(Ix, Clone, Debug)]
    enum Shape {
        Empty,
        Circle(f64),
        Rect { w: u32, h: u32 },
    }

    #[test]
    fn data_carrying_enum_records_variant_payloads() {
        use ix_schema::VariantKind;
        let m = Shape::MANIFEST;
        assert_eq!(m.variants.len(), 3);
        // A data-carrying enum cannot be cast to an integer, so discriminants
        // are not const-knowable.
        assert!(m.variants.iter().all(|v| v.discriminant.is_none()));

        // Empty: unit, no payload.
        assert_eq!(m.variants[0].name, "Empty");
        assert_eq!(m.variants[0].kind, VariantKind::Unit);
        assert!(m.variants[0].fields.is_empty());

        // Circle(f64): tuple, one unnamed 8-byte field.
        assert_eq!(m.variants[1].kind, VariantKind::Tuple);
        assert_eq!(m.variants[1].fields.len(), 1);
        assert_eq!(m.variants[1].fields[0].name, None);
        assert_eq!(m.variants[1].fields[0].type_name, "f64");
        assert_eq!(m.variants[1].fields[0].size, 8);

        // Rect { w, h }: struct, two named u32 fields (offsets intentionally absent).
        assert_eq!(m.variants[2].kind, VariantKind::Struct);
        assert_eq!(m.variants[2].fields.len(), 2);
        assert_eq!(m.variants[2].fields[0].name, Some("w"));
        assert_eq!(m.variants[2].fields[1].name, Some("h"));
        assert_eq!(m.variants[2].fields[1].type_name, "u32");

        // Construct and read each variant's payload so the fixture is fully
        // exercised (not just declared) — the derive references variants only for
        // the fieldless discriminant cast, which a data-carrying enum has none of.
        let _ = Shape::Empty;
        match Shape::Circle(2.0) {
            Shape::Circle(r) => assert_eq!(r, 2.0),
            _ => unreachable!(),
        }
        match (Shape::Rect { w: 3, h: 4 }) {
            Shape::Rect { w, h } => assert_eq!(w + h, 7),
            _ => unreachable!(),
        }
    }

    /// A documented reading.
    #[derive(Ix, Clone, Copy, Debug, PartialEq)]
    #[repr(C)]
    struct Documented {
        /// the unique id
        id: u32,
    }

    /// modes of operation
    #[derive(Ix, Clone, Copy, Debug)]
    #[repr(u8)]
    enum DocMode {
        /// the idle state
        Idle,
    }

    #[test]
    fn doc_comments_are_captured_into_the_manifest() {
        // Type, field, and variant `///` comments all land in the manifest —
        // the metadata facet/utoipa expose and downstream schema consumers need.
        let d = Documented { id: 7 };
        assert_eq!(d.id, 7);
        assert_eq!(Documented::MANIFEST.doc, "A documented reading.");
        assert_eq!(Documented::MANIFEST.fields[0].doc, "the unique id");

        assert_eq!(DocMode::MANIFEST.doc, "modes of operation");
        assert_eq!(DocMode::MANIFEST.variants[0].doc, "the idle state");
        let _ = DocMode::Idle;
    }

    // A tuple (newtype-ish) struct and a unit struct — newly supported.
    #[derive(Ix, Clone, Copy, Debug, PartialEq)]
    #[repr(C)]
    struct Pair(u32, u64);

    #[derive(Ix, Clone, Copy, Debug, PartialEq)]
    struct Marker;

    #[test]
    fn tuple_and_unit_structs_reflect() {
        // Tuple positions become fields named "0","1" with real offsets via
        // offset_of!(Self, 0) — the same compiler-computed guarantee as named.
        let p = Pair(1, 2);
        assert_eq!((p.0, p.1), (1, 2));
        let m = Pair::MANIFEST;
        assert_eq!(m.fields.len(), 2);
        assert_eq!(m.fields[0].name, "0");
        assert_eq!(m.fields[0].offset, 0);
        assert_eq!(m.fields[1].name, "1");
        assert_eq!(m.fields[1].type_name, "u64");
        assert_eq!(m.fields[1].offset, 8); // u64 aligned to 8 after the u32

        // A unit struct has neither fields nor variants.
        let _ = Marker;
        assert!(Marker::MANIFEST.fields.is_empty());
        assert!(Marker::MANIFEST.variants.is_empty());
    }

    // Migrating a tuple struct: positional carry-over + a defaulted new position.
    #[derive(Ix, Clone, Copy, Debug, PartialEq)]
    #[repr(C)]
    struct CounterV1(u32);

    #[derive(Ix, Clone, Copy, Debug, PartialEq)]
    #[repr(C)]
    #[ix(version = 2, migrate_from = CounterV1)]
    struct CounterV2(u32, #[ix(since = 2, default = 7)] u8);

    #[test]
    fn tuple_struct_migration_carries_positions() {
        // field 0 is carried from the predecessor; field 1 is the v2 default.
        let v2 = <CounterV2 as ix_schema::MigrateFrom<CounterV1>>::migrate_from(CounterV1(42));
        assert_eq!(v2.0, 42);
        assert_eq!(v2.1, 7);
        assert_eq!(CounterV2::MANIFEST.evolution.migrates_from, Some(1));
    }

    // The nested-padding boundary: is_gap_free is a TOP-LEVEL check only.
    #[derive(Ix)]
    #[repr(C)]
    struct Padded {
        a: u32,
        b: u8,
    } // u32 + u8 in 8 bytes => 3 trailing padding bytes

    #[derive(Ix)]
    #[repr(C)]
    struct WrapsPadded {
        inner: Padded,
    }

    #[test]
    fn is_gap_free_is_top_level_only() {
        // A type's own padding is caught at its own level…
        assert!(!Padded::MANIFEST.is_gap_free());
        assert_eq!(Padded::MANIFEST.padding_bytes(), 3);
        // …but a wrapper sees only top-level field *sizes*, so it reports
        // gap-free even though `Padded` is padded. Documented and safe: the
        // zerocopy driver's `IntoBytes` bound — not `is_gap_free` — is what
        // actually rejects a byte view of a (nested-)padded type.
        assert!(WrapsPadded::MANIFEST.is_gap_free());
        assert_eq!(WrapsPadded::MANIFEST.padding_bytes(), 0);
    }

    // extends() matches fields by NAME, so a rename is not a layout extension.
    #[derive(Ix)]
    #[repr(C)]
    struct CountV1 {
        count: u32,
    }
    #[derive(Ix)]
    #[repr(C)]
    struct CountRenamed {
        total: u32,
    }

    #[test]
    fn extends_requires_field_name_stability() {
        // same offset/size/type, only the name differs -> not an extension.
        assert!(!CountRenamed::MANIFEST.extends(&CountV1::MANIFEST));
        // sanity: an identical manifest still extends itself.
        assert!(CountV1::MANIFEST.extends(&CountV1::MANIFEST));
    }
}
