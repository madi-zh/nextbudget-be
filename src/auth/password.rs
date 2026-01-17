use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Algorithm, Argon2, Params, Version,
};
use lazy_static::lazy_static;

use crate::errors::AppError;

lazy_static! {
    /// Configured Argon2 instance with explicit parameters for consistent hashing
    /// Parameters: memory=19456 KiB, iterations=2, parallelism=1
    static ref ARGON2: Argon2<'static> = Argon2::new(
        Algorithm::Argon2id,
        Version::V0x13,
        Params::new(19456, 2, 1, None).expect("Invalid Argon2 params")
    );
}

/// Hash a password using Argon2id
pub fn hash_password(password: &str) -> Result<String, AppError> {
    let salt = SaltString::generate(&mut OsRng);
    ARGON2
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|e| AppError::InternalError(format!("Failed to hash password: {e}")))
}

/// Verify a password against a stored hash
pub fn verify_password(password: &str, hash: &str) -> Result<bool, AppError> {
    let parsed_hash = PasswordHash::new(hash)
        .map_err(|e| AppError::InternalError(format!("Invalid password hash: {e}")))?;
    Ok(ARGON2
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_password_creates_valid_hash() {
        let password = "secure_password123";
        let hash = hash_password(password).expect("Should hash password");
        assert!(hash.starts_with("$argon2"), "Hash should be Argon2 format");
    }

    #[test]
    fn test_hash_password_different_salts() {
        let password = "same_password";
        let hash1 = hash_password(password).expect("Should hash password");
        let hash2 = hash_password(password).expect("Should hash password");
        assert_ne!(hash1, hash2, "Hashes should differ due to random salt");
    }

    #[test]
    fn test_verify_password_correct() {
        let password = "test_password";
        let hash = hash_password(password).expect("Should hash password");
        let is_valid = verify_password(password, &hash).expect("Should verify");
        assert!(is_valid, "Correct password should verify");
    }

    #[test]
    fn test_verify_password_incorrect() {
        let password = "correct_password";
        let wrong_password = "wrong_password";
        let hash = hash_password(password).expect("Should hash password");
        let is_valid = verify_password(wrong_password, &hash).expect("Should verify");
        assert!(!is_valid, "Wrong password should not verify");
    }

    #[test]
    fn test_verify_password_invalid_hash() {
        let result = verify_password("password", "invalid_hash");
        assert!(result.is_err(), "Invalid hash should return error");
    }
}
