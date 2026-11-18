# MinIO built from its source: it no longer publishes images anyone may pull.
# The pin is its last commit (the community edition is in maintenance).
FROM golang:1.25-alpine AS build
ARG MINIO_VERSION=v0.0.0-20260212201848-7aac2a2c5b7c
RUN CGO_ENABLED=0 go install "github.com/minio/minio@${MINIO_VERSION}"

FROM alpine:3.24
COPY --from=build /go/bin/minio /usr/local/bin/minio
ENTRYPOINT ["minio"]
