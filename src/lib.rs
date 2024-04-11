//! [![github]](https://github.com/dtolnay/async-trait)&ensp;[![crates-io]](https://crates.io/crates/async-trait)&ensp;[![docs-rs]](https://docs.rs/async-trait)
//!
//! [github]: https://img.shields.io/badge/github-8da0cb?style=for-the-badge&labelColor=555555&logo=github
//! [crates-io]: https://img.shields.io/badge/crates.io-fc8d62?style=for-the-badge&labelColor=555555&logo=rust
//! [docs-rs]: https://img.shields.io/badge/docs.rs-66c2a5?style=for-the-badge&labelColor=555555&logo=docs.rs
//!
//! <br>
//!
//! <h4>Type erasure for async trait methods</h4>
//!
//! The stabilization of async functions in traits in Rust 1.75 did not include
//! support for using traits containing async functions as `dyn Trait`. Trying
//! to use dyn with an async trait produces the following error:
//!
//! ```compile_fail
//! pub trait Trait {
//!     async fn f(&self);
//! }
//!
//! pub fn make() -> Box<dyn Trait> {
//!     unimplemented!()
//! }
//! ```
//!
//! ```text
//! error[E0038]: the trait `Trait` cannot be made into an object
//!  --> src/main.rs:5:22
//!   |
//! 5 | pub fn make() -> Box<dyn Trait> {
//!   |                      ^^^^^^^^^ `Trait` cannot be made into an object
//!   |
//! note: for a trait to be "object safe" it needs to allow building a vtable to allow the call to be resolvable dynamically; for more information visit <https://doc.rust-lang.org/reference/items/traits.html#object-safety>
//!  --> src/main.rs:2:14
//!   |
//! 1 | pub trait Trait {
//!   |           ----- this trait cannot be made into an object...
//! 2 |     async fn f(&self);
//!   |              ^ ...because method `f` is `async`
//!   = help: consider moving `f` to another trait
//! ```
//!
//! This crate provides an attribute macro to make async fn in traits work with
//! dyn traits.
//!
//! Please refer to [*why async fn in traits are hard*][hard] for a deeper
//! analysis of how this implementation differs from what the compiler and
//! language deliver natively.
//!
//! [hard]: https://smallcultfollowing.com/babysteps/blog/2019/10/26/async-fn-in-traits-are-hard/
//!
//! <br>
//!
//! # Example
//!
//! This example implements the core of a highly effective advertising platform
//! using async fn in a trait.
//!
//! The only thing to notice here is that we write an `#[async_trait]` macro on
//! top of traits and trait impls that contain async fn, and then they work. We
//! get to have `Vec<Box<dyn Advertisement + Sync>>` or `&[&dyn Advertisement]`,
//! for example.
//!
//! ```
//! use async_trait::async_trait;
//!
//! #[async_trait]
//! trait Advertisement {
//!     async fn run(&self);
//! }
//!
//! struct Modal;
//!
//! #[async_trait]
//! impl Advertisement for Modal {
//!     async fn run(&self) {
//!         self.render_fullscreen().await;
//!         for _ in 0..4u16 {
//!             remind_user_to_join_mailing_list().await;
//!         }
//!         self.hide_for_now().await;
//!     }
//! }
//!
//! struct AutoplayingVideo {
//!     media_url: String,
//! }
//!
//! #[async_trait]
//! impl Advertisement for AutoplayingVideo {
//!     async fn run(&self) {
//!         let stream = connect(&self.media_url).await;
//!         stream.play().await;
//!
//!         // Video probably persuaded user to join our mailing list!
//!         Modal.run().await;
//!     }
//! }
//! #
//! # impl Modal {
//! #     async fn render_fullscreen(&self) {}
//! #     async fn hide_for_now(&self) {}
//! # }
//! #
//! # async fn remind_user_to_join_mailing_list() {}
//! #
//! # struct Stream;
//! # async fn connect(_media_url: &str) -> Stream { Stream }
//! # impl Stream {
//! #     async fn play(&self) {}
//! # }
//! ```
//!
//! <br><br>
//!
//! # Supported features
//!
//! It is the intention that all features of Rust traits should work nicely with
//! #\[async_trait\], but the edge cases are numerous. Please file an issue if
//! you see unexpected borrow checker errors, type errors, or warnings. There is
//! no use of `unsafe` in the expanded code, so rest assured that if your code
//! compiles it can't be that badly broken.
//!
//! > &#9745;&emsp;Self by value, by reference, by mut reference, or no self;<br>
//! > &#9745;&emsp;Any number of arguments, any return value;<br>
//! > &#9745;&emsp;Generic type parameters and lifetime parameters;<br>
//! > &#9745;&emsp;Associated types;<br>
//! > &#9745;&emsp;Having async and non-async functions in the same trait;<br>
//! > &#9745;&emsp;Default implementations provided by the trait;<br>
//! > &#9745;&emsp;Elided lifetimes.<br>
//!
//! <br>
//!
//! # Explanation
//!
//! Async fns get transformed into methods that return `Pin<Box<dyn Future +
//! Send + 'async_trait>>` and delegate to an async block.
//!
//! For example the `impl Advertisement for AutoplayingVideo` above would be
//! expanded as:
//!
//! ```
//! # const IGNORE: &str = stringify! {
//! impl Advertisement for AutoplayingVideo {
//!     fn run<'async_trait>(
//!         &'async_trait self,
//!     ) -> Pin<Box<dyn core::future::Future<Output = ()> + Send + 'async_trait>>
//!     where
//!         Self: Sync + 'async_trait,
//!     {
//!         Box::pin(async move {
//!             /* the original method body */
//!         })
//!     }
//! }
//! # };
//! ```
//!
//! <br><br>
//!
//! # Non-threadsafe futures
//!
//! Not all async traits need futures that are `dyn Future + Send`. To avoid
//! having Send and Sync bounds placed on the async trait methods, invoke the
//! async trait macro as `#[async_trait(?Send)]` on both the trait and the impl
//! blocks.
//!
//! <br>
//!
//! # Elided lifetimes
//!
//! Be aware that async fn syntax does not allow lifetime elision outside of `&`
//! and `&mut` references. (This is true even when not using #\[async_trait\].)
//! Lifetimes must be named or marked by the placeholder `'_`.
//!
//! Fortunately the compiler is able to diagnose missing lifetimes with a good
//! error message.
//!
//! ```compile_fail
//! # use async_trait::async_trait;
//! #
//! type Elided<'a> = &'a usize;
//!
//! #[async_trait]
//! trait Test {
//!     async fn test(not_okay: Elided, okay: &usize) {}
//! }
//! ```
//!
//! ```text
//! error[E0726]: implicit elided lifetime not allowed here
//!  --> src/main.rs:9:29
//!   |
//! 9 |     async fn test(not_okay: Elided, okay: &usize) {}
//!   |                             ^^^^^^- help: indicate the anonymous lifetime: `<'_>`
//! ```
//!
//! The fix is to name the lifetime or use `'_`.
//!
//! ```
//! # use async_trait::async_trait;
//! #
//! # type Elided<'a> = &'a usize;
//! #
//! #[async_trait]
//! trait Test {
//!     // either
//!     async fn test<'e>(elided: Elided<'e>) {}
//! # }
//! # #[async_trait]
//! # trait Test2 {
//!     // or
//!     async fn test(elided: Elided<'_>) {}
//! }
//! ```
//!
//! <br><br>
//!
//! # Dyn traits
//!
//! Traits with async methods can be used as trait objects as long as they meet
//! the usual requirements for dyn -- no methods with type parameters, no self
//! by value, no associated types, etc.
//!
//! ```
//! # use async_trait::async_trait;
//! #
//! #[async_trait]
//! pub trait ObjectSafe {
//!     async fn f(&self);
//!     async fn g(&mut self);
//! }
//!
//! # const IGNORE: &str = stringify! {
//! impl ObjectSafe for MyType {...}
//!
//! let value: MyType = ...;
//! # };
//! #
//! # struct MyType;
//! #
//! # #[async_trait]
//! # impl ObjectSafe for MyType {
//! #     async fn f(&self) {}
//! #     async fn g(&mut self) {}
//! # }
//! #
//! # let value: MyType = MyType;
//! let object = &value as &dyn ObjectSafe;  // make trait object
//! ```
//!
//! The one wrinkle is in traits that provide default implementations of async
//! methods. In order for the default implementation to produce a future that is
//! Send, the async_trait macro must emit a bound of `Self: Sync` on trait
//! methods that take `&self` and a bound `Self: Send` on trait methods that
//! take `&mut self`. An example of the former is visible in the expanded code
//! in the explanation section above.
//!
//! If you make a trait with async methods that have default implementations,
//! everything will work except that the trait cannot be used as a trait object.
//! Creating a value of type `&dyn Trait` will produce an error that looks like
//! this:
//!
//! ```text
//! error: the trait `Test` cannot be made into an object
//!  --> src/main.rs:8:5
//!   |
//! 8 |     async fn cannot_dyn(&self) {}
//!   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
//! ```
//!
//! For traits that need to be object safe and need to have default
//! implementations for some async methods, there are two resolutions. Either
//! you can add Send and/or Sync as supertraits (Send if there are `&mut self`
//! methods with default implementations, Sync if there are `&self` methods with
//! default implementations) to constrain all implementors of the trait such that
//! the default implementations are applicable to them:
//!
//! ```
//! # use async_trait::async_trait;
//! #
//! #[async_trait]
//! pub trait ObjectSafe: Sync {  // added supertrait
//!     async fn can_dyn(&self) {}
//! }
//! #
//! # struct MyType;
//! #
//! # #[async_trait]
//! # impl ObjectSafe for MyType {}
//! #
//! # let value = MyType;
//!
//! let object = &value as &dyn ObjectSafe;
//! ```
//!
//! or you can strike the problematic methods from your trait object by
//! bounding them with `Self: Sized`:
//!
//! ```
//! # use async_trait::async_trait;
//! #
//! #[async_trait]
//! pub trait ObjectSafe {
//!     async fn cannot_dyn(&self) where Self: Sized {}
//!
//!     // presumably other methods
//! }
//! #
//! # struct MyType;
//! #
//! # #[async_trait]
//! # impl ObjectSafe for MyType {}
//! #
//! # let value = MyType;
//!
//! let object = &value as &dyn ObjectSafe;
//! ```

#![doc(html_root_url = "https://docs.rs/async-trait/0.1.80")]
#![allow(
    clippy::default_trait_access,
    clippy::doc_markdown,
    clippy::explicit_auto_deref,
    clippy::if_not_else,
    clippy::items_after_statements,
    clippy::match_like_matches_macro,
    clippy::module_name_repetitions,
    clippy::shadow_unrelated,
    clippy::similar_names,
    clippy::too_many_lines
)]

extern crate proc_macro;

mod args;
mod bound;
mod expand;
mod lifetime;
mod parse;
mod receiver;
mod verbatim;

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
