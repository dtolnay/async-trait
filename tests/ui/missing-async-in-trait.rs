use async_trait_ext::async_trait;

#[async_trait]
pub trait Trait {
    fn method();
}

pub struct Struct;

#[async_trait]
impl Trait for Struct {
    async fn method() {}
}

fn main() {}
