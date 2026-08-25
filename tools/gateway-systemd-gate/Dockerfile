FROM ubuntu:24.04

ENV container=docker

RUN for attempt in 1 2 3 4 5; do \
        rm -rf /var/lib/apt/lists/*; \
        apt-get -o Acquire::Retries=5 update && break; \
        test "$attempt" != 5; \
        sleep 10; \
    done \
    && DEBIAN_FRONTEND=noninteractive apt-get -o Acquire::Retries=5 install -y --no-install-recommends \
        ca-certificates curl dbus-user-session iproute2 procps python3 systemd \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 2000 --gid operator --shell /bin/bash operator \
    && systemctl mask \
        dev-hugepages.mount \
        sys-fs-fuse-connections.mount \
        systemd-remount-fs.service

STOPSIGNAL SIGRTMIN+3
CMD ["/sbin/init"]
