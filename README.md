# Async trait methods with `Sync` support

This crate is based entirely based on https://github.com/dtolnay/async-trait except that it adds a tiny patch to allow deriving `Sync` on these futures as well (see https://github.com/dtolnay/async-trait/pull/96), which are currently required until the broader ecosystem issues addressed in https://github.com/dtolnay/async-trait/issues/77 get resolved.