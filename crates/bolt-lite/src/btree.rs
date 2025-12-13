use crate::{Bolt, Error, Result, meta::*, page::*};

pub(crate) fn find_in_tree(db: &Bolt, page_id: u64, key: &[u8]) -> Result<Option<LeafEntry>> {
    let page = db.read_page(page_id)?;
    let flags = page_flags(page);
    if flags & BRANCH_PAGE_FLAG != 0 {
        let elems = branch_elements(db.page_size, page)?;
        for i in 0..elems.len() {
            if key < elems[i].key.as_slice() {
                return find_in_tree(db, elems[i].pgid, key);
            }
        }
        return find_in_tree(
            db,
            elems.last().ok_or(Error::Corrupt("empty branch"))?.pgid,
            key,
        );
    }
    if flags & LEAF_PAGE_FLAG != 0 {
        return find_in_page(db.page_size, page, key);
    }
    Err(Error::Corrupt("unexpected page type"))
}

pub(crate) fn find_in_page(page_size: usize, page: &[u8], key: &[u8]) -> Result<Option<LeafEntry>> {
    let entries = leaf_elements(page_size, page)?;
    for e in entries {
        if e.key == key {
            return Ok(Some(e));
        }
    }
    Ok(None)
}

pub(crate) fn collect_tree_entries(db: &Bolt, page_id: u64) -> Result<Vec<LeafEntry>> {
    let page = db.read_page(page_id)?;
    let flags = page_flags(page);
    if flags & BRANCH_PAGE_FLAG != 0 {
        let mut out = Vec::new();
        for elem in branch_elements(db.page_size, page)? {
            out.extend(collect_tree_entries(db, elem.pgid)?);
        }
        Ok(out)
    } else if flags & LEAF_PAGE_FLAG != 0 {
        leaf_elements(db.page_size, page)
    } else {
        Err(Error::Corrupt("unexpected page type"))
    }
}
