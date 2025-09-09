FROM debian:bullseye-slim

COPY ./check-my-orb /usr/sbin/
COPY ./check-my-orb_output.txt /root/

COPY ./orb-mcu-util /usr/sbin/
COPY ./shutdown /usr/sbin/

CMD ["sleep", "infinity"]
