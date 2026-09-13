/// Compares two secrets without returning early on a mismatched byte.
pub fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let length = left.len().max(right.len());
    let mut difference = left.len() ^ right.len();
    for (left_byte, right_byte) in left
        .iter()
        .copied()
        .chain(std::iter::repeat(0))
        .zip(right.iter().copied().chain(std::iter::repeat(0)))
        .take(length)
    {
        difference |= usize::from(left_byte ^ right_byte);
    }
    difference == 0
}

/// Validates an HTTP bearer credential without comparing the secret prefix.
pub fn bearer_token_matches(header: Option<&str>, expected_token: &str) -> bool {
    header
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|token| constant_time_eq(token.as_bytes(), expected_token.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_equality_requires_the_same_bytes_and_length() {
        assert!(constant_time_eq(b"secret", b"secret"));
        assert!(!constant_time_eq(b"secret", b"secrex"));
        assert!(!constant_time_eq(b"secret", b"secret-longer"));
    }

    #[test]
    fn bearer_matching_requires_a_matching_scheme_and_secret() {
        assert!(bearer_token_matches(Some("Bearer secret"), "secret"));
        assert!(!bearer_token_matches(Some("Bearer wrong"), "secret"));
        assert!(!bearer_token_matches(Some("Basic secret"), "secret"));
        assert!(!bearer_token_matches(None, "secret"));
    }
}
