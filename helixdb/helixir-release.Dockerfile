ARG BASE_IMAGE
FROM ${BASE_IMAGE}

ARG SOURCE_URL
ARG REVISION
ARG VERSION
ARG ENGINE_REVISION
ARG SCHEMA_FINGERPRINT
ARG UPSTREAM_REVISION

LABEL org.opencontainers.image.source="${SOURCE_URL}" \
      org.opencontainers.image.revision="${REVISION}" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.licenses="AGPL-3.0-only" \
      io.helixir.engine-revision="${ENGINE_REVISION}" \
      io.helixir.schema-fingerprint="${SCHEMA_FINGERPRINT}" \
      io.helixir.upstream-revision="${UPSTREAM_REVISION}"
