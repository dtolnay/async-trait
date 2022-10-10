use async_trait::async_trait;

struct A;

#[async_trait]
pub trait Trait {
    async fn method(&self);
}

#[async_trait]
impl Trait for A {
    async fn method(&self) {}
}

fn main() {
    fn test<T>(a: T) where T: Trait + Send + Sync  {}
    test(A.method());
}
