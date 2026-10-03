# Fuzzing

[cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) targets for the parts of
Blocks2Cpp that read untrusted project files
([spec 08 §8.13](../docs/spec/08-security.md#813-continuous-security-process),
[09 §9.2](../docs/spec/09-quality-and-delivery.md)).

| Target | What it feeds | What must hold |
|--------|---------------|----------------|
| `load` | arbitrary bytes to `b2c_model::load` | no panic, no stack overflow, no unbounded allocation; every rejection has a diagnostic |
| `resolve` | every document `load` accepts, to `b2c_catalog::resolve` with the core catalog | no panic; resolving the completed document again changes nothing |
| `roundtrip` | every document `load` accepts, to `to_canonical_json` | the saved text loads back, saves to the same bytes and has the same content hash |

This folder is its own Cargo workspace: libFuzzer needs a nightly toolchain,
so the main workspace never builds it.

## Running

```sh
cargo install cargo-fuzz          # once
mkdir -p fuzz/corpus/load
cp examples/*.b2c tests/security/projects/*.b2c fuzz/corpus/load/
cargo +nightly fuzz run load fuzz/corpus/load -- -dict=fuzz/b2c.dict -max_len=65536
cargo +nightly fuzz run resolve fuzz/corpus/load -- -dict=fuzz/b2c.dict
cargo +nightly fuzz run roundtrip fuzz/corpus/load -- -dict=fuzz/b2c.dict
```

Add `-max_total_time=60` for the per-PR smoke run and `-max_total_time=1800`
for the nightly run. A crash is saved under `fuzz/artifacts/<target>/`;
reproduce it with `cargo +nightly fuzz run <target> <file>`, then add the file
(minimised with `cargo +nightly fuzz tmin`) as a regression test, and to
`tests/security/projects/` when it is an attack a project file can express.

Checking that the targets compile needs no nightly toolchain:

```sh
cargo check --manifest-path fuzz/Cargo.toml
```
