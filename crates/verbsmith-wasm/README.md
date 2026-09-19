# verbsmith-wasm

This crate exposes Verbsmith's canonical `.http` parser and formatter to browser
clients. It exists so API Studio and third-party tools do not need to reimplement
the workspace format.

Build with `wasm-pack build crates/verbsmith-wasm --target web`. The generated
package is published as `@verbsmith/workspace` after the project name is legally
cleared and the schema has completed its alpha review.

