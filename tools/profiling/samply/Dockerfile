FROM debian:bookworm-slim@sha256:abd67ffcfa541b485a3dff59865ab629aa048a6c613e639d36e7456b0b229241

ARG TARGETARCH
ARG SAMPLY_VERSION=0.13.1

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl xz-utils \
    && case "$TARGETARCH" in \
         arm64) archive=samply-aarch64-unknown-linux-gnu.tar.xz; \
                checksum=aa465162b62830168775b7ff4804bc35049436dcbc29bb3d1ea9f580380ea06a ;; \
         amd64) archive=samply-x86_64-unknown-linux-gnu.tar.xz; \
                checksum=61875daad67888798690dea3cb2748279df6ac299c5c6a857d67eed7642473d9 ;; \
         *) echo "unsupported architecture: $TARGETARCH" >&2; exit 2 ;; \
       esac \
    && curl --fail --location --retry 3 \
         "https://github.com/mstange/samply/releases/download/samply-v${SAMPLY_VERSION}/${archive}" \
         --output "/tmp/${archive}" \
    && echo "${checksum}  /tmp/${archive}" | sha256sum --check --strict \
    && folder="${archive%.tar.xz}" \
    && tar -xJf "/tmp/${archive}" -C /usr/local/bin \
         --strip-components=1 "${folder}/samply" \
    && chmod 0755 /usr/local/bin/samply \
    && rm -f "/tmp/${archive}" \
    && apt-get purge -y --auto-remove curl xz-utils \
    && rm -rf /var/lib/apt/lists/*

COPY --chmod=0755 entrypoint.sh /usr/local/bin/samply-private

ENTRYPOINT ["samply-private"]
