#![cfg(feature = "alloc")]
#![no_std]

extern crate alloc;
extern crate std;

use alloc::boxed::Box;
use async_trait::async_trait;

mod executor;

struct Struct;

#[async_trait]
trait Trait {
    type Assoc;

    async fn selfref(&self) {}

    async fn required() -> Self::Assoc;

    async fn generic_type_param<T: Send>(x: Box<T>) -> T {
        *x
    }
}

#[async_trait]
impl Trait for Struct {
    type Assoc = ();

    async fn selfref(&self) {}

    async fn required() -> Self::Assoc {}

    async fn generic_type_param<T: Send>(x: Box<T>) -> T {
        *x
    }
}

#[test]
fn test_no_std_alloc_trait_methods() {
    executor::block_on_simple(async {
        let s = Struct;
        s.selfref().await;

        Struct::required().await;
        Struct::generic_type_param(Box::new("")).await;
    });
}

#[test]
fn test_no_std_alloc_dyn_compatible() {
    #[async_trait]
    trait DynCompatible {
        async fn f(&self);
    }

    #[async_trait]
    impl DynCompatible for Struct {
        async fn f(&self) {}
    }

    executor::block_on_simple(async {
        let object = &Struct as &dyn DynCompatible;
        object.f().await;
    });
}
