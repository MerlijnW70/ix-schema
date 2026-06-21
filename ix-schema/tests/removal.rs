//! A v1 → v2 migration that *drops* a field, recorded as `FieldChange::Removed`.

use ix_schema::{FieldChange, Ix, MigrateFrom};

#[derive(Ix)]
#[repr(C)]
struct RecordV1 {
    id: u32,
    legacy_checksum: u32,
}

#[derive(Ix)]
#[ix(version = 2, migrate_from = RecordV1, removed("legacy_checksum"))]
#[repr(C)]
struct RecordV2 {
    id: u32,
    #[ix(since = 2, default = 0)]
    epoch: u64,
}

#[test]
fn dropped_field_is_recorded_and_ignored_by_migration() {
    let v2 = RecordV2::migrate_from(RecordV1 {
        id: 5,
        legacy_checksum: 0xDEAD,
    });
    assert_eq!(v2.id, 5);
    assert_eq!(v2.epoch, 0);

    let changes = RecordV2::MANIFEST.evolution.changes;
    assert!(changes.contains(&FieldChange::Removed {
        name: "legacy_checksum",
    }));
    assert!(changes.contains(&FieldChange::Added { name: "epoch" }));
}
