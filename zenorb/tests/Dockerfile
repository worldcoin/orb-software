FROM debian:bookworm-slim

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    ca-certificates \
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
    dpkg -i /tmp/zenoh/zenohd_${ZENOH_VERSION}_${DEB_ARCH}.deb /tmp/zenoh/zenoh-plugin-storage-manager_${ZENOH_VERSION}_${DEB_ARCH}.deb || \
    apt-get install -f -y --no-install-recommends && \
    rm -rf /var/lib/apt/lists/* /tmp/zenoh.zip /tmp/zenoh && \
    zenohd --version

COPY zenohd.json5 /etc/zenohd.json5

CMD ["zenohd", "--config=/etc/zenohd.json5"]
