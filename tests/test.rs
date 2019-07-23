#![feature(async_await)]

use async_trait::async_trait;

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
