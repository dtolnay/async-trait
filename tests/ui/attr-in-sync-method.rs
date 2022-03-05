use async_trait::async_trait;

#[async_trait]
pub trait Trait {
    #[async_trait(?Send)]
    fn method();
}

fn main() {}
