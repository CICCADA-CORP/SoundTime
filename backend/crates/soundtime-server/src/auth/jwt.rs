use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    /// Subject (user ID)
    pub sub: Uuid,
    /// Username
    pub username: String,
    /// Role (admin, user)
    pub role: String,
    /// Token type (access, refresh)
    pub token_type: TokenType,
    /// Issued at
    pub iat: i64,
    /// Expiration
    pub exp: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TokenType {
    Access,
    Refresh,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: String,
    pub expires_in: i64,
}

/// Generate access + refresh token pair
pub fn generate_token_pair(
    user_id: Uuid,
    username: &str,
    role: &str,
    secret: &str,
) -> Result<TokenPair, jsonwebtoken::errors::Error> {
    let now = Utc::now();

    // Access token: 15 minutes
    let access_exp = now + Duration::minutes(15);
    let access_claims = Claims {
        sub: user_id,
        username: username.to_string(),
        role: role.to_string(),
        token_type: TokenType::Access,
        iat: now.timestamp(),
        exp: access_exp.timestamp(),
    };
    let access_token = encode(
        &Header::default(),
        &access_claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?;

    // Refresh token: 7 days
    let refresh_exp = now + Duration::days(7);
    let refresh_claims = Claims {
        sub: user_id,
        username: username.to_string(),
        role: role.to_string(),
        token_type: TokenType::Refresh,
        iat: now.timestamp(),
        exp: refresh_exp.timestamp(),
    };
    let refresh_token = encode(
        &Header::default(),
        &refresh_claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?;

    Ok(TokenPair {
        access_token,
        refresh_token,
        token_type: "Bearer".to_string(),
        expires_in: 900, // 15 minutes in seconds
    })
}

/// Validate a JWT token and return claims
pub fn validate_token(token: &str, secret: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )?;
    Ok(token_data.claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "test-secret-key-for-jwt";

    #[test]
    fn test_token_generation_and_validation() {
        let user_id = Uuid::new_v4();

        let pair = generate_token_pair(user_id, "testuser", "user", SECRET).unwrap();
        assert!(!pair.access_token.is_empty());
        assert!(!pair.refresh_token.is_empty());

        let claims = validate_token(&pair.access_token, SECRET).unwrap();
        assert_eq!(claims.sub, user_id);
        assert_eq!(claims.username, "testuser");
        assert_eq!(claims.role, "user");
        assert_eq!(claims.token_type, TokenType::Access);

        let refresh_claims = validate_token(&pair.refresh_token, SECRET).unwrap();
        assert_eq!(refresh_claims.token_type, TokenType::Refresh);
    }

    #[test]
    fn test_access_token_has_correct_claims() {
        let user_id = Uuid::new_v4();
        let pair = generate_token_pair(user_id, "alice", "admin", SECRET).unwrap();
        let claims = validate_token(&pair.access_token, SECRET).unwrap();

        assert_eq!(claims.sub, user_id);
        assert_eq!(claims.username, "alice");
        assert_eq!(claims.role, "admin");
        assert_eq!(claims.token_type, TokenType::Access);
        assert!(claims.exp > claims.iat);
        // Access token should expire in ~15 minutes (900s)
        let diff = claims.exp - claims.iat;
        assert!((899..=901).contains(&diff));
    }

    #[test]
    fn test_refresh_token_has_correct_claims() {
        let user_id = Uuid::new_v4();
        let pair = generate_token_pair(user_id, "bob", "user", SECRET).unwrap();
        let claims = validate_token(&pair.refresh_token, SECRET).unwrap();

        assert_eq!(claims.sub, user_id);
        assert_eq!(claims.token_type, TokenType::Refresh);
        // Refresh token should expire in ~7 days (604800s)
        let diff = claims.exp - claims.iat;
        assert!((604799..=604801).contains(&diff));
    }

    #[test]
    fn test_invalid_secret_rejects_token() {
        let user_id = Uuid::new_v4();
        let pair = generate_token_pair(user_id, "user1", "user", SECRET).unwrap();
        let result = validate_token(&pair.access_token, "wrong-secret");
        assert!(result.is_err());
    }

    #[test]
    fn test_garbage_token_rejected() {
        let result = validate_token("not-a-valid-jwt", SECRET);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_token_rejected() {
        let result = validate_token("", SECRET);
        assert!(result.is_err());
    }

    #[test]
    fn test_access_and_refresh_tokens_are_different() {
        let user_id = Uuid::new_v4();
        let pair = generate_token_pair(user_id, "user1", "user", SECRET).unwrap();
        assert_ne!(pair.access_token, pair.refresh_token);
    }

    #[test]
    fn test_token_pair_bearer_type() {
        let user_id = Uuid::new_v4();
        let pair = generate_token_pair(user_id, "user1", "user", SECRET).unwrap();
        assert_eq!(pair.token_type, "Bearer");
        assert_eq!(pair.expires_in, 900);
    }

    #[test]
    fn test_different_users_get_different_tokens() {
        let pair1 = generate_token_pair(Uuid::new_v4(), "user1", "user", SECRET).unwrap();
        let pair2 = generate_token_pair(Uuid::new_v4(), "user2", "user", SECRET).unwrap();
        assert_ne!(pair1.access_token, pair2.access_token);
        assert_ne!(pair1.refresh_token, pair2.refresh_token);
    }

    #[test]
    fn test_uuid_sub_roundtrip() {
        let user_id = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let pair = generate_token_pair(user_id, "testuser", "user", SECRET).unwrap();
        let claims = validate_token(&pair.access_token, SECRET).unwrap();
        assert_eq!(
            claims.sub.to_string(),
            "550e8400-e29b-41d4-a716-446655440000"
        );
    }

    #[test]
    fn test_token_type_serialization() {
        let json = serde_json::to_string(&TokenType::Access).unwrap();
        assert_eq!(json, "\"access\"");
        let json = serde_json::to_string(&TokenType::Refresh).unwrap();
        assert_eq!(json, "\"refresh\"");
    }
}
