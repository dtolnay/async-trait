//! [![github]](https://github.com/wvwwvwwv/async-trait-fn)&ensp;[![crates-io]](https://crates.io/crates/async-trait-fn)&ensp;[![docs-rs]](https://docs.rs/async-trait-fn)
//!
//! [github]: https://img.shields.io/badge/github-8da0cb?style=for-the-badge&labelColor=555555&logo=github
//! [crates-io]: https://img.shields.io/badge/crates.io-fc8d62?style=for-the-badge&labelColor=555555&logo=rust
//! [docs-rs]: https://img.shields.io/badge/docs.rs-66c2a5?style=for-the-badge&labelColor=555555&logo=docs.rs
//!
//! # async-trait-fn
//!
//! This is a fork of the widely acclaimed [async-trait](https://github.com/dtolnay/async-trait)
//! crate. This crate adds two experimental attributes to
//! [async-trait](https://github.com/dtolnay/async-trait) that can be applied to asynchronous trait
//! methods and associated functions to avoid heap memory allocation.
//!
//! ### `unboxed`
//!
//! An `async fn` without a default implementation may get transformed into a
//! method that returns `impl Future + Send + 'async_trait` when
//! `#[macro@unboxed]` is marked on both the trait and the impl blocks.
//! `#[macro@unboxed]` requires the following unstable language features:
//! `associated_type_bounds` and `type_alias_impl_trait`.
//!
//! ```ignore
//! #![feature(associated_type_bounds, type_alias_impl_trait)]
//! # use async_trait_fn::async_trait;
//!
//! #[async_trait]
//! pub trait MyFastTrait {
//!     /// `cnt_fast` returns an instance of a concrete `Future` type.
//!     #[unboxed]
//!     async fn cnt_fast(&self) -> usize;
//!
//!     // presumably other methods
//! }
//!
//! struct MyType(usize);
//!
//! #[async_trait]
//! impl MyFastTrait for MyType {
//!     #[unboxed]
//!     async fn cnt_fast(&self) -> usize {
//!         self.0
//!     }
//! }
//!
//! let value = MyType(1);
//! let unboxed_future = value.cnt_fast();
//! ```
//!
//! The feature is not generally applicable due to a
//! [bug](https://github.com/rust-lang/rust/issues/95719) in the Rust type
//! system.
//!
//! ### `unboxed_simple`
//!
//! `unboxed_simple` is identical to `unboxed` except that all the lifetime
//! bounds in the type and parameters are substituted with a single lifetime.

#![allow(
    clippy::default_trait_access,
    clippy::doc_markdown,
    clippy::explicit_auto_deref,
    clippy::if_not_else,
    clippy::items_after_statements,
    clippy::module_name_repetitions,
    clippy::shadow_unrelated,
    clippy::similar_names,
    clippy::too_many_lines
)]

extern crate proc_macro;

mod args;
mod expand;
mod lifetime;
mod parse;
mod receiver;

use crate::args::Args;
use crate::expand::expand;
use crate::parse::Item;
use proc_macro::TokenStream;
use quote::quote;
use syn::parse_macro_input;

#[proc_macro_attribute]
pub fn async_trait(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as Args);
    let mut item = parse_macro_input!(input as Item);
    expand(&mut item, args.local);
    TokenStream::from(quote!(#item))
}

#[proc_macro_attribute]
pub fn unboxed(_args: TokenStream, input: TokenStream) -> TokenStream {
    input
}

#[proc_macro_attribute]
pub fn unboxed_simple(_args: TokenStream, input: TokenStream) -> TokenStream {
    input
}
