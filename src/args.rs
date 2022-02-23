use proc_macro2::Span;
use syn::parse::{Error, Parse, ParseStream, Result};
use syn::Token;

#[derive(Copy, Clone)]
pub struct Args {
    pub local: bool,
    pub impl_future: bool,
}

mod kw {
    syn::custom_keyword!(Send);
    syn::custom_keyword!(impl_future);
}

impl Parse for Args {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut args = Args {
            local: false,
            impl_future: false,
        };
        while !input.is_empty() {
            if try_parse(input, &mut args).is_err() {
                return Err(error());
            }
        }
        Ok(args)
    }
}

fn try_parse(input: ParseStream, args: &mut Args) -> Result<()> {
    if input.peek(Token![?]) {
        input.parse::<Token![?]>()?;
        input.parse::<kw::Send>()?;
        args.local = true;
        return Ok(());
    }
    input.parse::<kw::impl_future>()?;
    args.impl_future = true;
    Ok(())
}

fn error() -> Error {
    let msg = "expected #[async_trait], #[async_trait(impl_future), #[async_trait(?Send)], or #[async_trait(?Send, impl_future)]";
    Error::new(Span::call_site(), msg)
}
