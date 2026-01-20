FROM debian:bullseye-slim

RUN curl -L https://download.eclipse.org/zenoh/debian-repo/zenoh-public-key | sudo gpg --dearmor --yes --output /etc/apt/keyrings/zenoh-public-key.gpg
RUN echo "deb [signed-by=/etc/apt/keyrings/zenoh-public-key.gpg] https://download.eclipse.org/zenoh/debian-repo/ /" | sudo tee -a /etc/apt/sources.list > /dev/null

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    ca-certificates \
    dbus \
    network-manager \
    util-linux && \
    zenoh && \
    rm -rf /var/lib/apt/lists/*

RUN mkdir -p /run/integration-tests

COPY zenohd.json5 /etc/zenohd.json5

COPY dbus.conf /etc/dbus.conf
COPY NetworkManager.conf /etc/NetworkManager/NetworkManager.conf
COPY docker-entry.sh /docker-entry.sh
RUN chmod +x /docker-entry.sh

ENV DBUS_VERBOSE=1
ENV DBUS_SYSTEM_BUS_ADDRESS=unix:path=/run/integration-tests/socket

CMD ["/docker-entry.sh"]
