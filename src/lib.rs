extern crate proc_macro;

mod expand;
mod lifetime;
mod parse;
mod receiver;

use crate::expand::expand;
use crate::parse::{Item, Nothing};
use proc_macro::TokenStream;
use quote::quote;
use syn::parse_macro_input;

#[proc_macro_attribute]
pub fn async_trait(args: TokenStream, input: TokenStream) -> TokenStream {
    parse_macro_input!(args as Nothing);
    let mut item = parse_macro_input!(input as Item);
    expand(&mut item);
    TokenStream::from(quote!(#item))
}
