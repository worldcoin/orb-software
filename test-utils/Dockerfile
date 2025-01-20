FROM debian:bullseye-slim

RUN apt-get update && \
  apt-get install -y --no-install-recommends \
  dbus 

RUN mkdir -p /run/integration-tests

COPY dbus.conf /etc/dbus.conf

ENV DBUS_VERBOSE=1

CMD ["dbus-daemon", "--nofork", "--config-file=/etc/dbus.conf", "--print-address"]

