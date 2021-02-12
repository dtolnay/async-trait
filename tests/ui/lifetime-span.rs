use async_trait::async_trait;

struct A;
struct B;

#[async_trait]
pub trait Trait<'r> {
    async fn method(&'r self);
}

#[async_trait]
impl Trait for A {
    async fn method(&self) { }
}

#[async_trait]
impl<'r> Trait<'r> for B {
    async fn method(&self) { }
}

fn main() {}
