use proc_macro2::Span;
use syn::parse::{Error, Parse, ParseStream, Result};
use syn::spanned::Spanned;
use syn::Token;

#[derive(Copy, Clone)]
pub struct Args {
    pub local: bool,
    pub sync: bool,
}

mod kw {
    syn::custom_keyword!(Send);
}

impl Parse for Args {
    fn parse(input: ParseStream) -> Result<Self> {
        match try_parse(input) {
            Ok(args) if input.is_empty() => Ok(args),
            _ => Err(error()),
        }
    }
}

fn try_parse(input: ParseStream) -> Result<Args> {
    let mut send = false;
    let mut sync = false;

    let arg_list: syn::punctuated::Punctuated<syn::TypeParamBound, Token![+]>;
    arg_list = input.parse_terminated(syn::TypeParamBound::parse)?;
    for bound in arg_list.into_iter() {
        let error = || Error::new(bound.span(), r#"only "?Send" and "Sync" are allowed"#);

        let (modifier, path) = match &bound {
            syn::TypeParamBound::Trait(syn::TraitBound { modifier, path, .. }) => (modifier, path),
            _ => return Err(error()),
        };
        let ident = path.get_ident().ok_or_else(error)?;

        match (modifier, ident.to_string().as_ref()) {
            (syn::TraitBoundModifier::Maybe(_), "Send") => send = true,
            (syn::TraitBoundModifier::None, "Sync") => sync = true,
            _ => return Err(error()),
        }
    }
    Ok(Args { local: send, sync })
}

fn error() -> Error {
    let msg = "expected #[async_trait], #[async_trait(?Send)]], #[async_trait(?Send, Sync)] or #[async_trait(Sync)]";
    Error::new(Span::call_site(), msg)
}
