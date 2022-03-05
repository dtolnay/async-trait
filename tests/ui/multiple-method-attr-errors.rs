use async_trait::async_trait;

#[async_trait(?Send)]
pub trait Trait {
    #[async_trait(?Send)]
    #[async_trait]
    #[async_trait(?Send)]
    async fn method();
}

#[async_trait]
pub trait Trait2 {
    #[async_trait(?Send)]
    #[async_trait(?Send)]
    async fn method();
}

fn main() {}
