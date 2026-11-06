# Credentials

Credentials are resolved by sonde's Rust core (`crates/sonde-core/src/credentials`), cached until five minutes
before they expire, and used only to sign requests. The TSX interface never sees them: it gets endpoint ids,
names and a description of the source ("profile prod", "instance role (IMDSv2)"). There is no API through which
JavaScript can read a key, and the endpoint form cannot set one (static keys live only in `config.toml`).

| `kind` | Where from |
|---|---|
| `default` | The AWS default chain, in order: `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` / `AWS_SESSION_TOKEN`; web identity from the environment (`AWS_WEB_IDENTITY_TOKEN_FILE` + `AWS_ROLE_ARN`: IRSA on EKS); the profile in `AWS_PROFILE` (or `default`); container credentials (`AWS_CONTAINER_CREDENTIALS_RELATIVE_URI` / `_FULL_URI`, ECS and EKS Pod Identity); IMDSv2. The error lists what each step said. |
| `profile` | A profile of `~/.aws/config` and `~/.aws/credentials` (`AWS_CONFIG_FILE`, `AWS_SHARED_CREDENTIALS_FILE`): static keys; `credential_process`; IAM Identity Center (`sso_session` or legacy `sso_start_url`: the token `aws sso login` cached in `~/.aws/sso/cache` is exchanged for role credentials — sonde does not run the login); `role_arn` with `source_profile` or `credential_source` (STS `AssumeRole`, `external_id`); `role_arn` with `web_identity_token_file`. |
| `env` | Environment variables you name. |
| `imds` | The instance role through IMDSv2 (a PUT token, then the role's credentials; `AWS_EC2_METADATA_SERVICE_ENDPOINT`, `AWS_EC2_METADATA_DISABLED`). |
| `web-identity` | `AssumeRoleWithWebIdentity` with a token file and role. |
| `process` | A command printing `credential_process` JSON. |
| `static` | Keys in `config.toml` (for local servers). |
| `anonymous` | Unsigned requests (public buckets). |

STS calls go to `sts.<region>.amazonaws.com` (`.com.cn` in China regions) or `AWS_ENDPOINT_URL_STS`.

## Redaction

Every request record is redacted when it is created, before the ring buffer, SQLite, exports, the inspector or a
probe report can see it (`crates/reqlog/src/redact.rs`):

- `Authorization`: the signature is replaced; the key id is shortened (`AKIA…LE`); scope and signed headers stay;
- `x-amz-security-token`, the SSE-C key headers, `Cookie`, `Proxy-Authorization`: replaced;
- presigned URLs: `X-Amz-Signature`, `X-Amz-Security-Token`, SigV2 `Signature` replaced, `X-Amz-Credential` and
  `AWSAccessKeyId` shortened;
- error bodies: `<SignatureProvided>`, `<StringToSign…>`, `<AWSAccessKeyId>`, tokens.

"Copy as curl" never carries a signature: it prints the request and the `--aws-sigv4` option curl signs with.
