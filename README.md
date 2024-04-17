# orb-software

Open source software for [the orb][inside-orb].

![A wireframe expansion of the orb](docs/orb-wireframe.png)

## Repository structure

For the most part, every toplevel directory is a separate software component.
We also link to some other public repositories, to provide a unified view of
the orb's software. The most important applications on the orb are as follows:

- [orb-attest](orb-attest): Talks with the secure element to generate an
  attestation token for the signup backend service.
- [orb-core](https://github.com/worldcoin/orb-core): The core signup logic and
  sensor management of the orb.
- [orb-firmware](https://github.com/worldcoin/orb-firmware): The firmware for
  the orb's microcontrollers (MCUs). This excludes the firmware that runs on
  the security MCU.
- [orb-messages](https://github.com/worldcoin/orb-messages): Schemas for
  messages sent between the Jetson and the MCU.
- [orb-secure-element](https://github.com/worldcoin/orb-secure-element): Code
  that interacts with the orb's secure element - a dedicated security hardened
  chip that provides a hardware root of trust. Provides important signing
  functionality.
- [orb-ui](orb-ui): Daemon that manages the UI/UX of the orb.
- [open-iris](https://github.com/worldcoin/open-iris): The iris recognition
  inference system.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Unless otherwise specified, all code in this repository is dual-licensed under
either:

- MIT License ([LICENSE-MIT](LICENSE-MIT))
- Apache License, Version 2.0, with LLVM Exceptions
  ([LICENSE-APACHE](LICENSE-APACHE))

at your option. This means you may select the license you prefer to use.

Any contribution intentionally submitted for inclusion in the work by you, as
defined in the Apache-2.0 license, shall be dual licensed as above, without any
additional terms or conditions.

[inside-orb]: https://worldcoin.org/blog/engineering/opening-orb-look-inside-worldcoin-biometric-imaging-device

