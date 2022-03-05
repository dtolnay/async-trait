use async_trait::async_trait;

#[async_trait(?Send)]
pub trait Trait {
    #[async_trait(?Send)]
    async fn method();
}

pub struct Struct;

#[async_trait(?Send)]
impl Trait for Struct {
    #[async_trait(?Send)]
    async fn method() {}
}

fn main() {}
