#![cfg_attr(async_trait_nightly_testing, feature(specialization))]

use async_trait::async_trait;

pub mod executor;

// Dummy module to check that the expansion refer to rust's core crate
mod core {}

#[async_trait]
trait Trait {
    type Assoc;

    async fn selfvalue(self)
    where
        Self: Sized,
    {
    }

    async fn selfref(&self) {}

    async fn selfmut(&mut self) {}

    async fn required() -> Self::Assoc;

    async fn elided_lifetime(_x: &str) {}

    async fn explicit_lifetime<'a>(_x: &'a str) {}

    async fn generic_type_param<T: Send>(x: Box<T>) -> T {
        *x
    }

    async fn calls(&self) {
        self.selfref().await;
        Self::elided_lifetime("").await;
        <Self>::elided_lifetime("").await;
    }

    async fn calls_mut(&mut self) {
        self.selfmut().await;
    }
}

struct Struct;

#[async_trait]
impl Trait for Struct {
    type Assoc = ();

    async fn selfvalue(self) {}

    async fn selfref(&self) {}

    async fn selfmut(&mut self) {}

    async fn required() -> Self::Assoc {}

    async fn elided_lifetime(_x: &str) {}

    async fn explicit_lifetime<'a>(_x: &'a str) {}

    async fn generic_type_param<T: Send>(x: Box<T>) -> T {
        *x
    }

    async fn calls(&self) {
        self.selfref().await;
        Self::elided_lifetime("").await;
        <Self>::elided_lifetime("").await;
    }

    async fn calls_mut(&mut self) {
        self.selfmut().await;
    }
}

pub async fn test() {
    let mut s = Struct;
    s.selfref().await;
    s.selfmut().await;
    s.selfvalue().await;

    Struct::required().await;
    Struct::elided_lifetime("").await;
    Struct::explicit_lifetime("").await;
    Struct::generic_type_param(Box::new("")).await;

    let mut s = Struct;
    s.calls().await;
    s.calls_mut().await;
}

pub async fn test_object_safe_without_default() {
    #[async_trait]
    trait ObjectSafe {
        async fn f(&self);
    }

    #[async_trait]
    impl ObjectSafe for Struct {
        async fn f(&self) {}
    }

    let object = &Struct as &dyn ObjectSafe;
    object.f().await;
}

pub async fn test_object_safe_with_default() {
    #[async_trait]
    trait ObjectSafe: Sync {
        async fn f(&self) {}
    }

    #[async_trait]
    impl ObjectSafe for Struct {
        async fn f(&self) {}
    }

    let object = &Struct as &dyn ObjectSafe;
    object.f().await;
}

pub async fn test_object_no_send() {
    #[async_trait(?Send)]
    trait ObjectSafe: Sync {
        async fn f(&self) {}
    }

    #[async_trait(?Send)]
    impl ObjectSafe for Struct {
        async fn f(&self) {}
    }

    let object = &Struct as &dyn ObjectSafe;
    object.f().await;
}

// https://github.com/dtolnay/async-trait/issues/1
pub mod issue1 {
    use async_trait::async_trait;

    #[async_trait]
    trait Issue1 {
        async fn f<U>(&self);
    }

    #[async_trait]
    impl<T: Sync> Issue1 for Vec<T> {
        async fn f<U>(&self) {}
    }
}

// https://github.com/dtolnay/async-trait/issues/2
pub mod issue2 {
    use async_trait::async_trait;
    use std::future::Future;

    #[async_trait]
    pub trait Issue2: Future {
        async fn flatten(self) -> <Self::Output as Future>::Output
        where
            Self::Output: Future + Send,
            Self: Sized,
        {
            let nested_future = self.await;
            nested_future.await
        }
    }
}

// https://github.com/dtolnay/async-trait/issues/9
pub mod issue9 {
    use async_trait::async_trait;

    #[async_trait]
    pub trait Issue9: Sized + Send {
        async fn f(_x: Self) {}
    }
}

// https://github.com/dtolnay/async-trait/issues/11
pub mod issue11 {
    use async_trait::async_trait;
    use std::sync::Arc;

    #[async_trait]
    trait Issue11 {
        async fn example(self: Arc<Self>);
    }

    struct Struct;

    #[async_trait]
    impl Issue11 for Struct {
        async fn example(self: Arc<Self>) {}
    }
}

// https://github.com/dtolnay/async-trait/issues/15
pub mod issue15 {
    use async_trait::async_trait;
    use std::marker::PhantomData;

    trait Trait {}

    #[async_trait]
    trait Issue15 {
        async fn myfn(&self, _: PhantomData<dyn Trait + Send>) {}
    }
}

// https://github.com/dtolnay/async-trait/issues/17
pub mod issue17 {
    use async_trait::async_trait;

    #[async_trait]
    trait Issue17 {
        async fn f(&self);
    }

    struct Struct {
        string: String,
    }

    #[async_trait]
    impl Issue17 for Struct {
        async fn f(&self) {
            println!("{}", self.string);
        }
    }
}

// https://github.com/dtolnay/async-trait/issues/23
pub mod issue23 {
    use async_trait::async_trait;

    #[async_trait]
    pub trait Issue23 {
        async fn f(self);

        async fn g(mut self)
        where
            Self: Sized,
        {
            do_something(&mut self);
        }
    }

    struct S {}

    #[async_trait]
    impl Issue23 for S {
        async fn f(mut self) {
            do_something(&mut self);
        }
    }

    fn do_something<T>(_: &mut T) {}
}

// https://github.com/dtolnay/async-trait/issues/25
#[cfg(async_trait_nightly_testing)]
pub mod issue25 {
    use crate::executor;
    use async_trait::async_trait;
    use std::fmt::{Display, Write};

    #[async_trait]
    trait AsyncToString {
        async fn async_to_string(&self) -> String;
    }

    #[async_trait]
    impl AsyncToString for String {
        async fn async_to_string(&self) -> String {
            "special".to_owned()
        }
    }

    macro_rules! hide_from_stable_parser {
        ($($tt:tt)*) => {
            $($tt)*
        };
    }

    hide_from_stable_parser! {
        #[async_trait]
        impl<T: ?Sized + Display + Sync> AsyncToString for T {
            default async fn async_to_string(&self) -> String {
                let mut buf = String::new();
                buf.write_fmt(format_args!("{}", self)).unwrap();
                buf
            }
        }
    }

    #[test]
    fn test() {
        let fut = true.async_to_string();
        assert_eq!(executor::block_on_simple(fut), "true");

        let string = String::new();
        let fut = string.async_to_string();
        assert_eq!(executor::block_on_simple(fut), "special");
    }
}

// https://github.com/dtolnay/async-trait/issues/28
pub mod issue28 {
    use async_trait::async_trait;

    struct Str<'a>(&'a str);

    #[async_trait]
    trait Trait1<'a> {
        async fn f(x: Str<'a>) -> &'a str;
        async fn g(x: Str<'a>) -> &'a str {
            x.0
        }
    }

    #[async_trait]
    impl<'a> Trait1<'a> for str {
        async fn f(x: Str<'a>) -> &'a str {
            x.0
        }
    }

    #[async_trait]
    trait Trait2 {
        async fn f();
    }

    #[async_trait]
    impl<'a> Trait2 for &'a () {
        async fn f() {}
    }

    #[async_trait]
    trait Trait3<'a, 'b> {
        async fn f(_: &'a &'b ()); // chain 'a and 'b
        async fn g(_: &'b ()); // chain 'b only
        async fn h(); // do not chain
    }
}

// https://github.com/dtolnay/async-trait/issues/31
pub mod issue31 {
    use async_trait::async_trait;

    pub struct Struct<'a> {
        pub name: &'a str,
    }

    #[async_trait]
    pub trait Trait<'a> {
        async fn hello(thing: Struct<'a>) -> String;
        async fn hello_twice(one: Struct<'a>, two: Struct<'a>) -> String {
            let str1 = Self::hello(one).await;
            let str2 = Self::hello(two).await;
            str1 + &str2
        }
    }
}

#[async_trait]
pub unsafe trait UnsafeTrait {}

#[async_trait]
unsafe impl UnsafeTrait for () {}

#[async_trait]
pub(crate) unsafe trait UnsafeTraitPubCrate {}

#[async_trait]
unsafe trait UnsafeTraitPrivate {}
