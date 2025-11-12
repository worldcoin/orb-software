FROM debian:bullseye-slim

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    ca-certificates \
    dbus \
    network-manager \
    util-linux && \
    rm -rf /var/lib/apt/lists/*

RUN mkdir -p /run/integration-tests

COPY dbus.conf /etc/dbus.conf
COPY NetworkManager.conf /etc/NetworkManager/NetworkManager.conf
COPY docker-entry.sh /docker-entry.sh
RUN chmod +x /docker-entry.sh

ENV DBUS_VERBOSE=1
ENV DBUS_SYSTEM_BUS_ADDRESS=unix:path=/run/integration-tests/socket

CMD ["/docker-entry.sh"]
