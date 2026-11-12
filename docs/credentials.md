# Credentials

Credentials are resolved in Rust (`crates/sonde-core/src/credentials`), cached until five minutes before expiry, and
used only to sign. The interface sees endpoint ids, names and a description of the source ("profile prod",
"instance role (IMDSv2)"), never a key. Static keys can be set only in `config.toml`.

| `kind` | Source |
|---|---|
| `default` | The AWS default chain, in order: `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` / `AWS_SESSION_TOKEN`; web identity (`AWS_WEB_IDENTITY_TOKEN_FILE` + `AWS_ROLE_ARN`, IRSA on EKS); the profile in `AWS_PROFILE` (or `default`); container credentials (`AWS_CONTAINER_CREDENTIALS_RELATIVE_URI` / `_FULL_URI`: ECS, EKS Pod Identity); IMDSv2. The error lists each step's result. |
| `profile` | A profile in `~/.aws/config` / `~/.aws/credentials` (`AWS_CONFIG_FILE`, `AWS_SHARED_CREDENTIALS_FILE`): static keys; `credential_process`; IAM Identity Center (`sso_session` or legacy `sso_start_url`; uses the token `aws sso login` cached in `~/.aws/sso/cache`, sonde does not log in); `role_arn` with `source_profile` or `credential_source` (STS `AssumeRole`, `external_id`); `role_arn` with `web_identity_token_file`. |
| `env` | Environment variables you name. |
| `imds` | The instance role via IMDSv2 (`AWS_EC2_METADATA_SERVICE_ENDPOINT`, `AWS_EC2_METADATA_DISABLED`). |
| `web-identity` | `AssumeRoleWithWebIdentity` with a token file and role. |
| `process` | A command printing `credential_process` JSON. |
| `static` | Keys in `config.toml` (for local servers). |
| `anonymous` | Unsigned requests (public buckets). |

STS calls go to `sts.<region>.amazonaws.com` (`.com.cn` in China regions) or `AWS_ENDPOINT_URL_STS`.

## Redaction

Every request record is redacted when created, before the ring buffer, SQLite, exports, the inspector or a probe
report sees it (`crates/reqlog/src/redact.rs`):

| What | Done |
|---|---|
| `Authorization` | signature replaced; key id shortened (`AKIA…LE`); scope and signed headers kept |
| `x-amz-security-token`, SSE-C key headers, `Cookie`, `Proxy-Authorization` | replaced |
| Presigned URLs | `X-Amz-Signature`, `X-Amz-Security-Token`, SigV2 `Signature` replaced; `X-Amz-Credential`, `AWSAccessKeyId` shortened |
| Error bodies | `<SignatureProvided>`, `<StringToSign…>`, `<AWSAccessKeyId>`, tokens replaced |

"Copy as curl" never carries a signature: it prints the request with curl's `--aws-sigv4` option.
