# **UNI**fied **CO**routines

UNICO is a library that integrates the built-in stackless coroutines (a.k.a. [generators](https://doc.rust-lang.org/nightly/core/ops/trait.Coroutine.html) and [futures](https://doc.rust-lang.org/nightly/core/future/trait.Future.html)) in Rust with powerful stackful coroutines.

## Features

- Straightforward and easy-to-use structures of symmetric and asymetric stackful coroutines.
- Generalized implementation of context switching methods and stack allocators, and users can implement their own.
- Capability of polling futures synchronously inside stackful coroutines, and turning stackful coroutines into generators or futures.

## Reference

This library is partially inspired by [`nbdd0121/stackful`](https://github.com/nbdd0121/stackful).

## License

MIT or APACHE-2.0