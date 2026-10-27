use tree_sitter_language::LanguageFn;

extern "C" {
    fn tree_sitter_blazor() -> *const ();
}

#[expect(
    unsafe_code,
    reason = "LanguageFn::from_raw is the required tree-sitter binding pattern"
)]
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_blazor) };

#[must_use]
pub const fn language() -> LanguageFn {
    LANGUAGE
}

#[cfg(test)]
mod tests {
    use super::LANGUAGE;

    #[test]
    fn can_load_language() {
        let _ = LANGUAGE;
    }
}
