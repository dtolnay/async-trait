use async_trait::async_trait;

pub trait Trait {
    #[async_trait(?Send)]
    async fn method();
}

fn main() {}
