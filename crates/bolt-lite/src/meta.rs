use crate::{Error, Result};

pub const MAGIC: u32 = 0xED0C_DAED;
pub const META_PAGE_FLAG: u16 = 0x04;
pub const BRANCH_PAGE_FLAG: u16 = 0x01;
pub const LEAF_PAGE_FLAG: u16 = 0x02;
pub const BUCKET_VALUE_FLAG: u32 = 0x01;
pub const META_STRUCT_OFFSET: usize = 16; // after generic page header

#[derive(Clone, Copy, Debug)]
pub(crate) struct BucketHeader {
    pub(crate) root: u64,
    pub(crate) _sequence: u64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Meta {
    pub(crate) root: BucketHeader,
    pub(crate) txid: u64,
}

pub(crate) fn parse_page_size(data: &[u8]) -> Result<u32> {
    let meta_slice = &data[..4096];
    let magic = u32::from_le_bytes(meta_slice[META_STRUCT_OFFSET..META_STRUCT_OFFSET + 4].try_into().unwrap());
    if magic != MAGIC {
        return Err(Error::InvalidMagic);
    }
    let page_size = u32::from_le_bytes(meta_slice[META_STRUCT_OFFSET + 8..META_STRUCT_OFFSET + 12].try_into().unwrap());
    if page_size < 1024 || page_size > 65536 {
        return Err(Error::InvalidPageSize(page_size));
    }
    Ok(page_size)
}

pub(crate) fn parse_meta_at(data: &[u8], page_size: usize, offset: usize) -> Result<Meta> {
    let end = offset + page_size;
    if end > data.len() {
        return Err(Error::Corrupt("meta page truncated"));
    }
    let page = &data[offset..end];
    let flags = u16::from_le_bytes(page[8..10].try_into().unwrap());
    if flags & META_PAGE_FLAG == 0 {
        return Err(Error::Corrupt("page 0 not meta"));
    }
    let magic = u32::from_le_bytes(page[META_STRUCT_OFFSET..META_STRUCT_OFFSET + 4].try_into().unwrap());
    if magic != MAGIC {
        return Err(Error::InvalidMagic);
    }
    let page_size_meta = u32::from_le_bytes(page[META_STRUCT_OFFSET + 8..META_STRUCT_OFFSET + 12].try_into().unwrap());
    if page_size_meta as usize != page_size {
        return Err(Error::InvalidPageSize(page_size_meta));
    }

    let root = BucketHeader {
        root: u64::from_le_bytes(page[META_STRUCT_OFFSET + 16..META_STRUCT_OFFSET + 24].try_into().unwrap()),
        _sequence: u64::from_le_bytes(page[META_STRUCT_OFFSET + 24..META_STRUCT_OFFSET + 32].try_into().unwrap()),
    };
    let txid = u64::from_le_bytes(page[META_STRUCT_OFFSET + 48..META_STRUCT_OFFSET + 56].try_into().unwrap());

    Ok(Meta { root, txid })
}
