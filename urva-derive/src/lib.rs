mod case;
mod codegen;
mod entity;
mod index_attr;

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

#[proc_macro_derive(Entity, attributes(entity, index, external_index))]
pub fn derive_entity(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    entity::parse_entity(&input)
        .and_then(|model| codegen::entity_tokens(&model))
        .unwrap_or_else(|err| err.to_compile_error())
        .into()
}

#[proc_macro_derive(Embedded)]
pub fn derive_embedded(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    entity::parse_struct(&input, "Embedded")
        .map(|model| codegen::embedded_tokens(&model))
        .unwrap_or_else(|err| err.to_compile_error())
        .into()
}
