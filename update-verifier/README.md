# orb-update-verifier

Checks general system health and manages the slot and rootfs state of the Orb.
It is designed to run as systemd oneshot service that will run once on boot.

## Testing

Health test can be forced by setting environment variable `UPDATE_VERIFIER_DRY_RUN`.

```sh
$ sudo UPDATE_VERIFIER_DRY_RUN="1" ./update-verifier
```
