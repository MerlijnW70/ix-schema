# ix-schema

**A universal meta-interface for data structures.**

`ix-schema` does not serialize anything itself. It is an **orchestrator**: a
`#[derive(Ix)]` type publishes a compile-time **manifest** describing its
fields, memory layout, and schema evolution. *Drivers* — thin adapters around
`serde`, `zerocopy`, and friends — read that manifest and do the real work,
branching on `const` data that folds away. No reflection, no runtime schema
parsing, no cost.

The core crate is `#![no_std]` and `#![forbid(unsafe_code)]`. Everything the
manifest carries is `const`, so it is free at runtime.

## Why

Two facts about a struct usually live in different worlds: its **memory layout**
(what `zerocopy` cares about) and its **schema** (what `serde` and your wire
format care about). `ix-schema` makes both a single, compiler-computed value:

```rust
use ix_schema::Ix;

#[derive(Ix)]
#[repr(C)]
struct User {
    id: u32,
    flags: u16,
}

// Layout figures come from the compiler via `core::mem`, so the manifest
// is *guaranteed* to match the real in-memory layout.
assert_eq!(User::MANIFEST.fields.len(), 2);
assert_eq!(User::MANIFEST.layout.repr, ix_schema::Repr::C);
```

Because the manifest is `const`, a driver can decide what it supports at compile
time, and the dead branches are eliminated:

```rust
use ix_schema::{Driver, Ix, Repr};

struct ZerocopyDriver;
impl<T: Ix> Driver<T> for ZerocopyDriver {
    // Folds to a constant; no runtime check survives.
    const SUPPORTED: bool =
        matches!(T::MANIFEST.layout.repr, Repr::C | Repr::Transparent)
            && T::MANIFEST.is_gap_free();
}
```

## Schema evolution, type-checked

Versions are wired with `migrate_from`. Each edge is generated field-by-field,
so an impossible transformation is a **type error**, not a runtime panic. A whole
chain composes into a direct oldest-to-newest upgrade:

```rust
use ix_schema::{Ix, Upgrade};

#[derive(Ix)]
#[repr(C)]
struct DocV1 { id: u32 }

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

ix_schema::migrate_chain!(DocV1 => DocV2 => DocV3);

// One call walks v1 -> v2 -> v3, applying each version's defaults in order.
let doc: DocV3 = DocV1 { id: 7 }.upgrade();
```

Wire compatibility is also a compile-time assertion. `assert_compatible!` fails
the build if a carried field is moved, resized, retyped, or dropped:

```rust
ix_schema::assert_compatible!(DocV2 : DocV1); // append-only — OK
```

## Field attributes

Struct-level `#[ix(...)]`:

| attribute            | meaning                                  |
| -------------------- | ---------------------------------------- |
| `version = N`        | schema version (default `1`)             |
| `migrate_from = T`   | this version evolved from type `T`       |
| `removed("a", "b")`  | fields dropped relative to the predecessor |

Field-level `#[ix(...)]`:

| attribute             | meaning                                       |
| --------------------- | --------------------------------------------- |
| `since = N`           | version the field was introduced in           |
| `default = EXPR`      | value for a field absent in the previous version |
| `with = PATH`         | function converting the predecessor's field   |
| `rename_from = "old"` | the field's name in the previous version      |

## Crates

| crate          | role                                                              |
| -------------- | ---------------------------------------------------------------- |
| `ix-schema`            | the core trait, the `const` manifest, and the migration plumbing |
| `ix-schema-derive`     | the `#[derive(Ix)]` proc-macro                                   |
| `ix-schema-serde`      | serde driver: version-tagged JSON and migrate/upgrade-on-decode  |
| `ix-schema-postcard`   | postcard driver: the same, over compact binary instead of JSON   |
| `ix-schema-toml`       | TOML driver: the same, over a config-shaped, table format        |
| `ix-schema-zerocopy`   | zerocopy driver: manifest-validated, zero-cost byte views        |
| `ix-schema-jsonschema` | JSON Schema (Draft 2020-12) generator from a type's manifest      |

`ix-schema-serde` (self-describing JSON), `ix-schema-postcard` (compact binary),
and `ix-schema-toml` (config-shaped) are the same version-and-migration logic over
three structurally different codecs — the manifest contract is format-agnostic, so
further backends (and consumers like `ix-schema-jsonschema`) drop in the same way.

## Status

`0.1` — the core model, the derive, and both drivers are implemented and tested.
The derive supports structs (named, tuple, newtype, unit) and enums — both
fieldless and data-carrying (variant payloads are described by type, though
variant-payload byte offsets are not modelled, as Rust exposes no `const` access
to them). Only unions are not yet modelled.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
