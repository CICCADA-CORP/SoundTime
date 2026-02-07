# Security Policy

## Supported Versions

Only the latest `1.x` release is currently supported with security updates. We recommend always running the latest version.

| Version | Supported          |
| ------- | ------------------ |
| 1.x     | :white_check_mark: |
| < 1.0   | :x:                |

## Reporting a Vulnerability

We take security seriously at SoundTime. If you discover a security vulnerability, we appreciate your help in disclosing it responsibly.

### How to Report

Please **DO NOT** report security vulnerabilities via public GitHub issues.

Instead, please report vulnerabilities by email to **security@ciccada-corp.com** (replace with your actual security contact).

Please include the following details in your report:
- Type of issue (e.g., cross-site scripting, SQL injection, RCE, authorization bypass)
- Step-by-step instructions to reproduce the issue
- Proof-of-concept or exploit code/screenshots
- Impact of the vulnerability

### Response Timeline

We will acknowledge receipt of your report within **48 hours**. We aim to resolve critical issues within **14 days**.

You will be notified when a fix is ready. We ask that you wait until the fix is released before publicly disclosing the vulnerability.

### Scope

The following areas are **in scope**:
- Authentication & Authorization flows (JWT, passwords, roles)
- API endpoints (`/api/**`)
- Peer-to-peer protocol (`/federation/**` and iroh transport)
- Data isolation (private playlists, user data)
- Injection vulnerabilities (SQLi, XSS, etc.)

The following are generally **out of scope**:
- Attacks requiring physical access to the user's device
- Social engineering (phishing)
- DoS attacks (though we implement rate limiting)
- Third-party dependencies (unless directly exploitable via SoundTime)
- Vulnerabilities in self-hosted infrastructure (e.g., misconfigured Nginx, weak SSH passwords on host)

## Security Features

SoundTime implements several security features by default:

- **Argon2id** for password hashing (OWASP recommended)
- **JWT** (JSON Web Tokens) for stateless authentication
- **Rate Limiting** on sensitive endpoints (login, register) via `tower-governor`
- **CORS** configuration to restrict cross-origin requests
- **Security Headers** (HSTS, X-Content-Type-Options, etc.)
- **SQL Injection Protection** via Sea-ORM parameterized queries
- **Input Validation** via strict Rust types and Axum extractors

## Safe Harbor

We will not pursue legal action against researchers who:
- Report vulnerabilities to us following this policy
- Do not exploit the vulnerability beyond what is necessary to prove the risk
- Do not access, modify, or delete user data without permission
- Do not disrupt our systems or services

Thank you for helping keep SoundTime secure!
