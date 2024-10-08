# orb-attest (formerly orb-short-lived-token-daemon)

Systemd service to retrieve the current attestation token for the backend.

The token is generated by:
* Requesting a challenge from the backend
* Signing the challenge with the secure element
* Sending the signed challenge back to the backend
* Getting a token on success.

## Busctl commands

To fetch the auth token from the command line via dbus, run the following:
```bash
busctl call --address=unix:path=/tmp/worldcoin_bus_socket org.worldcoin.AuthTokenManager1 /org/worldcoin/AuthTokenManager1 org.freedesktop.DBus.Properties Get ss "org.worldcoin.AuthTokenManager1" "Token"
```
