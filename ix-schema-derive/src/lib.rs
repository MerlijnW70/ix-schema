//! Derive macro for [`ix-schema`](https://docs.rs/ix-schema): turns a struct into a
//! compile-time semantic `Manifest` (and, in later versions, a type-safe
//! migration edge).
//!
//! The macro runs entirely at compile time. It parses the struct into an
//! internal IR (`model::StructModel`), validates it, and lowers it to a
//! `const MANIFEST` plus an `impl Ix`. Layout figures are emitted as
//! `core::mem` calls so the compiler — not this macro — computes them, which
//! makes the manifest provably consistent with the real layout.
//!
//! Doc links cannot reach the `ix-schema` crate here (`ix-schema` depends on this
//! crate, not the reverse), so its types are referenced as plain code spans.
#![deny(rustdoc::broken_intra_doc_links, rustdoc::private_intra_doc_links)]

use proc_macro::TokenStream;
use proc_macro_error2::{abort, proc_macro_error};
use quote::quote;

mod model;

use model::{StructModel, VariantShape};

/// Derive `ix_schema::Ix`, publishing a compile-time semantic manifest.
///
/// Works on **structs** (named, tuple, newtype, or unit — tuple positions become
/// fields `"0"`, `"1"`, …) and **enums** — both fieldless (variants +
/// discriminants) and data-carrying (variants + payload field types;
/// variant-payload byte offsets are not modelled, since Rust gives no `const`
/// access to them). Only unions are rejected.
///
/// # Attributes
///
/// Struct-level `#[ix(...)]`:
/// * `version = N` — schema version (default `1`).
/// * `migrate_from = M` — declares this version evolved from version `M`.
///
/// Field-level `#[ix(...)]`:
/// * `since = N` — version the field was introduced in (default `1`).
/// * `default = EXPR` — value for a field absent in the previous version.
/// * `with = PATH` — function converting the predecessor's field.
/// * `rename_from = "old"` — the field's name in the previous version.
#[proc_macro_error]
#[proc_macro_derive(Ix, attributes(ix))]
pub fn derive_ix(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    let model = StructModel::analyze(&input);
    expand(&model).into()
}

/// Lower a validated `StructModel` to the `impl Ix` token stream.
fn expand(model: &StructModel) -> proc_macro2::TokenStream {
    let ident = &model.ident;
    let version = model.version;
    let repr = &model.repr;
    let type_doc = &model.doc;
    let (impl_generics, ty_generics, where_clause) = model.generics.split_for_impl();

    let field_specs = model.fields.iter().map(|f| {
        let name = &f.member;
        let ty = &f.ty;
        let since = f.since;
        let doc = &f.doc;
        quote! {
            ::ix_schema::FieldSpec {
                name: ::core::stringify!(#name),
                doc: #doc,
                type_name: ::core::stringify!(#ty),
                offset: ::core::mem::offset_of!(Self, #name),
                size: ::core::mem::size_of::<#ty>(),
                align: ::core::mem::align_of::<#ty>(),
                since: #since,
            }
        }
    });

    // A fieldless enum (all unit variants) can cast each variant to its integer
    // discriminant; a data-carrying enum cannot, so its discriminants are `None`.
    let enum_is_fieldless = !model.variants.is_empty()
        && model
            .variants
            .iter()
            .all(|v| matches!(v.kind, VariantShape::Unit));

    let variant_specs = model.variants.iter().map(|v| {
        let vname = &v.ident;
        let vdoc = &v.doc;
        let kind = match v.kind {
            VariantShape::Unit => quote!(::ix_schema::VariantKind::Unit),
            VariantShape::Tuple => quote!(::ix_schema::VariantKind::Tuple),
            VariantShape::Struct => quote!(::ix_schema::VariantKind::Struct),
        };
        // `Self::Variant as i64` is compiler-computed, so it cannot disagree with
        // the real discriminant; only valid when the whole enum is fieldless.
        let discriminant = if enum_is_fieldless {
            quote!(::core::option::Option::Some(Self::#vname as i64))
        } else {
            quote!(::core::option::Option::None)
        };
        // Payload fields: name (None for tuple positions), type, size, align — no
        // offset, which Rust cannot evaluate in const for an enum variant.
        let variant_fields = v.fields.iter().map(|f| {
            let ty = &f.ty;
            let name = match &f.name {
                Some(id) => quote!(::core::option::Option::Some(::core::stringify!(#id))),
                None => quote!(::core::option::Option::None),
            };
            quote! {
                ::ix_schema::VariantFieldSpec {
                    name: #name,
                    type_name: ::core::stringify!(#ty),
                    size: ::core::mem::size_of::<#ty>(),
                    align: ::core::mem::align_of::<#ty>(),
                }
            }
        });
        quote! {
            ::ix_schema::VariantSpec {
                name: ::core::stringify!(#vname),
                doc: #vdoc,
                discriminant: #discriminant,
                kind: #kind,
                fields: &[ #(#variant_fields),* ],
            }
        }
    });

    let (evolution_expr, migration_impl) = match &model.migrate_from {
        None => (quote!(::ix_schema::EvolutionSpec::GENESIS), quote!()),
        Some(prev_ty) => {
            let changes = model.fields.iter().filter_map(field_change);
            let removed = model
                .removed
                .iter()
                .map(|name| quote!(::ix_schema::FieldChange::Removed { name: #name }));
            let inits = model.fields.iter().map(field_init);
            let evolution = quote! {
                ::ix_schema::EvolutionSpec {
                    migrates_from: ::core::option::Option::Some(
                        <#prev_ty as ::ix_schema::Ix>::MANIFEST.schema_version
                    ),
                    changes: &[ #(#changes,)* #(#removed),* ],
                }
            };
            let migration = quote! {
                // Type-safe guard: the predecessor must be an older schema.
                const _: () = ::core::assert!(
                    <#prev_ty as ::ix_schema::Ix>::MANIFEST.schema_version < #version,
                    "ix-schema: `migrate_from` target must be an older schema version",
                );

                impl #impl_generics ::ix_schema::MigrateFrom<#prev_ty> for #ident #ty_generics
                #where_clause {
                    fn migrate_from(prev: #prev_ty) -> Self {
                        Self { #(#inits),* }
                    }
                }
            };
            (evolution, migration)
        }
    };

    let ix_impl = quote! {
        impl #impl_generics ::ix_schema::Ix for #ident #ty_generics #where_clause {
            const MANIFEST: ::ix_schema::Manifest<'static> = ::ix_schema::Manifest {
                type_name: ::core::concat!(::core::module_path!(), "::", ::core::stringify!(#ident)),
                doc: #type_doc,
                schema_version: #version,
                layout: ::ix_schema::LayoutSpec {
                    size: ::core::mem::size_of::<Self>(),
                    align: ::core::mem::align_of::<Self>(),
                    repr: #repr,
                },
                fields: &[ #(#field_specs),* ],
                variants: &[ #(#variant_specs),* ],
                evolution: #evolution_expr,
            };
        }
    };

    quote! {
        #ix_impl
        #migration_impl
    }
}

/// The constructor expression for one field of the migrated struct.
///
/// The four cases are exhaustive and each is checked by the type system:
/// a `default` field never touches `prev`; `with`/`rename_from`/carry-over all
/// reference `prev`, so a wrong type or missing field is a compile error.
fn field_init(field: &model::FieldModel) -> proc_macro2::TokenStream {
    let name = &field.member;
    if let Some(default) = &field.default {
        quote!(#name: #default)
    } else if let Some(with) = &field.with {
        quote!(#name: #with(prev.#name))
    } else if let Some(old) = &field.rename_from {
        let old = syn::Ident::new(old, field.span);
        quote!(#name: prev.#old)
    } else {
        quote!(#name: prev.#name)
    }
}

/// The `FieldChange` entry a field contributes to the evolution record, if any.
fn field_change(field: &model::FieldModel) -> Option<proc_macro2::TokenStream> {
    let name = &field.member;
    if field.with.is_some() {
        Some(quote!(::ix_schema::FieldChange::Transformed {
            name: ::core::stringify!(#name)
        }))
    } else if let Some(old) = &field.rename_from {
        Some(
            quote!(::ix_schema::FieldChange::Renamed { from: #old, to: ::core::stringify!(#name) }),
        )
    } else if field.default.is_some() {
        Some(quote!(::ix_schema::FieldChange::Added {
            name: ::core::stringify!(#name)
        }))
    } else {
        None
    }
}

/// Abort compilation if the same key appears twice in an attribute list.
fn reject_duplicate(seen: bool, meta: &syn::meta::ParseNestedMeta) -> syn::Result<()> {
    if seen {
        return Err(meta.error("duplicate `ix` attribute key"));
    }
    Ok(())
}

/// Consume and ignore a parenthesised group after a meta path (e.g. `align(8)`).
fn skip_optional_parens(meta: &syn::meta::ParseNestedMeta) -> syn::Result<()> {
    if meta.input.peek(syn::token::Paren) {
        let content;
        syn::parenthesized!(content in meta.input);
        let _: proc_macro2::TokenStream = content.parse()?;
    }
    Ok(())
}

/// Translate the `#[repr(..)]` attributes into a `::ix_schema::Repr` expression.
fn parse_repr(attrs: &[syn::Attribute]) -> proc_macro2::TokenStream {
    use proc_macro_error2::ResultExt as _;

    let mut repr = quote!(::ix_schema::Repr::Rust);
    for attr in attrs {
        if !attr.path().is_ident("repr") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("C") {
                repr = quote!(::ix_schema::Repr::C);
            } else if meta.path.is_ident("transparent") {
                repr = quote!(::ix_schema::Repr::Transparent);
            } else if meta.path.is_ident("packed") {
                if meta.input.peek(syn::token::Paren) {
                    let content;
                    syn::parenthesized!(content in meta.input);
                    let n: syn::LitInt = content.parse()?;
                    let n: usize = n.base10_parse()?;
                    repr = quote!(::ix_schema::Repr::Packed(#n));
                } else {
                    repr = quote!(::ix_schema::Repr::Packed(1));
                }
            } else {
                // align(n) and primitive enum reprs don't change struct identity
                // for our purposes; consume any payload and ignore.
                skip_optional_parens(&meta)?;
            }
            Ok(())
        })
        .unwrap_or_abort();
    }
    repr
}

/// Emit precise guidance on an item shape ix cannot model. Structs (all shapes)
/// and enums are handled elsewhere; only unions reach here.
fn abort_unsupported(input: &syn::DeriveInput) -> ! {
    match &input.data {
        syn::Data::Union(_) => abort!(
            input.ident,
            "`#[derive(Ix)]` supports structs and enums, not unions";
            help = "a union's fields overlap in memory; model it as a struct with a tag field"
        ),
        // Structs and enums are dispatched to their analysers; these arms only
        // trigger if a future shape reaches here.
        _ => abort!(input.ident, "`#[derive(Ix)]` could not model this type"),
    }
}
