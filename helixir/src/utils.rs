

#[inline]
pub fn safe_truncate(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}


#[inline]
pub fn safe_truncate_ellipsis(s: &str, max_chars: usize) -> String {
    if s.chars().count() > max_chars {
        format!("{}...", s.chars().take(max_chars).collect::<String>())
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_truncate_ascii() {
        assert_eq!(safe_truncate("hello world", 5), "hello");
    }

    #[test]
    fn test_safe_truncate_cyrillic() {
        assert_eq!(safe_truncate("Привет мир", 6), "Привет");
    }

    #[test]
    fn test_safe_truncate_mixed() {
        assert_eq!(safe_truncate("Hello Мир!", 8), "Hello Ми");
    }

    #[test]
    fn test_safe_truncate_shorter() {
        assert_eq!(safe_truncate("hi", 10), "hi");
    }

    #[test]
    fn test_safe_truncate_ellipsis() {
        assert_eq!(safe_truncate_ellipsis("hello world", 5), "hello...");
        assert_eq!(safe_truncate_ellipsis("hi", 10), "hi");
    }
}

