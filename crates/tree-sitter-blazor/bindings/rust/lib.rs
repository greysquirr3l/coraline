use tree_sitter_language::LanguageFn;

extern "C" {
    fn tree_sitter_blazor() -> *const ();
}

pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_blazor) };

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
