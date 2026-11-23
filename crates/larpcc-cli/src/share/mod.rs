pub(crate) mod skills;

// A single pending namespace gates all registry consumers. Future alias/MCP
// variants keep separate typed operations and fixed artifact path allowlists.
pub(crate) fn pending(root: &std::path::Path) -> bool {
    !matches!(std::fs::symlink_metadata(root.join("share-journal.json")), Err(e) if e.kind()==std::io::ErrorKind::NotFound)
}
