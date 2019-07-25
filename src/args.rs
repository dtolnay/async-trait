use syn::parse::{Parse, ParseStream, Result};

pub struct Args {
    pub local: bool
}

mod kw {
    syn::custom_keyword!(local);
}

impl Parse for Args {
    fn parse(input: ParseStream) -> Result<Self> {
        let local: Option<kw::local> = input.parse()?;
        Ok(Args {
            local: local.is_some(),
        })
    }
}
