# syntax=docker/dockerfile:1.4
FROM mcr.microsoft.com/devcontainers/rust:latest

RUN apt-get update && export DEBIAN_FRONTEND=noninteractive \
    && apt-get -y install libudev-dev libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev libasound2-dev protobuf-compiler libsquashfs-dev libclang-dev

ADD Seek_Thermal_SDK_4.1.0.0.zip /tmp/
ADD Seek_Thermal_SDK_4.1.0.0.zip.sha256sum /tmp/

# Check that we got the same version of the Seek Thermal SDK as expected
RUN cd /tmp/ && sha256sum -c /tmp/Seek_Thermal_SDK_4.1.0.0.zip.sha256sum

RUN <<EOF
    machine=$(gcc -dumpmachine)
    unzip /tmp/Seek_Thermal_SDK_4.1.0.0.zip "Seek_Thermal_SDK_4.1.0.0/$machine/*" -d /tmp/
    cd "/tmp/Seek_Thermal_SDK_4.1.0.0/$machine/"
    mv include/* /usr/include/
    mv lib/* /lib/x86_64-linux-gnu/
    cd /
    rm -rf /tmp/Seek_Thermal_SDK_4.1.0.0 /tmp/Seek_Thermal_SDK_4.1.0.0.zip
EOF
