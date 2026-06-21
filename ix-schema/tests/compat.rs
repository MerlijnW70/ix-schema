//! Compile-time layout/wire compatibility: a new schema version that only
//! *appends* fields stays readable by old code; reordering breaks it, and the
//! breakage is caught at compile time.

use ix_schema::{Ix, assert_compatible};

#[derive(Ix)]
#[repr(C)]
struct EventV1 {
    id: u32,
    kind: u16,
}

#[derive(Ix)]
#[ix(version = 2, migrate_from = EventV1)]
#[repr(C)]
struct EventV2 {
    id: u32,
    kind: u16,
    #[ix(since = 2, default = 0)]
    flags: u16,
}

// Proven at compile time: EventV2 only appended `flags`, so it extends EventV1.
assert_compatible!(EventV2 : EventV1);

// A hypothetical broken redesign that reorders the carried fields.
#[derive(Ix)]
#[repr(C)]
struct BadEvent {
    kind: u16,
    id: u32,
}

#[test]
fn append_only_extension_is_compatible() {
    let ok = EventV2::MANIFEST.extends(&EventV1::MANIFEST);
    assert!(ok);
}

#[test]
fn reordering_breaks_compatibility() {
    // Detected at compile time and confirmed at runtime: `id`/`kind` moved.
    const { assert!(!BadEvent::MANIFEST.extends(&EventV1::MANIFEST)) };
    let broken = BadEvent::MANIFEST.extends(&EventV1::MANIFEST);
    assert!(!broken);
}
