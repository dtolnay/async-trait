use async_trait::async_trait;

pub struct Struct {}

#[async_trait]
pub trait Trait {
    async fn method(self);
}

#[async_trait]
impl Trait for Struct {
    async fn method(self) {
        let _: () = self;
        let _: Self = Self;
    }
}

fn main() {}
