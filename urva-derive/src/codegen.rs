use proc_macro2::TokenStream;
use quote::{format_ident, quote, quote_spanned};
use syn::Ident;
use syn::ext::IdentExt;

use crate::case::to_snake;
use crate::entity::{EntityModel, FieldModel, StructModel, same_ident, token_type};
use crate::index_attr::{KeyDecl, KeyKindDecl};

pub fn entity_tokens(model: &EntityModel) -> TokenStream {
    let base = &model.base;
    let ident = &base.ident;
    let snake = to_snake(&ident.unraw().to_string());
    let fields_mod = format_ident!("{snake}_fields");
    let index_mod = format_ident!("{snake}_index");
    let fields_module = fields_module(
        base,
        &fields_mod,
        Some((&model.id_ty, model.version.as_deref())),
    );
    let index_module = index_module_tokens(model, &index_mod);

    let collection = &model.collection;
    let id_ty = &model.id_ty;
    let spec_refs = model.indexes.iter().map(|decl| {
        let r = &decl.ident;
        quote! { #index_mod::#r.spec() }
    });

    let (lock, version_field, marker) = match &model.version {
        Some(name) => (
            quote! { ::urva::Version },
            name.as_str(),
            quote! { ::urva::Versioned },
        ),
        None => (quote! { () }, "version", quote! { ::urva::Unversioned }),
    };

    let ttl_asserts = model.indexes.iter().flat_map(|decl| {
        let ttl_span = decl.ttl.map(|(_, span)| span);
        decl.keys.iter().filter_map(move |key| {
            let ttl_span = ttl_span?;
            let field = base
                .fields
                .iter()
                .find(|f| same_ident(&f.ident, &key.segments[0]))?;
            let leaf_ty = declared_or_encoded(field);
            Some(quote_spanned! {ttl_span=>
                const _: () = {
                    fn ttl_key_must_be_a_date<T: ::urva::Accepts<::urva::TtlKey>>() {}
                    let _ = ttl_key_must_be_a_date::<#leaf_ty>;
                };
            })
        })
    });
    let ttl_asserts: TokenStream = ttl_asserts.collect();

    quote! {
        #fields_module

        #index_module

        const _: () = {
        #[automatically_derived]
        impl ::urva::__private::Sealed for #ident {}

        #[automatically_derived]
        impl ::urva::Entity for #ident {
            const COLLECTION: &'static str = #collection;
            type Id = #id_ty;
            type Lock = #lock;
            const VERSION_FIELD: &'static str = #version_field;
            const INDEX_SPECS: &'static [&'static ::urva::IndexSpec] = &[#(#spec_refs),*];
        }

        #[automatically_derived]
        impl #marker for #ident {}

        #ttl_asserts
        };
    }
}

fn declared_or_encoded(field: &FieldModel) -> TokenStream {
    match &field.custom_writer_path {
        Some(_) => quote! { ::urva::__private::CustomWritten },
        None => {
            let ty = &field.ty;
            quote! { #ty }
        }
    }
}

pub fn embedded_tokens(base: &StructModel) -> TokenStream {
    let ident = &base.ident;
    let fields_mod = format_ident!("{}_fields", to_snake(&ident.unraw().to_string()));
    let fields_module = fields_module(base, &fields_mod, None);
    quote! {
        #[automatically_derived]
        impl ::urva::__private::Sealed for #ident {}

        #[automatically_derived]
        impl ::urva::Embedded for #ident {}

        #fields_module
    }
}

fn encoder_ident(base: &StructModel, field: &FieldModel) -> Ident {
    format_ident!(
        "__urva_enc_{}_{}",
        to_snake(&base.ident.unraw().to_string()),
        field.ident.unraw()
    )
}

fn fields_module(
    base: &StructModel,
    fields_mod: &Ident,
    entity: Option<(&syn::Type, Option<&str>)>,
) -> TokenStream {
    let ident = &base.ident;
    let vis = &base.vis;
    let mut encoders = TokenStream::new();
    let mut consts: Vec<TokenStream> = base
        .fields
        .iter()
        .filter(|field| field.unstorable.is_none())
        .map(|field| {
            let fident = &field.ident;
            let stored = &field.stored_name;
            match &field.custom_writer_path {
                Some(writer) => {
                    let ty = &field.ty;
                    let encoder = encoder_ident(base, field);
                    encoders.extend(quote! {
                        #[doc(hidden)]
                        #[allow(non_camel_case_types)]
                        #vis struct #encoder;
                        #[automatically_derived]
                        impl ::urva::Encode<#ty> for #encoder {
                            fn encode(
                                value: &#ty,
                            ) -> ::core::result::Result<::urva::bson::Bson, ::urva::bson::ser::Error> {
                                #writer(value, ::urva::bson::Serializer::new())
                            }
                        }
                    });
                    quote! {
                        pub const #fident: ::urva::Field<#ident, #ty, ::urva::Full, ::urva::Encoded<#encoder>> =
                            ::urva::__private::new_field(#stored);
                    }
                }
                None => {
                    let ty = token_type(&field.ty);
                    quote! {
                        pub const #fident: ::urva::Field<#ident, #ty> =
                            ::urva::__private::new_field(#stored);
                    }
                }
            }
        })
        .collect();
    if let Some((id_ty, version)) = entity {
        consts.push(quote! {
            pub const _id: ::urva::Field<#ident, #id_ty, ::urva::MatchOnly> =
                ::urva::__private::new_field("_id");
        });
        if let Some(version) = version {
            let vident = format_ident!("{version}");
            consts.push(quote! {
                pub const #vident: ::urva::VersionField<#ident> =
                    ::urva::__private::new_version_field(#version);
            });
        }
    }
    quote! {
        #[allow(non_camel_case_types)]
        #vis enum #fields_mod {}
        #[allow(non_upper_case_globals)]
        impl #fields_mod {
            #(#consts)*
        }
        #encoders
    }
}

fn index_module_tokens(model: &EntityModel, index_mod: &Ident) -> TokenStream {
    let ident = &model.base.ident;
    let vis = &model.base.vis;

    let items = model.indexes.iter().map(|decl| {
        let name = decl.ident.unraw().to_string();
        let keys = decl.keys.iter().map(|key| {
            let path = key_path_expr(model, key);
            let kind = key_kind_tokens(key.kind);
            quote! { ::urva::IndexKey { path: #path, kind: #kind } }
        });
        let unique = decl.unique;
        let sparse = decl.sparse;
        let hidden = decl.hidden;
        let ttl = option_tokens(decl.ttl.map(|(secs, _)| quote! { #secs }));
        let ref_ident = &decl.ident;
        let span = ref_ident.span();
        quote_spanned! {span=>
            pub const #ref_ident: ::urva::IndexRef<#ident> = {
                const SPEC: ::urva::IndexSpec = ::urva::IndexSpec {
                    name: #name,
                    keys: &[#(#keys),*],
                    unique: #unique,
                    sparse: #sparse,
                    hidden: #hidden,
                    ttl_seconds: #ttl,
                    ..::urva::IndexSpec::DEFAULT
                };
                ::urva::__private::index_ref_from_spec(&SPEC)
            };
        }
    });

    quote! {
        #[allow(non_camel_case_types)]
        #vis enum #index_mod {}
        #[allow(non_upper_case_globals)]
        impl #index_mod {
            #(#items)*
        }
    }
}

fn key_path_expr(model: &EntityModel, key: &KeyDecl) -> TokenStream {
    stored_path_expr(model, &key.segments, key.kind == KeyKindDecl::FieldWildcard)
}

fn stored_path_expr(
    model: &EntityModel,
    segments: &[Ident],
    trailing_wildcard: bool,
) -> TokenStream {
    let first = &segments[0];
    if first.unraw() == "_id" && !trailing_wildcard {
        return quote! { "_id" };
    }
    let field = model
        .base
        .fields
        .iter()
        .find(|f| same_ident(&f.ident, first))
        .expect("validated: field exists");
    let path = if trailing_wildcard {
        format!("{}.$**", field.stored_name)
    } else {
        field.stored_name.clone()
    };
    quote! { #path }
}

fn option_tokens(value: Option<TokenStream>) -> TokenStream {
    match value {
        Some(v) => quote! { ::core::option::Option::Some(#v) },
        None => quote! { ::core::option::Option::None },
    }
}

fn key_kind_tokens(kind: KeyKindDecl) -> TokenStream {
    match kind {
        KeyKindDecl::Asc => quote! { ::urva::KeyKind::Asc },
        KeyKindDecl::Desc => quote! { ::urva::KeyKind::Desc },
        KeyKindDecl::Text => quote! { ::urva::KeyKind::Text },
        KeyKindDecl::Hashed => quote! { ::urva::KeyKind::Hashed },
        KeyKindDecl::TwoDSphere => quote! { ::urva::KeyKind::TwoDSphere },
        KeyKindDecl::TwoD => quote! { ::urva::KeyKind::TwoD },
        KeyKindDecl::FieldWildcard => quote! { ::urva::KeyKind::Wildcard },
    }
}
