use proc_macro2::Span;
use syn::ext::IdentExt;
use syn::parse::{Parse, ParseStream};
use syn::{Ident, Lit, LitStr, Token, parenthesized};

const MISSING_IDENT: &str = "missing the index name. Write `#[index(<name>, keys(...))]`";
const KEY_KINDS: &str = "expected one of `1`, `-1`, `text`, `hashed`, `2dsphere`, `2d`, `wildcard`";

#[derive(Clone)]
pub struct IndexDecl {
    pub ident: Ident,
    pub keys: Vec<KeyDecl>,
    pub unique: bool,
    pub sparse: bool,
    pub hidden: bool,
    pub ttl: Option<(u64, Span)>,
    pub partial: Option<PartialDecl>,
    pub weights: Vec<(Ident, i32)>,
    pub collation: Option<CollationDecl>,
    pub wildcard_projection: Option<LitStr>,
    pub bits: Option<(u32, Span)>,
    pub min: Option<(f64, Span)>,
    pub max: Option<(f64, Span)>,
}

#[derive(Clone)]
pub struct CollationDecl {
    pub locale: String,
    pub strength: Option<&'static str>,
}

#[derive(Clone)]
pub enum PartialDecl {
    Shorthand { field: Ident, value: Lit },
    Raw(LitStr),
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
    WholeDocWildcard,
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
            ttl: None,
            partial: None,
            weights: Vec::new(),
            collation: None,
            wildcard_projection: None,
            bits: None,
            min: None,
            max: None,
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
        .map_err(|_| syn::Error::new(input.span(), "expected a field name or `wildcard`"))?;
    let span = field.span();
    if field == "wildcard" && !input.peek(Token![=]) {
        return Ok(KeyDecl {
            segments: Vec::new(),
            kind: KeyKindDecl::WholeDocWildcard,
            span,
        });
    }
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
        "ttl" => {
            input.parse::<Token![=]>()?;
            let lit: syn::LitInt = input.parse()?;
            let seconds: u64 = lit.base10_parse()?;
            decl.ttl = Some((seconds, name.span()));
        }
        "partial" => {
            let content;
            parenthesized!(content in input);
            let field: Ident = content.parse()?;
            if content.peek(Token![.]) {
                return Err(syn::Error::new(
                    field.span(),
                    "`partial(...)` takes a direct field. Use `partial_raw = \"...\"` for a dotted path",
                ));
            }
            content.parse::<Token![=]>()?;
            let negative = content.parse::<Option<Token![-]>>()?.is_some();
            let value: Lit = content.parse()?;
            let value = match value {
                Lit::Int(i) if negative => Lit::Int(syn::LitInt::new(
                    &format!("-{}", i.base10_digits()),
                    i.span(),
                )),
                Lit::Float(f) if negative => Lit::Float(syn::LitFloat::new(
                    &format!("-{}", f.base10_digits()),
                    f.span(),
                )),
                other if negative => {
                    return Err(syn::Error::new(other.span(), "`-` applies to numbers only"));
                }
                other => other,
            };
            if !content.is_empty() {
                return Err(syn::Error::new(
                    content.span(),
                    "`partial(...)` takes exactly one `<field> = <literal>`. Use `partial_raw` for other filters",
                ));
            }
            decl.partial = Some(PartialDecl::Shorthand { field, value });
        }
        "partial_raw" => {
            input.parse::<Token![=]>()?;
            let lit: LitStr = input.parse()?;
            decl.partial = Some(PartialDecl::Raw(lit));
        }
        "weights" => {
            let content;
            parenthesized!(content in input);
            decl.weights.clear();
            loop {
                if content.is_empty() {
                    break;
                }
                let field: Ident = content.parse()?;
                content.parse::<Token![=]>()?;
                let weight: syn::LitInt = content.parse()?;
                if decl
                    .weights
                    .iter()
                    .any(|(seen, _)| seen.unraw() == field.unraw())
                {
                    return Err(syn::Error::new(
                        field.span(),
                        format!("duplicate weight for `{}`", field.unraw()),
                    ));
                }
                let value: i32 = weight.base10_parse()?;
                decl.weights.push((field, value));
                if content.is_empty() {
                    break;
                }
                content.parse::<Token![,]>()?;
            }
            if decl.weights.is_empty() {
                return Err(syn::Error::new(
                    name.span(),
                    "weights(...) takes at least one `<field> = <n>`",
                ));
            }
        }
        "collation" => {
            let content;
            parenthesized!(content in input);
            let mut locale: Option<String> = None;
            let mut strength: Option<(&'static str, Span)> = None;
            loop {
                if content.is_empty() {
                    break;
                }
                let key: Ident = content.parse()?;
                content.parse::<Token![=]>()?;
                match key.to_string().as_str() {
                    "locale" => {
                        let lit: LitStr = content.parse()?;
                        locale = Some(lit.value());
                    }
                    "strength" => {
                        let lit: syn::LitInt = content.parse()?;
                        let variant = match lit.base10_digits() {
                            "1" => "Primary",
                            "2" => "Secondary",
                            "3" => "Tertiary",
                            "4" => "Quaternary",
                            "5" => "Identical",
                            _ => {
                                return Err(syn::Error::new(
                                    lit.span(),
                                    "collation strength must be between 1 and 5",
                                ));
                            }
                        };
                        strength = Some((variant, lit.span()));
                    }
                    other => {
                        return Err(syn::Error::new(
                            key.span(),
                            format!(
                                "unknown collation option `{other}`. Expected `locale` or `strength`"
                            ),
                        ));
                    }
                }
                if content.is_empty() {
                    break;
                }
                content.parse::<Token![,]>()?;
            }
            let locale = locale.ok_or_else(|| {
                syn::Error::new(name.span(), "collation(...) requires `locale = \"...\"`")
            })?;
            if locale == "simple"
                && let Some((_, span)) = strength
            {
                return Err(syn::Error::new(
                    span,
                    "`strength` has no effect with the `simple` locale. Remove `strength`, or pick a locale",
                ));
            }
            decl.collation = Some(CollationDecl {
                locale,
                strength: strength.map(|(value, _)| value),
            });
        }
        "wildcard_projection" => {
            input.parse::<Token![=]>()?;
            decl.wildcard_projection = Some(input.parse()?);
        }
        "bits" => {
            input.parse::<Token![=]>()?;
            let lit: syn::LitInt = input.parse()?;
            let value: u32 = lit.base10_parse()?;
            decl.bits = Some((value, name.span()));
        }
        "min" => {
            input.parse::<Token![=]>()?;
            decl.min = Some((parse_signed_number(input)?, name.span()));
        }
        "max" => {
            input.parse::<Token![=]>()?;
            decl.max = Some((parse_signed_number(input)?, name.span()));
        }
        other => {
            return Err(syn::Error::new(
                name.span(),
                format!("unknown index option `{other}`"),
            ));
        }
    }
    Ok(())
}

fn parse_signed_number(input: ParseStream) -> syn::Result<f64> {
    let negative = input.parse::<Option<Token![-]>>()?.is_some();
    let value: f64 = if input.peek(syn::LitFloat) {
        input.parse::<syn::LitFloat>()?.base10_parse()?
    } else {
        input.parse::<syn::LitInt>()?.base10_parse()?
    };
    Ok(if negative { -value } else { value })
}
