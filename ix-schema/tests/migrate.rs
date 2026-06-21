//! End-to-end test of compile-time, type-safe schema evolution: a real v1 → v2
//! migration expressed purely through `#[derive(Ix)]` attributes.

use ix_schema::{FieldChange, Ix, MigrateFrom};

#[derive(Ix)]
#[repr(C)]
struct UserV1 {
    id: u32,
    name_len: u16,
}

#[derive(Ix)]
#[ix(version = 2, migrate_from = UserV1)]
#[repr(C)]
struct UserV2 {
    // carried over unchanged
    id: u32,
    // renamed from `name_len`
    #[ix(rename_from = "name_len")]
    name_length: u16,
    // brand new in v2, initialised from a default
    #[ix(since = 2, default = 1)]
    schema_revision: u8,
    // brand new boolean flag, defaulted
    #[ix(since = 2, default = false)]
    verified: bool,
}

#[test]
fn migrate_lifts_v1_into_v2() {
    let v1 = UserV1 {
        id: 42,
        name_len: 7,
    };
    let v2 = UserV2::migrate_from(v1);

    assert_eq!(v2.id, 42);
    assert_eq!(v2.name_length, 7); // renamed field carried the value across
    assert_eq!(v2.schema_revision, 1); // default applied
    assert!(!v2.verified);
}

#[test]
fn manifest_records_the_evolution() {
    let m = UserV2::MANIFEST;
    assert_eq!(m.schema_version, 2);
    // migrates_from is read from UserV1's own manifest at compile time.
    assert_eq!(m.evolution.migrates_from, Some(1));

    let changes = m.evolution.changes;
    assert!(changes.contains(&FieldChange::Renamed {
        from: "name_len",
        to: "name_length",
    }));
    assert!(changes.contains(&FieldChange::Added {
        name: "schema_revision",
    }));
    assert!(changes.contains(&FieldChange::Added { name: "verified" }));
}

#[test]
fn with_function_transforms_a_field() {
    fn widen(n: u16) -> u32 {
        u32::from(n)
    }

    #[derive(Ix)]
    #[repr(C)]
    struct CounterV1 {
        ticks: u16,
    }

    #[derive(Ix)]
    #[ix(version = 2, migrate_from = CounterV1)]
    #[repr(C)]
    struct CounterV2 {
        #[ix(with = widen)]
        ticks: u32,
    }

    let v2 = CounterV2::migrate_from(CounterV1 { ticks: 300 });
    assert_eq!(v2.ticks, 300u32);

    assert!(
        CounterV2::MANIFEST
            .evolution
            .changes
            .contains(&FieldChange::Transformed { name: "ticks" })
    );
}
