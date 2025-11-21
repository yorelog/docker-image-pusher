# oci-core

`oci-core` is a lightweight, MIT-licensed OCI registry utility library extracted from
`docker-image-pusher`. The crate focuses on providing the minimum set of
building blocks required to authenticate with registries, inspect references,
and upload blobs and manifests in a fully asynchronous manner.

## Features

- Minimal `Reference` parser with clear error semantics (`OciError`).
- Auth helpers for anonymous and basic authentication flows.
- Streaming `Client` built on top of `reqwest` and `tokio` that supports
  chunked uploads, upload resumption, and manifest/config pushes.
- Error types designed for reuse in higher-level tools.

## Usage

Add the crate as a dependency from a sibling workspace member:

```toml
[dependencies]
oci-core = { path = "../oci-core" }
```

Then create a client and authenticate against a registry:

```rust
use oci_core::{auth::RegistryAuth, client::{Client, ClientConfig}, reference::Reference};

let client = Client::new(ClientConfig::default());
let target: Reference = "registry.example.com/tools/app:latest".parse()?;
let auth = RegistryAuth::basic("user", "secret");
// start pushing blobs...
```

## License

`oci-core` is distributed under the MIT license (see `LICENSE`).
