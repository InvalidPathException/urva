use proc_macro2::TokenStream;
use quote::{format_ident, quote, quote_spanned};
use syn::Ident;
use syn::ext::IdentExt;

use crate::case::to_snake;
use crate::entity::{
    EntityModel, FieldModel, StructModel, dotted_key_base_type, field_name_words, same_ident,
    token_type,
};
use crate::index_attr::{IndexDecl, KeyDecl, KeyKindDecl, PartialDecl};
use serde_json::Value as Json;

pub fn entity_tokens(model: &EntityModel) -> syn::Result<TokenStream> {
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
    let witnesses = witness_impl(base);
    let index_module = index_module_tokens(model, &index_mod)?;

    let collection = &model.collection;
    let id_ty = &model.id_ty;
    let spec_refs = model.indexes.iter().map(|decl| {
        let r = &decl.ident;
        quote! { #index_mod::#r.spec() }
    });
    let external_names: Vec<String> = model
        .external_indexes
        .iter()
        .map(|e| e.server_name())
        .collect();

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
            let first = key.segments.first()?;
            let field = base.fields.iter().find(|f| same_ident(&f.ident, first))?;
            let leaf_ty = match nested_hops(field, &key.segments, |_| ttl_span).pop() {
                None => declared_or_encoded(field),
                Some((hop, span)) => quote_spanned! {span=> #hop::Declared },
            };
            Some(quote_spanned! {ttl_span=>
                const _: () = {
                    fn ttl_key_must_be_a_date<T: ::urva::Accepts<::urva::TtlKey>>() {}
                    let _ = ttl_key_must_be_a_date::<#leaf_ty>;
                };
            })
        })
    });
    let ttl_asserts: TokenStream = ttl_asserts.collect();

    Ok(quote! {
        #fields_module

        #witnesses

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
            const EXTERNAL_INDEX_NAMES: &'static [&'static str] = &[#(#external_names),*];
        }

        #[automatically_derived]
        impl #marker for #ident {}

        #ttl_asserts
        };
    })
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
    let witnesses = witness_impl(base);
    quote! {
        #[automatically_derived]
        impl ::urva::__private::Sealed for #ident {}

        #[automatically_derived]
        impl ::urva::Embedded for #ident {}

        #fields_module

        #witnesses
    }
}

fn witness_impl(base: &StructModel) -> TokenStream {
    let ident = &base.ident;
    let nested = base
        .fields
        .iter()
        .filter(|field| field.unstorable.is_none())
        .map(|field| {
            let marker = field_name_marker(&field.ident);
            let stored = &field.stored_name;
            let peeled = dotted_key_base_type(&field.ty);
            let declared = declared_or_encoded(field);
            quote! {
                #[automatically_derived]
                impl ::urva::__private::NestedField<#marker> for #ident {
                    const STORED: &'static str = #stored;
                    type Ty = #peeled;
                    type Declared = #declared;
                }
            }
        });
    quote! { #(#nested)* }
}

fn field_name_marker(field: &Ident) -> TokenStream {
    field_name_words(field).into_iter().rev().fold(
        quote! { ::urva::__private::End },
        |rest, word| {
            quote! { ::urva::__private::Seg<#word, #rest> }
        },
    )
}

fn respan(tokens: TokenStream, span: proc_macro2::Span) -> TokenStream {
    tokens
        .into_iter()
        .map(|mut tree| {
            if let proc_macro2::TokenTree::Group(group) = &tree {
                let mut regrouped =
                    proc_macro2::Group::new(group.delimiter(), respan(group.stream(), span));
                regrouped.set_span(span);
                tree = proc_macro2::TokenTree::Group(regrouped);
            } else {
                tree.set_span(span);
            }
            tree
        })
        .collect()
}

fn nested_hops(
    field: &FieldModel,
    segments: &[Ident],
    span_of: impl Fn(&Ident) -> proc_macro2::Span,
) -> Vec<(TokenStream, proc_macro2::Span)> {
    let base_ty = dotted_key_base_type(&field.ty);
    let mut hop_ty = quote! { #base_ty };
    segments[1..]
        .iter()
        .map(|segment| {
            let marker = field_name_marker(segment);
            let span = span_of(segment);
            let self_ty = respan(hop_ty.clone(), span);
            let hop =
                quote_spanned! {span=> <#self_ty as ::urva::__private::NestedField<#marker>> };
            hop_ty = quote! { #hop::Ty };
            (hop, span)
        })
        .collect()
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

fn index_module_tokens(model: &EntityModel, index_mod: &Ident) -> syn::Result<TokenStream> {
    let ident = &model.base.ident;
    let vis = &model.base.vis;

    let mut items = TokenStream::new();
    for decl in &model.indexes {
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
        let partial = partial_tokens(model, decl)?;
        let weights = decl.weights.iter().map(|(segments, weight)| {
            let stored = stored_path_expr(model, segments, false);
            quote! { ::urva::Weight { field: #stored, weight: #weight } }
        });
        let collation = option_tokens(decl.collation.as_ref().map(|c| {
            let locale = &c.locale;
            let strength = option_tokens(c.strength.map(|s| {
                let variant = format_ident!("{s}");
                quote! { ::urva::mongodb::options::CollationStrength::#variant }
            }));
            quote! { ::urva::CollationSpec { locale: #locale, strength: #strength } }
        }));
        let wildcard_projection = match &decl.wildcard_projection {
            Some(lit) => {
                let tree = json_tokens(&parse_json_object(lit, "wildcard_projection")?);
                quote! { ::core::option::Option::Some(#tree) }
            }
            None => quote! { ::core::option::Option::None },
        };
        let bits = option_tokens(decl.bits.map(|(b, _)| quote! { #b }));
        let min = option_tokens(decl.min.map(|(m, _)| quote! { #m }));
        let max = option_tokens(decl.max.map(|(m, _)| quote! { #m }));
        let ref_ident = &decl.ident;
        let span = ref_ident.span();
        items.extend(quote_spanned! {span=>
            pub const #ref_ident: ::urva::IndexRef<#ident> = {
                const SPEC: ::urva::IndexSpec = ::urva::IndexSpec {
                    name: #name,
                    keys: &[#(#keys),*],
                    unique: #unique,
                    sparse: #sparse,
                    hidden: #hidden,
                    ttl_seconds: #ttl,
                    partial: #partial,
                    weights: &[#(#weights),*],
                    collation: #collation,
                    wildcard_projection: #wildcard_projection,
                    bits: #bits,
                    min: #min,
                    max: #max,
                };
                ::urva::__private::index_ref_from_spec(&SPEC)
            };
        });
    }

    for external in &model.external_indexes {
        let name = external.server_name();
        let external = &external.ident;
        let span = external.span();
        items.extend(quote_spanned! {span=>
            pub const #external: ::urva::IndexRef<#ident> = {
                const SPEC: ::urva::IndexSpec = ::urva::IndexSpec {
                    name: #name,
                    ..::urva::IndexSpec::DEFAULT
                };
                ::urva::__private::index_ref_from_spec(&SPEC)
            };
        });
    }

    Ok(quote! {
        #[allow(non_camel_case_types)]
        #vis enum #index_mod {}
        #[allow(non_upper_case_globals)]
        impl #index_mod {
            #items
        }
    })
}

fn partial_tokens(model: &EntityModel, decl: &IndexDecl) -> syn::Result<TokenStream> {
    match &decl.partial {
        None => Ok(quote! { ::core::option::Option::None }),
        Some(PartialDecl::Shorthand { field, value }) => {
            let stored = stored_name_of(model, field);
            let value = lit_to_const_bson(value)?;
            Ok(quote! {
                ::core::option::Option::Some(::urva::ConstBson::Doc(&[(
                    #stored,
                    ::urva::ConstBson::Doc(&[("$eq", #value)]),
                )]))
            })
        }
        Some(PartialDecl::Raw(lit)) => {
            let tree = json_tokens(&parse_json_object(lit, "partial_raw")?);
            Ok(quote! { ::core::option::Option::Some(#tree) })
        }
    }
}

fn stored_name_of(model: &EntityModel, field: &Ident) -> String {
    model
        .base
        .fields
        .iter()
        .find(|f| same_ident(&f.ident, field))
        .map(|f| f.stored_name.clone())
        .unwrap_or_else(|| field.unraw().to_string())
}

fn lit_to_const_bson(lit: &syn::Lit) -> syn::Result<TokenStream> {
    Ok(match lit {
        syn::Lit::Str(s) => {
            let v = s.value();
            quote! { ::urva::ConstBson::Str(#v) }
        }
        syn::Lit::Int(i) => {
            let v: i64 = i.base10_parse()?;
            quote! { ::urva::ConstBson::I64(#v) }
        }
        syn::Lit::Float(f) => {
            let v: f64 = f.base10_parse()?;
            quote! { ::urva::ConstBson::F64(#v) }
        }
        syn::Lit::Bool(b) => {
            let v = b.value();
            quote! { ::urva::ConstBson::Bool(#v) }
        }
        other => {
            return Err(syn::Error::new(
                other.span(),
                "partial(...) literals may be strings, integers, floats, or booleans",
            ));
        }
    })
}

fn parse_json_object(lit: &syn::LitStr, option: &str) -> syn::Result<Json> {
    let parsed: Json = serde_json::from_str(&lit.value())
        .map_err(|e| syn::Error::new(lit.span(), format!("invalid JSON: {e}")))?;
    match parsed {
        Json::Object(_) => Ok(parsed),
        _ => Err(syn::Error::new(
            lit.span(),
            format!("{option} must be a JSON object (a document, `{{...}}`)"),
        )),
    }
}

fn json_tokens(value: &Json) -> TokenStream {
    match value {
        Json::Null => quote! { ::urva::ConstBson::Null },
        Json::Bool(b) => quote! { ::urva::ConstBson::Bool(#b) },
        Json::Number(n) => match n.as_i64() {
            Some(i) => quote! { ::urva::ConstBson::I64(#i) },
            None => {
                let f = n.as_f64().unwrap_or(f64::NAN);
                quote! { ::urva::ConstBson::F64(#f) }
            }
        },
        Json::String(s) => quote! { ::urva::ConstBson::Str(#s) },
        Json::Array(items) => {
            let items = items.iter().map(json_tokens);
            quote! { ::urva::ConstBson::Arr(&[#(#items),*]) }
        }
        Json::Object(entries) => {
            let entries = entries.iter().map(|(k, v)| {
                let v = json_tokens(v);
                quote! { (#k, #v) }
            });
            quote! { ::urva::ConstBson::Doc(&[#(#entries),*]) }
        }
    }
}

fn key_path_expr(model: &EntityModel, key: &KeyDecl) -> TokenStream {
    if key.kind == KeyKindDecl::WholeDocWildcard {
        return quote! { "$**" };
    }
    stored_path_expr(model, &key.segments, key.kind == KeyKindDecl::FieldWildcard)
}

fn stored_path_expr(
    model: &EntityModel,
    segments: &[Ident],
    trailing_wildcard: bool,
) -> TokenStream {
    let first = &segments[0];
    if segments.len() == 1 && first.unraw() == "_id" && !trailing_wildcard {
        return quote! { "_id" };
    }
    let field = model
        .base
        .fields
        .iter()
        .find(|f| same_ident(&f.ident, first))
        .expect("validated: field exists");
    let stored_first = &field.stored_name;

    if segments.len() == 1 {
        let path = if trailing_wildcard {
            format!("{stored_first}.$**")
        } else {
            stored_first.clone()
        };
        return quote! { #path };
    }

    let mut parts = vec![quote! { #stored_first }];
    for (hop, span) in nested_hops(field, segments, Ident::span) {
        parts.push(quote_spanned! {span=> #hop::STORED });
    }
    if trailing_wildcard {
        parts.push(quote! { "$**" });
    }

    quote! {
        {
            const PARTS: &[&str] = &[#(#parts),*];
            const LEN: usize = {
                let mut __urva_n = 0usize;
                let mut __urva_i = 0;
                while __urva_i < PARTS.len() {
                    __urva_n += PARTS[__urva_i].len();
                    __urva_i += 1;
                }
                __urva_n + PARTS.len() - 1
            };
            const BYTES: [u8; LEN] = {
                let mut __urva_buf = [0u8; LEN];
                let mut __urva_pos = 0;
                let mut __urva_i = 0;
                while __urva_i < PARTS.len() {
                    if __urva_i > 0 {
                        __urva_buf[__urva_pos] = b'.';
                        __urva_pos += 1;
                    }
                    let __urva_part = PARTS[__urva_i].as_bytes();
                    let mut __urva_j = 0;
                    while __urva_j < __urva_part.len() {
                        __urva_buf[__urva_pos] = __urva_part[__urva_j];
                        __urva_pos += 1;
                        __urva_j += 1;
                    }
                    __urva_i += 1;
                }
                __urva_buf
            };
            match ::core::str::from_utf8(&BYTES) {
                ::core::result::Result::Ok(__urva_s) => __urva_s,
                ::core::result::Result::Err(_) => ::core::panic!("unreachable: UTF-8 concat"),
            }
        }
    }
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
        KeyKindDecl::FieldWildcard | KeyKindDecl::WholeDocWildcard => {
            quote! { ::urva::KeyKind::Wildcard }
        }
    }
}
