//! Multi-hop migration: a real v1 → v2 → v3 chain where the oldest version can
//! be upgraded directly to the newest in one type-safe, zero-cost call.

use ix_schema::{Ix, Upgrade};

#[derive(Ix)]
#[repr(C)]
struct DocV1 {
    id: u32,
}

#[derive(Ix)]
#[ix(version = 2, migrate_from = DocV1)]
#[repr(C)]
struct DocV2 {
    id: u32,
    #[ix(since = 2, default = 1)]
    revision: u16,
}

#[derive(Ix)]
#[ix(version = 3, migrate_from = DocV2)]
#[repr(C)]
struct DocV3 {
    id: u32,
    revision: u16,
    #[ix(since = 3, default = true)]
    archived: bool,
}

// Wire up the full transitive closure of upgrades across the chain.
ix_schema::migrate_chain!(DocV1 => DocV2 => DocV3);

#[test]
fn single_hops_are_available() {
    let v2: DocV2 = DocV1 { id: 10 }.upgrade();
    assert_eq!(v2.id, 10);
    assert_eq!(v2.revision, 1);

    let v3: DocV3 = DocV2 {
        id: 20,
        revision: 5,
    }
    .upgrade();
    assert_eq!(v3.id, 20);
    assert_eq!(v3.revision, 5);
    assert!(v3.archived);
}

#[test]
fn oldest_upgrades_directly_to_newest() {
    // One call walks v1 → v2 → v3, applying each version's defaults in order.
    let v3: DocV3 = DocV1 { id: 7 }.upgrade();
    assert_eq!(v3.id, 7);
    assert_eq!(v3.revision, 1); // default introduced at v2
    assert!(v3.archived); // default introduced at v3
}

#[test]
fn manifest_chain_records_each_predecessor() {
    assert_eq!(DocV1::MANIFEST.evolution.migrates_from, None);
    assert_eq!(DocV2::MANIFEST.evolution.migrates_from, Some(1));
    assert_eq!(DocV3::MANIFEST.evolution.migrates_from, Some(2));
}
