use std::fs;
use std::path::Path;

use anyhow::Result;
use bolt_lite::{BUCKET_VALUE_FLAG, LEAF_PAGE_FLAG, MAGIC, META_PAGE_FLAG, META_STRUCT_OFFSET};
use containerd_store::ContainerdStore;
use tempfile::tempdir;

const PAGE_SIZE: usize = 4096;
const ROOT_PAGE: u64 = 2;

#[test]
fn discovers_images_from_rootless_v1_layout() -> Result<()> {
    let tmp = tempdir()?;
    let meta_dir = tmp.path().join("io.containerd.metadata.v1.bolt");
    fs::create_dir_all(&meta_dir)?;
    let db_path = meta_dir.join("meta.db");

    let namespace = "rootless";
    build_fixture_db(&db_path, namespace)?;

    let store = ContainerdStore::open(tmp.path(), namespace)?;
    let images = store.list_images()?;

    assert_eq!(images.len(), 1);
    let img = &images[0];
    assert_eq!(img.name, "busybox:latest");
    assert_eq!(img.target.digest, "sha256:deadbeef");
    assert_eq!(
        img.target.media_type,
        "application/vnd.oci.image.manifest.v1+json"
    );
    assert_eq!(img.target.size, 4242);

    Ok(())
}

fn build_fixture_db(path: &Path, namespace: &str) -> Result<()> {
    let page_count = 8;
    let mut data = vec![0u8; PAGE_SIZE * page_count];

    write_meta_page(&mut data[..PAGE_SIZE], ROOT_PAGE, 1);
    write_leaf_page(
        &mut data[PAGE_SIZE * 2..PAGE_SIZE * 3],
        ROOT_PAGE,
        &[bucket_entry(b"v1", 3)],
    )?;
    write_leaf_page(
        &mut data[PAGE_SIZE * 3..PAGE_SIZE * 4],
        3,
        &[bucket_entry(namespace.as_bytes(), 4)],
    )?;
    write_leaf_page(
        &mut data[PAGE_SIZE * 4..PAGE_SIZE * 5],
        4,
        &[bucket_entry(b"images", 5)],
    )?;
    write_leaf_page(
        &mut data[PAGE_SIZE * 5..PAGE_SIZE * 6],
        5,
        &[bucket_entry(b"busybox:latest", 6)],
    )?;
    write_leaf_page(
        &mut data[PAGE_SIZE * 6..PAGE_SIZE * 7],
        6,
        &[bucket_entry(b"target", 7)],
    )?;
    write_leaf_page(
        &mut data[PAGE_SIZE * 7..PAGE_SIZE * 8],
        7,
        &[
            kv_entry(b"digest", b"sha256:deadbeef"),
            kv_entry(b"mediatype", b"application/vnd.oci.image.manifest.v1+json"),
            kv_entry(b"size", &4242i64.to_le_bytes()),
        ],
    )?;

    fs::write(path, data)?;
    Ok(())
}

fn write_meta_page(page: &mut [u8], root_page: u64, txid: u64) {
    page.fill(0);
    page[0..8].copy_from_slice(&0u64.to_le_bytes());
    page[8..10].copy_from_slice(&META_PAGE_FLAG.to_le_bytes());
    page[10..12].copy_from_slice(&(0u16).to_le_bytes());
    page[12..16].copy_from_slice(&(0u32).to_le_bytes());

    page[META_STRUCT_OFFSET..META_STRUCT_OFFSET + 4].copy_from_slice(&MAGIC.to_le_bytes());
    page[META_STRUCT_OFFSET + 8..META_STRUCT_OFFSET + 12]
        .copy_from_slice(&(PAGE_SIZE as u32).to_le_bytes());
    page[META_STRUCT_OFFSET + 16..META_STRUCT_OFFSET + 24]
        .copy_from_slice(&root_page.to_le_bytes());
    page[META_STRUCT_OFFSET + 24..META_STRUCT_OFFSET + 32].copy_from_slice(&0u64.to_le_bytes());
    page[META_STRUCT_OFFSET + 48..META_STRUCT_OFFSET + 56].copy_from_slice(&txid.to_le_bytes());
}

fn write_leaf_page(
    page: &mut [u8],
    page_id: u64,
    entries: &[(Vec<u8>, Vec<u8>, u32)],
) -> Result<()> {
    let max_headers = 16 + entries.len() * 16;
    if max_headers > PAGE_SIZE {
        anyhow::bail!("too many entries for page");
    }

    page.fill(0);
    page[0..8].copy_from_slice(&page_id.to_le_bytes());
    page[8..10].copy_from_slice(&LEAF_PAGE_FLAG.to_le_bytes());
    page[10..12].copy_from_slice(&(entries.len() as u16).to_le_bytes());
    page[12..16].copy_from_slice(&(0u32).to_le_bytes());

    let mut data_offset = max_headers;
    for (idx, (key, value, flags)) in entries.iter().enumerate() {
        let base = 16 + idx * 16;
        let pos = (data_offset - base) as u32;

        let key_start = data_offset;
        let key_end = key_start + key.len();
        let val_end = key_end + value.len();
        if val_end > page.len() {
            anyhow::bail!("entry does not fit in page");
        }

        page[base..base + 4].copy_from_slice(&flags.to_le_bytes());
        page[base + 4..base + 8].copy_from_slice(&pos.to_le_bytes());
        page[base + 8..base + 12].copy_from_slice(&(key.len() as u32).to_le_bytes());
        page[base + 12..base + 16].copy_from_slice(&(value.len() as u32).to_le_bytes());
        page[key_start..key_end].copy_from_slice(key);
        page[key_end..val_end].copy_from_slice(value);

        data_offset = val_end;
    }

    Ok(())
}

fn bucket_entry(key: &[u8], root: u64) -> (Vec<u8>, Vec<u8>, u32) {
    let mut value = vec![0u8; 16];
    value[..8].copy_from_slice(&root.to_le_bytes());
    (key.to_vec(), value, BUCKET_VALUE_FLAG)
}

fn kv_entry(key: &[u8], value: &[u8]) -> (Vec<u8>, Vec<u8>, u32) {
    (key.to_vec(), value.to_vec(), 0u32)
}
