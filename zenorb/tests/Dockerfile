FROM debian:bookworm-slim

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    ca-certificates \
    wget \
    unzip \
    && rm -rf /var/lib/apt/lists/*

RUN wget -O /tmp/zenoh.zip "https://www.eclipse.org/downloads/download.php?file=/zenoh/zenoh/1.7.2/zenoh-1.7.2-x86_64-unknown-linux-gnu-debian.zip&r=1" && \
    unzip /tmp/zenoh.zip -d /tmp/zenoh && \
    dpkg -i /tmp/zenoh/zenohd_1.7.2_amd64.deb /tmp/zenoh/zenoh-plugin-storage-manager_1.7.2_amd64.deb || true && \
    rm -rf /tmp/zenoh.zip /tmp/zenoh

COPY zenohd.json5 /etc/zenohd.json5

CMD ["zenohd", "--config=/etc/zenohd.json5"]
