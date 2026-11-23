pub(crate) mod mcp;
pub(crate) mod skills;

// Skills, alias and MCP share the consumer gate and recover their own journal kinds.
pub(crate) fn pending(root: &std::path::Path) -> bool {
    !matches!(std::fs::symlink_metadata(root.join("share-journal.json")), Err(e) if e.kind()==std::io::ErrorKind::NotFound)
}
