# async-trait-unboxed

[![Cargo](https://img.shields.io/crates/v/async-trait-unboxed?style=flat-square)](https://crates.io/crates/async-trait-unboxed)
![Crates.io](https://img.shields.io/crates/l/async-trait-unboxed?style=flat-square)

This is a fork of the widely acclaimed [async-trait](https://github.com/dtolnay/async-trait) crate. This crate adds two experimental attributes to [async-trait](https://github.com/dtolnay/async-trait) that can be applied to asynchronous trait methods and associated functions to avoid heap memory allocation.

* The return type of an asynchronous trait method or associated function with the `unboxed` or `unboxed_simple` attribute is of `impl Future<..>` instead of `Box<dyn Future<..>>`.
* *Those attributes have a lot of limitations when used in a large software product due to known bugs in Rust generic associated types: e.g., [95719](https://github.com/rust-lang/rust/issues/95719) and [90696](https://github.com/rust-lang/rust/issues/90696).*

Note that, _the main author of [async-trait](https://github.com/dtolnay/async-trait) [did not want to add such options](https://github.com/dtolnay/async-trait/pull/189), therefore this fork will not be merged into the upstream._

See [async-trait](https://github.com/dtolnay/async-trait) for more details about [async-trait](https://github.com/dtolnay/async-trait).

## Examples

`unboxed` turns an asynchronous trait method or associated function into a synchronous one returning an `impl Future<..>` by adding a generic associated type for the method or associated function.

```rust
use async_trait::{async_trait, unboxed_simple};

#[async_trait]
pub trait SelfToUsize {
    #[unboxed]
    async fn get(&self) -> usize;
}

#[async_trait]
impl SelfToUsize for u32 {
    #[unboxed]
    async fn get(&self) -> usize {
        *self as usize
    }
}
```

The above code produces the following code.

```rust
pub trait SelfToUsize {
    fn get<'life0, 'async_trait>(&'life0 self) -> Self::RetTypeOfGet<'life0, 'async_trait>
    where
        'life0: 'async_trait,
        Self: 'async_trait;
    type RetTypeOfGet<'life0, 'async_trait>: ::core::future::Future<Output = usize>
        + ::core::marker::Send
        + 'async_trait
    where
        'life0: 'async_trait,
        Self: 'async_trait,
        Self: 'life0;
}
impl SelfToUsize for u32 {
    fn get<'life0, 'async_trait>(&'life0 self) -> Self::RetTypeOfGet<'life0, 'async_trait>
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        async move {
            if let ::core::option::Option::Some(__ret) = ::core::option::Option::None::<usize> {
                return __ret;
            }
            let __self = self;
            let __ret: usize = { *__self as usize };
            #[allow(unreachable_code)]
            __ret
        }
    }
    type RetTypeOfGet<'life0, 'async_trait> = impl ::core::future::Future<Output = usize>
        + ::core::marker::Send
        + 'async_trait
    where
        'life0: 'async_trait,
        Self: 'async_trait,
        Self: 'life0 ;
}
```

`unboxed_simple` is identical to `unboxed` except that it substitutes all the lifetime bounds and parameters with a single, fixed lifetime: `'async_trait`. When code around an `unboxed` attribute does not compile, `unboxed_simple` _might_ help.

```rust
use async_trait::{async_trait, unboxed_simple};

#[async_trait]
pub trait AddOther {
    #[unboxed_simple]
    async fn add<'s, 'o>(&'a self, other: &'o usize) -> usize;
}

#[async_trait]
impl AddOther for u32 {
    #[unboxed_simple]
    async fn add<'s, 'o>(&'a self, other: &'o usize) -> usize {
        (*self as usize) + *other
    }
}

```

The above code produces the following code; all the lifetime parameters are replaced with `'async_trait`.

```rust
pub trait AddOther {
    fn add<'async_trait>(
        &'async_trait self,
        other: &'async_trait usize,
    ) -> Self::RetTypeOfAdd<'async_trait>
    where
        Self: 'async_trait;
    type RetTypeOfAdd<'async_trait>: ::core::future::Future<Output = usize>
        + ::core::marker::Send
        + 'async_trait
    where
        Self: 'async_trait,
        Self: 'async_trait;
}

impl AddOther for u32 {
    fn add<'async_trait>(
        &'async_trait self,
        other: &'async_trait usize,
    ) -> Self::RetTypeOfAdd<'async_trait>
    where
        Self: 'async_trait,
    {
        async move {
            if let ::core::option::Option::Some(__ret) = ::core::option::Option::None::<usize> {
                return __ret;
            }
            let __self = self;
            let other = other;
            let __ret: usize = { (*__self as usize) + *other };
            #[allow(unreachable_code)]
            __ret
        }
    }
    type RetTypeOfAdd<'async_trait> = impl ::core::future::Future<Output = usize>
        + ::core::marker::Send
        + 'async_trait
    where
        Self: 'async_trait,
        Self: 'async_trait;
}
```
