FROM debian:bookworm-slim

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    ca-certificates \
    dbus \
    network-manager \
    util-linux \
    wget \
    unzip \
    && rm -rf /var/lib/apt/lists/*

RUN ZENOH_VERSION="1.7.2" && \
    DEB_ARCH=$(dpkg --print-architecture) && \
    if [ "$DEB_ARCH" = "arm64" ]; then \
        ARCH="aarch64"; \
    else \
        ARCH="x86_64"; \
    fi && \
    wget -O /tmp/zenoh.zip "https://www.eclipse.org/downloads/download.php?file=/zenoh/zenoh/${ZENOH_VERSION}/zenoh-${ZENOH_VERSION}-${ARCH}-unknown-linux-gnu-debian.zip&r=1" && \
    unzip /tmp/zenoh.zip -d /tmp/zenoh && \
    apt-get update && \
    printf '%s\n' '#!/bin/sh' 'exit 0' > /usr/local/bin/systemctl && chmod +x /usr/local/bin/systemctl && \
    dpkg -i /tmp/zenoh/zenohd_${ZENOH_VERSION}_${DEB_ARCH}.deb /tmp/zenoh/zenoh-plugin-storage-manager_${ZENOH_VERSION}_${DEB_ARCH}.deb || \
    apt-get install -f -y --no-install-recommends && \
    rm -f /usr/local/bin/systemctl && \
    rm -rf /var/lib/apt/lists/* /tmp/zenoh.zip /tmp/zenoh && \
    zenohd --version

RUN mkdir -p /run/integration-tests

COPY zenohd.json5 /etc/zenohd.json5

COPY dbus.conf /etc/dbus.conf
COPY NetworkManager.conf /etc/NetworkManager/NetworkManager.conf
COPY docker-entry.sh /docker-entry.sh
RUN chmod +x /docker-entry.sh

ENV DBUS_VERBOSE=1
ENV DBUS_SYSTEM_BUS_ADDRESS=unix:path=/run/integration-tests/socket

CMD ["/docker-entry.sh"]
