# motyga-protocol

This crate defines the "types" for the protocol used by Motyga CLI, which includes both "internal types" for communication between `motyga-core` and `motyga-tui`, as well as "external types" used with `motyga app-server`.

This crate should have minimal dependencies.

Ideally, we should avoid "material business logic" in this crate, as we can always introduce `Ext`-style traits to add functionality to types in other crates.
