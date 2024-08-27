# Initial repo setup

To be able to build the code, there is some first-time setup required.

## Set up nix + direnv (the developer environment)

We use [nix][zero-to-nix] to manage all of the dependencies during development.
While most of the software can use convnetional rust tools like cargo, we do
have a few additional dependencies. Instead of apt-installing them, we use nix
as the package manager, and use [direnv][direnv] to handle automatically
activating a developer shell. The process to install and configure these two
tools is as follows:

1. [Install nix][install nix]. This works for both mac and linux, if you are
   using a windows machine, you must first set up [WSL2][WSL2].
2. Ensure that you have these lines in your `~/.config/nix/nix.conf` or
   `/etc/nix/nix.conf`. This is done automatically by the above installer: 
   ```
   experimental-features = nix-command flakes
   max-jobs = auto
   ```
   You can check that things work by running `nix run nixpkgs#hello`
3. Install direnv: `nix profile install nixpkgs#direnv`
4. [Hook direnv](https://direnv.net/docs/hook.html) into your shell.
5. Set up your personalized .envrc file by running `cp .envrc.example .envrc`.
   You can customize this file if you wish. We recommend filling in your cachix
   token if you have one. If prompted, dont run `direnv allow` yet, follow step
   6 first. Otherwise you'll get a bunch of errors.
6. Follow the instructions on vendoring proprietary SDKs in the subsequent
   section.
7. Run `direnv allow` in the repository's root directory. Direnv will then
   automatically use the .envrc file you set up any time you `cd` into the
   directory.
8. If you are on macos, run the following:
   ```bash
   brew install dbus
   brew services start dbus
   ```

## Vendoring proprietary SDKs

Although all of Worldcoin's code in the orb-software repo is open source, some of the
sensors on the orb rely on proprietary SDKs provided by their hardware vendors.
Luckily, these are accessible without any cost, they are just annoying to get and
are not themselves open source.

To get started, you will need to download these SDKs. The process for this
depends on if you are officially affiliated with Worldcoin.

### If you have access to Worldcoin private repos

1. Create a [personal access token][pac] from github to allow you to use
   private git repos over HTTPS.
2. Append the following to your `~/.config/nix/nix.conf`:
   ```
   access-tokens =
   github.com=github_pat_YOUR_ACCESS_TOKEN_HERE
   ```
3. Test everything works so far by running `nix flake metadata
   github:worldcoin/priv-orb-core`. You should see a tree of info. If not, you
   probably don't have your personal access token set up right - post in slack
   for help.

### If you don't have access to Worldcoin private repos

1. Go to [https://developer.thermal.com][seek dev page] and create a developer
   account. Getting the SDK can take several days for Seek Thermal to approve
   access. In the meantime, you can skip steps 2 and 3.
2. Download the 4.1.0.0 version of the SDK (its in the developer forums).
3. Extract its contents, and note down the dir that *contains* the
   `Seek_Thermal_SDK_4.1.0.0` dir.
4. modify your `.envrc` like this: `use flake --override-input seekSdk
   "PATH_FROM_STEP_3"`. If you don't yet have access to the SDK, just provide
   a path to an empty directory.

[WSL2]: https://learn.microsoft.com/en-us/windows/wsl/install
[direnv]: https://direnv.net/
[install nix]: https://zero-to-nix.com/start/install
[pac]: https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens#creating-a-fine-grained-personal-access-token
[seek dev page]: https://developer.thermal.com/
[zero-to-nix]: https://zero-to-nix.com
