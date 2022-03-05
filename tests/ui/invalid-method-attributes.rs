use async_trait::async_trait;

#[async_trait]
pub trait Trait {
    #[async_trait]
    async fn method();
}

#[async_trait]
pub trait Trait2 {
    #[async_trait(invalid)]
    async fn method();
}

#[async_trait]
pub trait Trait3 {
    #[async_trait = "invalid"]
    async fn method();
}

fn main() {}

