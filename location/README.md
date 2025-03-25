# Location Service

Gather data from wpa_supplicant and the Quectel EC25 module to resolve an accurate position estimate for the device.

Important docs:
- https://files.pine64.org/doc/datasheet/project_anakin/LTE_module/Quectel_EC25&EC21_QuecCell_AT_Commands_Manual_V1.1.pdf
- https://forums.quectel.com/t/neighbor-cell-cell-id/36317
- https://w1.fi/wpa_supplicant/devel/ctrl_iface_page.html

# Building

Setting up pkgconfig with cargo-zigbuild was annoying so instead we use containerized cargo-cross :smiling_face_with_3_hearts:

```
$ cargo test
$ docker build -t rust-cross .
$ docker run -v "$PWD":/workdir rust-cross cargo build --target aarch64-unknown-linux-gnu
```
