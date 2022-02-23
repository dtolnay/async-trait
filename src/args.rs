use proc_macro2::Span;
use syn::parse::{Error, Parse, ParseStream, Result};
use syn::Token;

#[derive(Copy, Clone)]
pub struct Args {
    pub local: bool,
    pub no_box: bool,
}

mod kw {
    syn::custom_keyword!(Send);
    syn::custom_keyword!(no_box);
}

impl Parse for Args {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut args = Args {
            local: false,
            no_box: false,
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
    input.parse::<kw::no_box>()?;
    args.no_box = true;
    Ok(())
}

fn error() -> Error {
    let msg = "expected #[async_trait], #[async_trait(no_box), #[async_trait(?Send)], or #[async_trait(?Send, no_box)]";
    Error::new(Span::call_site(), msg)
}
