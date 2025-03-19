# Location Crates

These crates facilitate location detection on the orb.

There is one daemon:
- [`orb-location`](daemon/): Daemon that contunuously monitors for location and reports it to
  our backend.

Two are libraries:
- [`orb-cellcom`](cellcom/): Lower level interface to the Modem
- [`orb-google-geolocation-api`](google-geolocation-api/): Wrappers 
