//! The JSONC dialect PlaneAI reads and writes: JSON plus comments and trailing commas.

use jsonc_parser::ParseOptions;
use serde::de::DeserializeOwned;

pub fn options() -> ParseOptions {
    ParseOptions {
        allow_comments: true,
        allow_trailing_commas: true,
        allow_loose_object_property_names: false,
        allow_missing_commas: false,
        allow_single_quoted_strings: false,
        allow_hexadecimal_numbers: false,
        allow_unary_plus_numbers: false,
        allow_bare_decimal_point_numbers: false,
        allow_non_finite_numbers: false,
        allow_extended_string_escapes: false,
    }
}

pub fn parse<T: DeserializeOwned>(text: &str) -> Result<T, String> {
    jsonc_parser::parse_to_serde_value(text, &options()).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    #[test]
    fn accepts_comments_and_trailing_commas() {
        let text = "{\n  // note\n  \"a\": [1, 2,], /* inline */\n  \"b\": true,\n}";
        assert_eq!(
            parse::<Value>(text).unwrap(),
            json!({"a": [1, 2], "b": true})
        );
    }

    #[test]
    fn rejects_json5_extensions() {
        assert_eq!(
            parse::<Value>("{'a': 1}").unwrap_err(),
            "Single-quoted strings are not allowed on line 1 column 2"
        );
        assert_eq!(
            parse::<Value>("{\"a\": 0xFF}").unwrap_err(),
            "Hexadecimal numbers are not allowed on line 1 column 7"
        );
    }
}
