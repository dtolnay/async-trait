use async_trait::async_trait;

pub struct S {}

#[async_trait]
pub trait Trait {
    async fn method(self);
}

#[async_trait]
impl Trait for S {
    async fn method(self) {
        let _: () = self;
        let _: Self = Self;
    }
}

fn main() {}
