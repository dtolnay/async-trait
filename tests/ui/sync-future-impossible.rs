use async_trait::async_trait;

#[async_trait]
trait Trait {
    #[future_is[Sync]]
    async fn method(&mut self);
}

struct Struct<T> {
    value: T,
}

#[async_trait]
impl<T: std::fmt::Display + Send> Trait for Struct<T> {
    #[future_is[Sync]]
    async fn method(&mut self) {
        println!("{}", self.value);
    }
}

fn main() {}
