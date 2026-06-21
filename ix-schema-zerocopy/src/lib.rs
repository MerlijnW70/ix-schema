//! # ix-schema-zerocopy — the zerocopy driver
//!
//! ix does not implement transmutation itself; it *orchestrates* the
//! [`zerocopy`] crate. This driver lets any type that is both [`Ix`] and
//! `zerocopy`-safe expose its bytes, while cross-checking the ix [`ix_schema::Manifest`]
//! against zerocopy's own guarantee at compile time.
//!
//! The soundness guarantee is `zerocopy`'s: the `IntoBytes` bound on
//! [`ByteView`] is what proves the type has no padding (at any depth), so a byte
//! view never exposes uninitialised bytes. ix adds a `const` cross-check that the
//! manifest's **top-level** [`ix_schema::Manifest::is_gap_free`] agrees — a
//! corroborating invariant, not an independent proof. (`is_gap_free` only sees
//! this type's own fields; padding nested inside a field's type is caught by the
//! `IntoBytes` bound, not by the manifest — see `Manifest::padding_bytes`.)
//!
//! Every decision is `const`; nothing here costs anything at runtime.
#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links, rustdoc::private_intra_doc_links)]

use ix_schema::{Driver, Ix, Repr};
use zerocopy::{FromBytes, Immutable, IntoBytes};

/// Read-only byte view of a [`Ix`] type, available only when the type is
/// zerocopy-safe to read as bytes.
pub trait ByteView: Ix + IntoBytes + Immutable {
    /// Borrow this value as its raw little-or-native-endian bytes.
    ///
    /// Soundness is the `IntoBytes` bound's (no padding, at any depth). The
    /// inline `const` assertion is a corroborating cross-check that ix's manifest
    /// agrees the *top level* is gap-free; it would only fire if the manifest and
    /// `zerocopy` disagreed about top-level layout.
    fn as_ix_bytes(&self) -> &[u8] {
        const {
            assert!(
                <Self as Ix>::MANIFEST.is_gap_free(),
                "ix-schema-zerocopy: manifest reports padding; a byte view would expose uninitialised bytes",
            );
        }
        self.as_bytes()
    }
}

impl<T: Ix + IntoBytes + Immutable> ByteView for T {}

/// Construct a [`Ix`] value from raw bytes, available only when the type is
/// zerocopy-safe to read from any bit pattern.
pub trait FromByteView: Ix + FromBytes + Sized {
    /// Parse `bytes` into a value, returning `None` if the length is wrong.
    fn from_ix_bytes(bytes: &[u8]) -> Option<Self> {
        Self::read_from_bytes(bytes).ok()
    }
}

impl<T: Ix + FromBytes> FromByteView for T {}

/// The zerocopy driver. [`Driver::SUPPORTED`] is `true` only for types whose
/// manifest declares a stable representation with no padding.
pub struct ZerocopyDriver;

impl<T: Ix> Driver<T> for ZerocopyDriver {
    const SUPPORTED: bool =
        matches!(T::MANIFEST.layout.repr, Repr::C | Repr::Transparent) && T::MANIFEST.is_gap_free();
}

#[cfg(test)]
mod tests {
    use super::*;
    use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

    #[derive(Ix, IntoBytes, FromBytes, Immutable, KnownLayout, Clone, Copy)]
    #[repr(C)]
    struct Point {
        x: u32,
        y: u32,
    }

    #[test]
    fn byte_view_roundtrips_through_zerocopy() {
        let p = Point { x: 7, y: 9 };
        let bytes = p.as_ix_bytes();
        assert_eq!(bytes.len(), 8);

        let q = Point::from_ix_bytes(bytes).expect("8 bytes is exactly one Point");
        assert_eq!(q.x, 7);
        assert_eq!(q.y, 9);
    }

    #[test]
    fn from_bytes_rejects_wrong_length() {
        assert!(Point::from_ix_bytes(&[0u8; 3]).is_none());
    }

    #[test]
    fn driver_supports_dense_repr_c() {
        const { assert!(<ZerocopyDriver as Driver<Point>>::SUPPORTED) };
        let supported = <ZerocopyDriver as Driver<Point>>::SUPPORTED;
        assert!(supported);
    }
}
