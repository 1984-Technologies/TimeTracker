# Authentication fuzz tests

Four libFuzzer targets exercise the actual backend code with its `fuzzing`
feature enabled. The feature only exposes parser entry points. It does not
disable signature verification, expiry checks, or authorization rules, and is
absent from normal backend builds. Fuzzing uses fixed public test keys and never
reads production secrets, contacts a database, or runs a server.

| Target | Checks |
| --- | --- |
| `jwt_raw` | Arbitrary token strings; every accepted token must have an independently verified HS256 signature and valid required claims. |
| `jwt_claims` | Arbitrary payload bytes signed with the test key, plus structured mutations of valid claims; compares acceptance with a separate claims oracle and rejects wrong keys, algorithms, and damaged signatures. |
| `credentials` | base64url secrets and JWT configuration, lowercase hex hashes, SHA-256 round trips, and guaranteed hash mismatches. |
| `authorization` | Bare bearer values, HTTP headers, and gRPC metadata; missing/duplicate headers, whitespace, case, Unicode, and invalid bytes. |

The signed-payload target reaches claim validation without relying on the fuzzer
to discover a valid cryptographic signature. The reference oracle validates the
required claims independently of `JwtClaims` and `jsonwebtoken::Validation`.
Time verdicts are compared before and after each validation call so expiry at a
second boundary cannot create a false failure. Noninteger extra `nbf` values
are exercised without imposing a coercion policy beyond our required claims.

## Replay seeds with stable Rust

These assertions run without libFuzzer or a nightly toolchain:

```sh
cargo test --manifest-path fuzz/Cargo.toml --lib --no-default-features --locked
```

The checked-in `seeds/` include valid and invalid tokens, missing/duplicate claims,
all structured mutation modes, integer boundaries, malformed encodings, Unicode,
non-UTF-8 headers, and empty input. Newly minimized failures should be copied
into the matching seed directory so normal seed replay catches regressions.

## Run coverage-guided fuzzing

Install a nightly toolchain and cargo-fuzz. The CI job pins nightly-2026-11-26
and cargo-fuzz 0.13.2. Building also requires C++ build tools and `protoc`, as in
the main project's build instructions.

```sh
rustup toolchain install nightly --profile minimal
cargo install cargo-fuzz --version 0.13.2 --locked
cargo +nightly fuzz build --no-cfg-fuzzing
mkdir -p fuzz/corpus/jwt_claims
cargo +nightly fuzz run jwt_claims fuzz/corpus/jwt_claims fuzz/seeds/jwt_claims --no-cfg-fuzzing -- -max_total_time=60 -max_len=8192 -timeout=5 -rss_limit_mb=1024 -dict=fuzz/jwt.dict
```

Run the other targets by replacing `jwt_claims` and omitting `-dict`.
The first corpus directory receives discovered inputs; the second supplies the
checked-in seeds. Generated corpus, coverage and crash artifacts are ignored by
Git. `--no-cfg-fuzzing` keeps dependencies on their normal validation paths;
coverage instrumentation and the explicit parser-access feature remain enabled.
`fuzz/Cargo.lock` pins the separate workspace's dependencies.

For Windows hosts, the portable way to run the same checks with AddressSanitizer
is the Linux Docker image (PowerShell):

```powershell
docker build -f fuzz/Dockerfile -t timetracker-auth-fuzz fuzz
docker run --rm --mount "type=bind,source=$($PWD.Path),target=/workspace" -e FUZZ_SECONDS=60 timetracker-auth-fuzz
```

The image installs the pinned nightly toolchain, cargo-fuzz and required build
tools. `fuzz/run.sh` is shared with CI and runs all four targets. Native Windows
fuzzing depends on the installed C++/AddressSanitizer tooling and the nightly
toolchain's coverage support; stable seed replay works without those tools.

Reproduce and minimize a failure with the same sanitizer setting used to find it:

```sh
cargo +nightly fuzz run jwt_claims fuzz/artifacts/jwt_claims/crash-HASH --no-cfg-fuzzing
cargo +nightly fuzz tmin jwt_claims fuzz/artifacts/jwt_claims/crash-HASH --no-cfg-fuzzing
```
