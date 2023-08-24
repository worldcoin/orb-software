# orb-software
Open source software for [the orb](https://worldcoin.org/blog/engineering/opening-orb-look-inside-worldcoin-biometric-imaging-device).

## Contributing

We plan to accept contributions at a later date, but do not have bandwidth to review PRs
currently. 

Likewise, we are providing this source code for the benefit of the community, but cannot
commit to any SemVer or API stability guarantees. Be warned: we may change things in a
backwards-incompatible way at any time!

## First time Setup

1. [Install nix][nix]. This works for both mac and linux, windows is not supported.
2. Create a [personal access token][PAC] (classic) from github to allow you to use private git repos over HTTPS.
3. Ensure that you have these lines in your `~/.config/nix/nix.conf`:
```
experimental-features = nix-command flakes
max-jobs = auto
access-tokens = github.com=ghp_PUT_YOUR_PERSONAL_ACCESS_TOKEN_FROM_GITHUB_HERE
```
4. Test everything works so far by running `nix flake metadata github:worldcoin/orb-core`. You should see a tree of info. If not, you probably don't have your personal access token set up right - post in #public-orb-software on slack for help.
5. Install direnv: `nix profile install nixpkgs#direnv`
6. [Hook direnv](https://direnv.net/docs/hook.html) into your shell.
7. Tell direnv to use the nix flake with `cp .envrc.example .envrc`. You can customize this file if you wish. We recommend filling in your cachix token if you have one - if you are a team member, you can get this from 1Password.
8. Run `direnv allow` in the repository's root directory.

## Building

We use `cargo zigbuild` for most things. The following cross-compiles a binary
in the `foobar` crate to the orb:
```bash
cargo zigbuild -p foobar
```

## License
**NOTE: The following text will be used when we open source. Its not open sourced yet.**

> Unless otherwise specified, all code in this repository is dual-licensed under either:
> - MIT License ([LICENSE-MIT](LICENSE-MIT))
> - Apache License, Version 2.0, with LLVM Exceptions ([LICENSE-APACHE](LICENSE-APACHE))
>
> at your option. This means you may select the license you prefer to use.
>
> Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion
> in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above,
> without any additional terms or conditions.

[nix]: https://nixos.org/download.html
[PAC]: https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens#creating-a-personal-access-token-classic
