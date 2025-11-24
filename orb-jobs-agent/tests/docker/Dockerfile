FROM debian:bullseye-slim

# Install fsck and filesystem tools
RUN apt-get update && apt-get install -y \
    e2fsprogs \
    && rm -rf /var/lib/apt/lists/*

COPY ./check-my-orb /usr/sbin/
COPY ./check-my-orb_output.txt /root/

COPY ./orb-mcu-util /usr/sbin/
COPY ./shutdown /usr/sbin/

CMD ["sleep", "infinity"]
