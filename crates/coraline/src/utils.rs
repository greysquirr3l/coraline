#![forbid(unsafe_code)]

pub const fn version() -> &'static str {
    "0.1.0"
}

pub fn hash_sha256(input: &str) -> String {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    hex::encode(result)
}

pub fn node_id_for_symbol(
    file_path: &str,
    kind: &str,
    qualified_name: &str,
    start_line: i64,
) -> String {
    let seed = format!("{file_path}|{kind}|{qualified_name}|{start_line}");
    hash_sha256(&seed)
}
