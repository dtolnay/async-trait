use async_trait_fn::async_trait;

#[async_trait]
pub trait Trait {
    async fn method();
}

#[async_trait]
impl Trait for &'static str {
    async fn method() {
        let _ = Self;
    }
}

fn main() {}
