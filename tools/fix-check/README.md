# What the fix code is compiled against

`cargo test -p sonde-analysis --test fix_code` goes over every piece of fix code sonde prints (`fixes::catalogue`). With
nothing installed it checks that each closes its brackets and strings, and parses Rust's. With `python3`, `node`,
`gofmt` and a JDK it parses each with its language's own parser and runs the fixes whose meaning matters against a
small client of its own. With the SDKs here it compiles each against them, at the versions pinned in this folder:

| | SDK | Fetch it (once, with the network) | The test then runs |
|---|---|---|---|
| `go/` | aws-sdk-go-v2, errgroup | `go mod download` | `go build`, `go run` (offline: `GOPROXY=off`) |
| `java/` | AWS SDK for Java 2 | `mvn -q dependency:go-offline dependency:build-classpath` | `javac` (`mvn -o` names the jars) |
| `node/` | AWS SDK for JavaScript 3, p-limit, TypeScript | `npm ci` | `tsc --noEmit` |
| `rust/` | aws-sdk-s3, object_store | `cargo fetch` | `cargo check --offline` (several minutes the first time) |
| | boto3 | `pip install boto3` | each fix on boto3's own client, its transport answered by the test |

The test never reaches the network. What is not here is passed over with a line on stderr (`fix_code: SKIPPED …`);
with `SONDE_FIXES_STRICT=1` it is a failure.
