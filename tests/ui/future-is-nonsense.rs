use async_trait::async_trait;

#[async_trait]
trait Trait {
    #[future_is[Nonsense]]
    async fn method1(&mut self);

    #[future_is[Sync - Send]]
    async fn method2(&mut self);
    
    #[future_is(Sync)]
    async fn method3(&mut self);
}

fn main() {}
