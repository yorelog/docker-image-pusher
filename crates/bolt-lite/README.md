# bolt-lite

Minimal, read-only BoltDB parser focused on containerd metadata.db. It keeps everything in-memory, walks buckets, and exposes a tiny API that mirrors common containerd layouts.

## Features
- Parse BoltDB meta pages and B+tree without unsafe code.
- Bucket navigation (`Tx::bucket`, `Tx::bucket_path`), entry lookup, and iteration.
- Cursor over leaf entries and simple stats.
- No writes or page mutations.

## Quick start
```rust
use bolt_lite::Bolt;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = Bolt::open_ro("/var/lib/containerd/io.containerd.metadata.v1.bolt/meta.db")?;
    let tx = db.begin()?;
    let images = tx
        .bucket_path(&[b"v1", b"default", b"images"])
        .expect("images bucket")
        .iter_buckets();

    for (name, bucket) in images {
        if let Some(desc) = bucket.get(b"target") {
            println!("{} -> {} bytes", String::from_utf8_lossy(&name), desc.len());
        }
    }
    Ok(())
}
```

## Buckets and helpers
- `Tx::bucket_path` handles nested buckets by byte segments.
- `Bucket::get` and `Bucket::bucket` fetch values or nested buckets.
- `Bucket::cursor` iterates leaf entries; `Bucket::iter_buckets` discovers child buckets with `BUCKET_VALUE_FLAG` set.
- `ok_opt` converts `Result<T, E>` into `Option<T>` when you want fallbacks.

## Notes
- Only supports read-only access; checksum and freelist validation are intentionally skipped to tolerate containerd variants.
- Page sizes outside 1 KiB–64 KiB are rejected.
- Inline buckets (root=0) are handled by inlining the value bytes.

## License
MIT
