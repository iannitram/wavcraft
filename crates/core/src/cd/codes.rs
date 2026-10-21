//! ISRC and EAN-13 validators.
//!
//! ISRC format: CC-XXX-YY-NNNNN (12 characters without dashes)
//! EAN-13 format: 13 digits with valid check digit

/// Checks if an ISRC is valid.
pub fn is_valid_isrc(input: &str) -> bool {
    normalize_isrc(input).is_some()
}

/// Normalizes an ISRC to uppercase without dashes.
pub fn normalize_isrc(input: &str) -> Option<String> {
    // Defensive ASCII guard
    if !input.is_ascii() {
        return None;
    }

    // Remove all dashes (but not whitespace) to get the cleaned version
    let cleaned: String = input.chars().filter(|c| *c != '-').collect();

    // Check if it's exactly 12 characters after cleaning
    if cleaned.len() != 12 {
        return None;
    }

    // Check if it's valid format: CC-XXX-YY-NNNNN
    let chars: Vec<char> = cleaned.chars().collect();

    // Check country code (first 2 characters): ASCII letters
    if !chars[0].is_ascii_alphabetic() || !chars[1].is_ascii_alphabetic() {
        return None;
    }

    // Check registrant (characters 3-5): ASCII letters or digits
    if !chars[2].is_ascii_alphanumeric() || !chars[3].is_ascii_alphanumeric() || !chars[4].is_ascii_alphanumeric() {
        return None;
    }

    // Check year (characters 6-7): ASCII digits
    if !chars[5].is_ascii_digit() || !chars[6].is_ascii_digit() {
        return None;
    }

    // Check designation (characters 8-12): ASCII digits
    if !chars[7].is_ascii_digit()
        || !chars[8].is_ascii_digit()
        || !chars[9].is_ascii_digit()
        || !chars[10].is_ascii_digit()
        || !chars[11].is_ascii_digit()
    {
        return None;
    }

    // If there are dashes, they must be at specific positions (2, 6, 9 for 12-char input)
    if input.contains('-') {
        // For a valid ISRC with dashes, the pattern must be:
        // CC-XXX-YY-NNNNN -> CC-XXX-YY-NNNNN (15 chars total)
        // Dashes at positions 2, 6, 9 (0-indexed)
        let mut dash_positions = Vec::new();
        for (i, c) in input.chars().enumerate() {
            if c == '-' {
                dash_positions.push(i);
            }
        }

        // If there are dashes, they must be exactly at positions 2, 6, 9
        if dash_positions != [2, 6, 9] {
            return None;
        }
    }

    // Normalize to uppercase
    Some(cleaned.to_uppercase())
}

/// Checks if an EAN-13 is valid.
pub fn is_valid_ean13(input: &str) -> bool {
    if input.len() != 13 {
        return false;
    }

    // Check if all characters are digits
    if !input.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }

    // Calculate check digit
    let check_digit = ean13_check_digit(input[..12].as_ref());
    match check_digit {
        Some(expected) => match input.chars().last() {
            Some(last_char) => match last_char.to_digit(10) {
                Some(actual) => actual as u8 == expected,
                None => false,
            },
            None => false,
        },
        None => false,
    }
}

/// Calculates the check digit for EAN-13.
pub fn ean13_check_digit(digits: &str) -> Option<u8> {
    if digits.len() != 12 {
        return None;
    }

    // Check if all characters are digits
    if !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }

    let mut sum = 0;
    for (i, c) in digits.chars().enumerate() {
        let digit = match c.to_digit(10) {
            Some(d) => d as u8,
            None => return None,
        };
        if i % 2 == 0 {
            sum += digit; // Odd position (1-indexed)
        } else {
            sum += digit * 3; // Even position (1-indexed)
        }
    }

    Some((10 - sum % 10) % 10)
}

/// Calculates the check digit for UPC-A.
pub fn upc_a_check_digit(digits: &str) -> Option<u8> {
    // Must be exactly 11 ASCII digits
    if digits.len() != 11 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }

    let mut sum = 0;
    for (i, c) in digits.chars().enumerate() {
        let digit = match c.to_digit(10) {
            Some(d) => d as u8,
            None => return None,
        };
        // Odd positions (1-indexed) are multiplied by 3, even positions by 1
        if (i + 1) % 2 == 1 {
            sum += digit * 3;
        } else {
            sum += digit;
        }
    }

    Some((10 - sum % 10) % 10)
}

/// Checks if a UPC-A is valid.
pub fn is_valid_upc_a(input: &str) -> bool {
    // Must be exactly 12 ASCII digits
    if input.len() != 12 || !input.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }

    // Get the check digit from the last character
    let last_digit = input.chars().last();
    let last_digit_value = match last_digit {
        Some(d) => match d.to_digit(10) {
            Some(d) => d as u8,
            None => return false,
        },
        None => return false,
    };

    // Calculate the expected check digit
    let expected_check_digit = upc_a_check_digit(&input[..11]);
    match expected_check_digit {
        Some(expected) => last_digit_value == expected,
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isrc_valid_table() {
        let valid_isrcs = ["USRC17607839", "US-RC1-76-07839", "usrc17607839", "GBAYE0601498", "QZES40000001", "us-rc1-76-07839"];

        for isrc in &valid_isrcs {
            assert!(is_valid_isrc(isrc));
        }
    }

    #[test]
    fn isrc_invalid_table() {
        let invalid_isrcs = [
            "USRC1760783",     // 11 characters
            "USRC176078390",   // 13 characters
            "USRC17607839X",   // 13 characters with letter
            "1SRC17607839",    // first character is a digit
            "USRC1X607839",    // year characters are X6
            "US-RC17-607839",  // dash after 6th character
            "US RC1 76 07839", // spaces
            "",                // empty string
        ];

        for isrc in &invalid_isrcs {
            assert!(!is_valid_isrc(isrc));
            assert_eq!(normalize_isrc(isrc), None);
        }
    }

    #[test]
    fn isrc_normalizes_to_uppercase_without_dashes() {
        let test_cases = [
            ("USRC17607839", "USRC17607839"),
            ("US-RC1-76-07839", "USRC17607839"),
            ("usrc17607839", "USRC17607839"),
            ("us-rc1-76-07839", "USRC17607839"),
            ("GBAYE0601498", "GBAYE0601498"),
            ("QZES40000001", "QZES40000001"),
        ];

        for (input, expected) in &test_cases {
            assert_eq!(normalize_isrc(input), Some(expected.to_string()));
        }
    }

    #[test]
    fn isrc_ascii_guard_regression() {
        // These should all return None due to non-ASCII characters
        let non_ascii_inputs = ["USRC176078é", "ÜSRC17607839", "USRC1760783🎵", "USRC17607839\u{301}", "éééééé"];

        for input in &non_ascii_inputs {
            assert_eq!(normalize_isrc(input), None);
            assert!(!is_valid_isrc(input));
        }

        // This should still work
        assert_eq!(normalize_isrc("USRC17607839"), Some("USRC17607839".to_string()));
    }

    #[test]
    fn ean13_valid_table() {
        let valid_eans = ["4006381333931", "5901234123457", "0012345678905", "9780306406157"];

        for ean in &valid_eans {
            assert!(is_valid_ean13(ean));
        }
    }

    #[test]
    fn ean13_invalid_table() {
        let invalid_eans = [
            "4006381333932",    // check digit should be 1
            "400638133393",     // 12 digits
            "40063813339311",   // 14 digits
            "400638133393A",    // letter
            "4006 3813 3393 1", // spaces
            "",                 // empty string
        ];

        for ean in &invalid_eans {
            assert!(!is_valid_ean13(ean));
        }
    }

    #[test]
    fn ean13_check_digit_known_values() {
        let test_cases = [("400638133393", Some(1)), ("590123412345", Some(7)), ("001234567890", Some(5)), ("978030640615", Some(7))];

        for (digits, expected) in &test_cases {
            assert_eq!(ean13_check_digit(digits), *expected);
        }
    }

    #[test]
    fn ean13_check_digit_rejects_bad_input() {
        let bad_inputs = [
            "40063813339",   // 11 digits
            "40063813339A",  // contains letter
            "",              // empty string
            "4006381333931", // 13 digits
        ];

        for input in &bad_inputs {
            assert_eq!(ean13_check_digit(input), None);
        }
    }

    #[test]
    fn upc_a_check_digit_valid() {
        // Test cases from the brief: upc_a_check_digit("03600029145") should be Some(2)
        assert_eq!(upc_a_check_digit("03600029145"), Some(2));

        // Additional test cases
        assert_eq!(upc_a_check_digit("00000000000"), Some(0));
        assert_eq!(upc_a_check_digit("12345678901"), Some(2));
        assert_eq!(upc_a_check_digit("99999999999"), Some(3));
    }

    #[test]
    fn upc_a_check_digit_invalid() {
        // Test invalid inputs
        assert_eq!(upc_a_check_digit("0360002914"), None); // 10 digits
        assert_eq!(upc_a_check_digit("036000291452"), None); // 12 digits
        assert_eq!(upc_a_check_digit("0360002914A"), None); // contains letter
        assert_eq!(upc_a_check_digit(""), None); // empty string
        assert_eq!(upc_a_check_digit("0360002914é"), None); // contains non-ASCII
    }

    #[test]
    fn is_valid_upc_a_valid() {
        // Test the classic valid UPC-A from the brief
        assert!(is_valid_upc_a("036000291452"));

        // Additional valid cases
        assert!(is_valid_upc_a("000000000000"));
        assert!(is_valid_upc_a("123456789012"));
    }

    #[test]
    fn is_valid_upc_a_invalid() {
        // Test invalid UPC-A numbers
        assert!(!is_valid_upc_a("036000291453")); // Wrong check digit
        assert!(!is_valid_upc_a("03600029145")); // 11 digits
        assert!(!is_valid_upc_a("0360002914522")); // 13 digits
        assert!(!is_valid_upc_a("03600029145A")); // Contains letter
        assert!(!is_valid_upc_a("036000291 452")); // Contains space
        assert!(!is_valid_upc_a("03600029145é")); // Contains non-ASCII
        assert!(!is_valid_upc_a("")); // Empty string
    }

    #[test]
    fn upc_a_equivalence_with_ean13() {
        // The brief states that is_valid_ean13("0036000291452") should be true
        // This is because UPC-A "036000291452" becomes EAN-13 "0036000291452"
        assert!(is_valid_ean13("0036000291452"));
    }
}
