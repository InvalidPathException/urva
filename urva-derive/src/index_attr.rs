use proc_macro2::Span;
use syn::parse::{Parse, ParseStream};
use syn::{Ident, Token, parenthesized};

const MISSING_IDENT: &str = "missing the index name. Write `#[index(<name>, keys(...))]`";
const KEY_KINDS: &str = "expected one of `1`, `-1`, `text`, `hashed`, `2dsphere`, `2d`, `wildcard`";

#[derive(Clone)]
pub struct IndexDecl {
    pub ident: Ident,
    pub keys: Vec<KeyDecl>,
    pub unique: bool,
    pub sparse: bool,
    pub hidden: bool,
}

#[derive(Clone)]
pub struct KeyDecl {
    pub segments: Vec<Ident>,
    pub kind: KeyKindDecl,
    pub span: Span,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum KeyKindDecl {
    Asc,
    Desc,
    Text,
    Hashed,
    TwoDSphere,
    TwoD,
    FieldWildcard,
}

impl Parse for IndexDecl {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if !input.peek(Ident) {
            return Err(syn::Error::new(input.span(), MISSING_IDENT));
        }
        let ident: Ident = input.parse()?;
        if ident == "keys" && input.peek(syn::token::Paren) {
            return Err(syn::Error::new(ident.span(), MISSING_IDENT));
        }

        input.parse::<Token![,]>().map_err(|_| {
            syn::Error::new(ident.span(), "expected `, keys(...)` after the index name")
        })?;

        let keys_kw: Ident = input
            .parse()
            .map_err(|_| syn::Error::new(input.span(), "expected `keys(...)`"))?;
        if keys_kw != "keys" {
            return Err(syn::Error::new(keys_kw.span(), "expected `keys(...)`"));
        }
        let keys_content;
        parenthesized!(keys_content in input);
        let mut keys = Vec::new();
        loop {
            if keys_content.is_empty() {
                break;
            }
            keys.push(parse_key(&keys_content)?);
            if keys_content.is_empty() {
                break;
            }
            keys_content.parse::<Token![,]>()?;
        }
        let mut decl = IndexDecl {
            ident,
            keys,
            unique: false,
            sparse: false,
            hidden: false,
        };

        while !input.is_empty() {
            input.parse::<Token![,]>()?;
            if input.is_empty() {
                break;
            }
            parse_option(input, &mut decl)?;
        }
        Ok(decl)
    }
}

fn parse_key(input: ParseStream) -> syn::Result<KeyDecl> {
    let field: Ident = input
        .parse()
        .map_err(|_| syn::Error::new(input.span(), "expected a field name"))?;
    let span = field.span();
    let kind = if input.peek(Token![=]) {
        input.parse::<Token![=]>()?;
        parse_key_kind(input)?
    } else {
        KeyKindDecl::Asc
    };
    Ok(KeyDecl {
        segments: vec![field],
        kind,
        span,
    })
}

fn parse_key_kind(input: ParseStream) -> syn::Result<KeyKindDecl> {
    let span = input.span();
    if input.peek(Token![-]) {
        input.parse::<Token![-]>()?;
        let lit: syn::LitInt = input.parse()?;
        if lit.base10_digits() == "1" && lit.suffix().is_empty() {
            return Ok(KeyKindDecl::Desc);
        }
        return Err(syn::Error::new(lit.span(), "expected `-1`"));
    }
    if input.peek(syn::LitInt) {
        let lit: syn::LitInt = input.parse()?;
        return match (lit.base10_digits(), lit.suffix()) {
            ("1", "") => Ok(KeyKindDecl::Asc),
            ("2", "dsphere") => Ok(KeyKindDecl::TwoDSphere),
            ("2", "d") => Ok(KeyKindDecl::TwoD),
            _ => Err(syn::Error::new(lit.span(), KEY_KINDS)),
        };
    }
    let ident: Ident = input
        .parse()
        .map_err(|_| syn::Error::new(span, KEY_KINDS))?;
    match ident.to_string().as_str() {
        "text" => Ok(KeyKindDecl::Text),
        "hashed" => Ok(KeyKindDecl::Hashed),
        "wildcard" => Ok(KeyKindDecl::FieldWildcard),
        other => Err(syn::Error::new(
            ident.span(),
            format!("unknown key type `{other}`. {KEY_KINDS}"),
        )),
    }
}

fn parse_option(input: ParseStream, decl: &mut IndexDecl) -> syn::Result<()> {
    let name: Ident = input
        .parse()
        .map_err(|_| syn::Error::new(input.span(), "expected an index option"))?;
    match name.to_string().as_str() {
        "unique" => decl.unique = true,
        "sparse" => decl.sparse = true,
        "hidden" => decl.hidden = true,
        other => {
            return Err(syn::Error::new(
                name.span(),
                format!("unknown index option `{other}`"),
            ));
        }
    }
    Ok(())
}
