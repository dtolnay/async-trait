#![deny(bare_trait_objects)]

use async_trait::async_trait;

type Elided<'a> = &'a usize;

#[async_trait]
trait TestOk1 {
    async fn test_ok1(elided: Elided, okay: &usize);
}

#[async_trait]
trait TestOk2 {
    async fn test_ok2<'a>(elided: Elided<'a>, okay: &usize) -> &'a usize;
}

#[async_trait]
trait TestOk3 {
    async fn test_ok3(elided: Elided) -> &usize;
}

#[async_trait]
trait TestNok {
    async fn test_nok(elided: Elided, okay: &usize) -> &usize;
}

fn main() {}
