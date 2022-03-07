use async_trait::async_trait;

struct Client;
struct Client2;
trait IntoUrl {}

#[async_trait]
pub trait ClientExt {
    async fn publish<T: IntoUrl>(&self, url: T) -> String;
}

// https://github.com/rust-lang/rust/issues/93828
#[async_trait]
impl ClientExt for Client {
    async fn publish<T: IntoUrl>(&self, url: T) -> String {
        "Foo".to_string()
    }
}

// Variant test case with no bounds at all.
// This doesn't actually work correctly yet. It ought to insert the colon,
// but getting it to do that would require a way to tell rustc that the bounds
// don't actually exist in the source code, and it needs to insert them.
#[async_trait]
impl ClientExt for Client2 {
    async fn publish<T>(&self, url: T) -> String {
        "Foo".to_string()
    }
}

fn main() {}
