//! Integration tests for `#[derive(Ix)]`. These compile real structs through
//! the proc-macro and assert the emitted manifest matches the declared shape.

use ix_schema::{Driver, Ix, Repr};

#[derive(Ix)]
#[repr(C)]
struct Packet {
    id: u32,
    kind: u16,
    flag: u8,
}

#[test]
fn derive_emits_manifest_for_named_struct() {
    let m = Packet::MANIFEST;
    assert_eq!(m.schema_version, 1);
    assert_eq!(m.layout.repr, Repr::C);
    assert_eq!(m.fields.len(), 3);
    assert_eq!(m.fields[0].name, "id");
    assert_eq!(m.fields[1].name, "kind");
    assert_eq!(m.fields[2].name, "flag");
    assert!(m.type_name.ends_with("::Packet"));
}

#[test]
fn derive_offsets_track_real_layout() {
    let m = Packet::MANIFEST;
    // repr(C): u32 @0, u16 @4, u8 @6. Offsets are emitted as offset_of! so the
    // compiler computes them — this asserts the macro wired them correctly.
    assert_eq!(m.fields[0].offset, 0);
    assert_eq!(m.fields[1].offset, 4);
    assert_eq!(m.fields[2].offset, 6);
    assert_eq!(m.packed_field_bytes(), 7);
}

#[test]
fn version_attribute_is_honoured() {
    #[derive(Ix)]
    #[ix(version = 3)]
    #[repr(C)]
    struct Versioned {
        x: u64,
    }
    assert_eq!(Versioned::MANIFEST.schema_version, 3);
}

#[test]
fn since_attribute_records_field_origin() {
    #[derive(Ix)]
    #[ix(version = 2)]
    #[repr(C)]
    struct Grown {
        original: u32,
        #[ix(since = 2)]
        added: u32,
    }
    let m = Grown::MANIFEST;
    assert_eq!(m.fields[0].since, 1);
    assert_eq!(m.fields[1].since, 2);
}

// A real driver: a zerocopy adapter accepts only stable, gap-free layouts. The
// decision is const and folds away — proving the orchestrator design.
struct ZerocopyDriver;
impl<T: Ix> Driver<T> for ZerocopyDriver {
    const SUPPORTED: bool =
        matches!(T::MANIFEST.layout.repr, Repr::C | Repr::Transparent) && T::MANIFEST.is_gap_free();
}

#[derive(Ix)]
#[repr(C)]
struct Dense {
    a: u32,
    b: u32,
}

#[test]
fn driver_accepts_dense_rejects_padded() {
    // Dense (u32,u32) is gap-free → supported. Packet (u32,u16,u8 in 8 bytes)
    // has a trailing padding byte → rejected. Both decided at compile time.
    const { assert!(<ZerocopyDriver as Driver<Dense>>::SUPPORTED) };
    const { assert!(!<ZerocopyDriver as Driver<Packet>>::SUPPORTED) };
    let dense_ok = <ZerocopyDriver as Driver<Dense>>::SUPPORTED;
    let packet_ok = <ZerocopyDriver as Driver<Packet>>::SUPPORTED;
    assert!(dense_ok);
    assert!(!packet_ok);
}
