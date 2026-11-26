# Credentials

Where an endpoint's credentials come from (`credentials` in [config.toml](configuration.md#configtoml)). They are
cached until five minutes before expiry; static keys can be set only in `config.toml`. What the interface sees of them:
[security](security.md#credential-isolation).

| `kind` | Source |
|---|---|
| `default` | The AWS default chain, in order: `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` / `AWS_SESSION_TOKEN`; web identity (`AWS_WEB_IDENTITY_TOKEN_FILE` + `AWS_ROLE_ARN`, IRSA on EKS); the profile in `AWS_PROFILE` (or `default`); container credentials (`AWS_CONTAINER_CREDENTIALS_RELATIVE_URI` / `_FULL_URI`: ECS, EKS Pod Identity); IMDSv2. The error lists each step's result. |
| `profile` | A profile in `~/.aws/config` / `~/.aws/credentials` (`AWS_CONFIG_FILE`, `AWS_SHARED_CREDENTIALS_FILE`): static keys; `credential_process`; IAM Identity Center (`sso_session` or legacy `sso_start_url`; uses the token `aws sso login` cached in `~/.aws/sso/cache`, sonde does not log in); `role_arn` with `source_profile` or `credential_source` (STS `AssumeRole`, `external_id`); `role_arn` with `web_identity_token_file`. |
| `env` | Environment variables you name. |
| `imds` | The instance role via IMDSv2 (`AWS_EC2_METADATA_SERVICE_ENDPOINT`, `AWS_EC2_METADATA_DISABLED`). |
| `web-identity` | `AssumeRoleWithWebIdentity` with a token file and role. |
| `process` | A command printing `credential_process` JSON. When it fails, sonde says how it ended (its exit status), never what it printed. |
| `static` | Keys in `config.toml` (for local servers). |
| `anonymous` | Unsigned requests (public buckets). |

STS calls go to `sts.<region>.amazonaws.com` (`.com.cn` in China regions) or `AWS_ENDPOINT_URL_STS`.

## Redaction

Every request record is redacted when created, before anything stores or shows it
(`crates/reqlog/src/redact.rs`), and again where a run's file is written, whoever made the record (an imported HAR
or log too):

| What | Done |
|---|---|
| `Authorization` | signature replaced; key id shortened (`AKIA…LE`); scope and signed headers kept |
| `x-amz-security-token`, `x-amz-s3session-token` (S3 Express), SSE-C key headers, `Cookie`, `Proxy-Authorization` | replaced |
| Presigned URLs | `X-Amz-Signature`, `X-Amz-Security-Token`, `X-Amz-S3session-Token`, SigV2 `Signature` replaced; `X-Amz-Credential`, `AWSAccessKeyId` shortened; the same in any text, any case |
| Error and request bodies | `<SignatureProvided>`, `<StringToSign>`, `<CanonicalRequest>` (signed headers' values) and their `…Bytes`, `<Signature>`, tokens (`<SecurityToken>`, `<SessionToken>`, `<Token>`), `<SecretAccessKey>` replaced; `<AWSAccessKeyId>`, `<AccessKeyId>` shortened; an element cut at the body's limit replaced to the end |

"Copy as curl" never carries a signature: it prints the request with curl's `--aws-sigv4` option. What a copied
command may hold of a run: [security](security.md#copied-commands).
