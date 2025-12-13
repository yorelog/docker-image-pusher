# containerd-store

A small, read-only helper crate to explore containerd metadata and content. It resolves images from containerd's `meta.db`, finds their content digests, and can export or iterate images without needing a running daemon.

## Features
- Open containerd metadata (`io.containerd.metadata.v1.bolt/meta.db`) and read images by namespace.
- Resolve image descriptors and locate blobs under `io.containerd.content.v1.content/blobs`.
- Export images (and `meta.db`) into a portable bundle; supports multiple images and deduplicates blobs.
- Optional bucket-path logging (`bucket-logging` feature) to trace layout differences across vendors.

## Quick start: list and resolve
```rust
use containerd_store::{ContainerdStore, ImageEntry};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let store = ContainerdStore::open(
        "~/.local/share/containerd",
        "default",
    )?;

    let images: Vec<ImageEntry> = store.list_images()?;
    for img in &images {
        println!("{} -> {} ({} bytes)", img.name, img.target.digest, img.target.size);
    }

    let resolved = store.resolve_image("busybox:latest")?;
    println!("manifest at {}", resolved.manifest_path.display());
    Ok(())
}
```

## Export helper
- Single image: `export_image_to_dir(&store, "busybox:latest", "./out")`
- Multiple images: `export_images_to_dir(&store, &["foo:tag", "bar:tag"], "./out")`

Exports include:
- `io.containerd.metadata.v1.bolt/meta.db` (so bundles are self-contained)
- All referenced manifests, configs, and layers under `io.containerd.content.v1.content/blobs/`
- Deduplicated blobs across all exported images

## Bucket-path logging
Enable `bucket-logging` feature and register a logger:
```rust
containerd_store::set_bucket_match_logger(|m| {
    eprintln!("matched {:?}: {}", m.kind, m.path.join("/"));
}).ok();
```
This is useful when debugging vendor-specific layout changes.

## Notes
- Read-only: no mutations to containerd data.
- Size parsing tolerates missing size fields (defaults to 0, warns once).
- Platform selection prefers linux/amd64 when walking OCI indexes; falls back to the first entry.

## License
MIT
