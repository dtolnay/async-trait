use async_trait_ext::async_trait;

#[async_trait]
pub trait Trait {
    async fn method();
}

pub struct Struct;

#[async_trait]
impl Trait for Struct {
    fn method() {}
}

fn main() {}
