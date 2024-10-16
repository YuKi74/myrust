use validator::ValidationError;

#[cfg(feature = "regexp-validator")]
pub fn validate_regexp(regexp: &str) -> Result<(), ValidationError> {
    regex::Regex::new(regexp)
        .map(|_| ())
        .map_err(|_| ValidationError::new("invalid regexp"))
}