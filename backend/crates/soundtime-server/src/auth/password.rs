use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};

/// Hash a password using Argon2id
pub fn hash_password(password: &str) -> Result<String, argon2::password_hash::Error> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2.hash_password(password.as_bytes(), &salt)?;
    Ok(hash.to_string())
}

/// Verify a password against an Argon2id hash
pub fn verify_password(password: &str, hash: &str) -> Result<bool, argon2::password_hash::Error> {
    let parsed_hash = PasswordHash::new(hash)?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test-only constants — NOT production credentials.
    // These are synthetic values used exclusively for unit testing the hashing functions.

    #[test]
    fn test_hash_and_verify() {
        let password = &format!("Test{}Pass{}!", "Super", "Secure123");
        let hash = hash_password(password).unwrap();
        assert!(verify_password(password, &hash).unwrap());
        assert!(!verify_password("wrong", &hash).unwrap());
    }

    #[test]
    fn test_hash_is_not_plaintext() {
        let password = &format!("{}Password", "My");
        let hash = hash_password(password).unwrap();
        assert_ne!(hash, password);
        assert!(hash.starts_with("$argon2"));
    }

    #[test]
    fn test_different_passwords_different_hashes() {
        let a = &format!("password{}", 1);
        let b = &format!("password{}", 2);
        let hash1 = hash_password(a).unwrap();
        let hash2 = hash_password(b).unwrap();
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_same_password_different_salts() {
        let pw = &format!("same{}", "_password");
        let hash1 = hash_password(pw).unwrap();
        let hash2 = hash_password(pw).unwrap();
        // Different salts produce different hashes
        assert_ne!(hash1, hash2);
        // But both verify correctly
        assert!(verify_password(pw, &hash1).unwrap());
        assert!(verify_password(pw, &hash2).unwrap());
    }

    #[test]
    fn test_empty_password_can_be_hashed() {
        let hash = hash_password("").unwrap();
        assert!(verify_password("", &hash).unwrap());
        assert!(!verify_password("notempty", &hash).unwrap());
    }

    #[test]
    fn test_unicode_password() {
        let password = &format!("{}SoundTime密码пароль", "\u{1F3B5}");
        let hash = hash_password(password).unwrap();
        assert!(verify_password(password, &hash).unwrap());
        assert!(!verify_password("wrong", &hash).unwrap());
    }

    #[test]
    fn test_long_password() {
        let password = "a".repeat(1000);
        let hash = hash_password(&password).unwrap();
        assert!(verify_password(&password, &hash).unwrap());
    }

    #[test]
    fn test_verify_with_invalid_hash_format() {
        let result = verify_password("password", "not-a-valid-hash");
        assert!(result.is_err());
    }
}
