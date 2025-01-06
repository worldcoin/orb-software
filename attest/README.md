# Attestation for authorization

This directory contains source code for a systemd service to retrieve an attestation token for the orb. The service runs on the orb. It uses [secure element](https://github.com/worldcoin/orb-software/tree/main/orb-secure-element) to get attestation token from the backend services. That attestation token is exposed via DBUS to other services (like [orb-core](https://github.com/worldcoin/orb-software/tree/main/orb-core#orb-core)).


## Summary

All communications between the Orb and the Worldcoin Cloud Services must be authorized. Only an orb should be able to interact with backend services. In order to enforce this authorization, the orb uses an [HTTP Authorization bearer token](https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Authorization) in the header of every request.  The purpose of this document is to define how and under what circumstances such a token must be delivered to an orb for subsequent requests, and the security requirements that ensure that all requests to backend services originate from an authorized party. Attestation allows us to verify that a request is coming from a legitimate and non-counterfeit Orb and is backed with hardware secrets only accessible by SE050.

The Worldcoin attestation service is an externally facing API that Orbs use to get a token for authorization on other Worldcoin services.

## Design requirements for auth tokens

The auth token should have the following properties:

- Auth Tokens are issued by the backend only after an Orb demonstrates that it possesses the SE050 attestation key.
- Token expiration time is 6-8 hours
- Orb can have multiple valid tokens at the same time
- Only one token can be issued for a single correctly signed challenge. (protection against replay-challenge attacks)
- An auth-token should give the orb sufficient entitlements to access all of the normally required services
- An orb should be able to request a new auth-token at any time


An Orb can demonstrate that it is still in possession of the SE050 attestation key, by signing challenge (some opaque data provided by the backend) and then the backend can verify the signature using a public key stored in the DB.

# Design

When an Orb boots it has no auth token, thus it needs to get one. To get a token, the orb requests a \<challenge\> from the Auth backend, signs it with the attestation key and returns to the backend, which verifies the signature and, if satisfied, issues a new token. That process is depicted in the diagram below. We assume that the Auth service has read-only access to the database with Orb public keys and the backend itself is stateless. How keys are getting into the attestation database is described in https://www.notion.so/worldcoin/Factory-Key-Reporting-fe5c80b697304998b868f7a63cf15d7b

``` mermaid
sequenceDiagram
    participant db as Public key attestation DB
    participant auth as Auth Backend service
    participant orb as Orb
    participant se050 as SE050
    orb ->> auth: I'm orb XXXXX<br/>give me a challenge
    auth ->> orb: your challenge is <challenge>
    orb ->> se050 : sign <challenge>
    se050 ->> orb: signed, here is <signature>
    orb ->> auth: my challenge is <challenge><br/>signature is <signature>
    auth ->> db: get attestation public key for orb XXXXX
    db ->> auth: attestation pubkey is <orb pubkey>
    auth ->> auth: verify signature of the <challenge> with <orb pubkey>
    auth ->> orb: here is your <auth token>
```
fig1: Sequence Diagram of getting an auth token

## Protocol

Note: The implementation example uses random tokens as both the Attestation challenge as well as the longer-lived token. This is only a backend implementation detail, for the orb the challenge and the token are opaque.

### Orb side

From the Orb side, to get an attestation token it needs to run two HTTP requests.

1\. Request a challenge from the backend. challenge is a chunk of data, which is unique for each challenge request but opaque for the Orb.

   In the example, it is implemented by 'getChallenge' function

   Example of request

   ``` json
   {"orbId": "orb1aaaaaaaaaaaaaaaaaaaaaaaaaaaa"}
   ```

   Example of response:

   ``` json
    {"challenge": "eyJhbGciOiJ ... 5elDru0RSg",
    "duration": 120,
    "expiryTime": "12:00:00 12 June 2022"}
   ```

2\. Orb signs the Challenge using a keys from the SE and then sends the Challenge and the signature to the backend

   Example of request

   ``` json
   {"orbId": "orb1aaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "challenge": "eyJhbGciOiJ ... 5elDru0RSg",
    "signature" : "MFkwEwY ... FXIZTex"}
   ```

   Example of response:

   ``` json
   {"token": "eyJhbGciOd7U ... mElfGdwlrzg",
    "duration": 28800,
    "expiryTime": "20:00:00 12 June 2022",
    "startTime": "12:00:00 12 June 2022"}
   ```

That token is used by Orb for any consequent request to other backend services. 'Duration' specifies the number of seconds after the token is issued when it is considered valid. Orb should try fetching a new token before that moment. ‘expiryTime’ specifies the absolute time when the token expires; current implementations of the orb software do not use it.

### Backend

The Auth service service runs separately from other services.

The Auth service has access to the orb attestation keys DB which maps orbid into public key. This database contains keys [collected during Orb provisioning](https://github.com/worldcoin/orb-se050-public-keys)

The backend provides two endpoints: `tokenchallenge` and `token`.

#### tokenchallenge endpoint

This endpoint takes an orbid as input and issues a short-lived (a few minutes) challenge-token.

The challenge-token \*must\* be:
\- short lived (a few minutes)
\- time-gated (there must be some mechanism that causes the token to be invalid after some time period)
\- contain some non-repeating data(nonce) in it. This could be a timestamp or random data.
\- validated only once such that subsequent attempts to use the same challenge-token will be rejected

The current implementation of the Auth-service uses randomly generated strings and stores them in a Mongo DB.

#### token endpoint

This endpoint takes the challenge-token and its signature as input and generates a long-lived (a few hours) token.

This endpoints:
1\. Finds the challenge token in the Mongo DB (should be saved there by tokenchallenge endpoint)
2\. Finds public key of the orb in question
3\. Validates signature of the challenge

If everything is ok, the endpoint generates a long-lived token and returns it to the Orb.

#### Other endpoints

Other endpoints must verify the orb authentication by checking the value of the ‘Authorization’ header and de-coding the token stored there. Other endpoints must verify the expiration date of the token as well as signature. The signature must be verified against persistent public keys.

## Disabling Backend access

If needed, orb’s access to the backend could be revoked at any time, that could be done in two places:

1. Mark public keys in the DB as inactive, that will prevent the orb from getting a new token but currently issued tokens will keep working until they expire
2. Invalidate currently issued tokens. That will immediately deny the orb all access to the backend but it could get a new valid token.

Depending on what is required one or both actions could be taken.

## Chain of trust for public keys (AKA why we trust public keys in the DB)

Public keys extracted from Orbs should *not* be trusted unless it could be proven that they come from a genuine SE050. NXP provides a way to cryptographically verify that the key materials extracted from SE050 are not replaced/modified, That is implemented via chat of cryptographically verified certificates (see chapter 3.11 in [AN12436](https://www.nxp.com/docs/en/application-note/AN12436.pdf)). The chain starts from a publicly available NXP [root](https://www.gp-ca.nxp.com/CA/getCA?caid=63709315050002) and [intermedeate](https://www.gp-ca.nxp.com/CA/getCA?caid=63709315060003) certificates and extends to all key materials extracted from the SE050. The attestation pubkeys are uploaded from the orb to the backend during manufacturing, together with die uniqueue SE050 certificate and pubkeys signature, thus it is possible to verify that the attestation pubkey comes from a SE050 with specific chipid.

See the figure below showing the chain of certificates we are using.

``` mermaid
flowchart TB
    subgraph NXP
    Root["NXP RootCAvE506"]
    Intermediate["NXP Intermediate-AttestationCAvE206"]
    Root --> Intermediate
    end
    subgraph "Unique per die"
    SE_Cert["Die unique certificate"]
    Intermediate --> SE_Cert
    chipid["Chip ID"]
    SE_Cert --> chipid 
    end
    subgraph "TFH generated keys"
    signup_pubkey["Signup pubkey"]
    SE_Cert --> signup_pubkey
    attest_pubkey["Attestation pubkey"]
    SE_Cert --> attest_pubkey
    end
```

In day-to-day work, only `Signup pubkey` and `Attestation pubkey` are used, the rest of key materials are preserved in a separate database with lower performance requirements.

### Verifying of SE050 Attestation pubkey certificate.

Each secure element has a die specific x509 Certificate ("SE050 Attestation pubkey certificate") signed by NXP. It could be verified using openssl x509 tools.

### Signup pubkey and Attestation pubkey

Public keys are read from SE050 using "read with attestation". "Read with attestation" is a special read command, which, in addition to the public key itself, returns some metadata and a cryptographic signature.
The metadata contains information about key type, key ID, die-unique chip id, etc. Concatenated public key and the metadata are signed with "SE050 Attestation pubkey certificate", thus completing the chain of trust. The metadata is stored in the database as an opaque ‘extra\_data’ field.

### Chip ID

Chip ID is a die-unique chip identifier, it is read using "read with attestation" and its metadata and signature are stored in a database.

## Use of token by application running on the orb

The auth-token is used for all backend calls and multiple services on the Orb need it, to avoid each services re-implementing fetching the token, we need a daemon which gets the token from auth-service and passes it down to orb-core, update-agent and other services.

This daemon:
\- Fetches the token right after boot
\- Provides Dbus API for other services to get a token
\- Provides Dbus API to force refresh the token
\- Does not store the token in persistent storage
\- Take care of refreshing the token before it expires

The daemon has a Dbus API, which exposes API for in-orb services to get the currently working token.

## Recommended Reading

- [RFC 2617 \- Original RFC on HTTP Authentication](https://www.rfc-editor.org/rfc/rfc2617)
	In particular look at the ["Security Considerations" section](https://www.rfc-editor.org/rfc/rfc2617#section-4). It has details on MITM, Replay Attacks, and the considerations around different Nonce techniques.
- [RFC 6750 \- OAUTH 2.0 Bearer Token Usage](https://www.rfc-editor.org/rfc/rfc6750)
	While we are likely *not* implementing OAUTH 2.0, the patterns described in the RFC are considered to be secure. This RFC describes the use of Bearer tokens which is the easiest way to implement the kind of security we want on the backend.
- [API Tokens: A Tedious Survey](https://fly.io/blog/api-tokens-a-tedious-survey)
	This has been linked and discussed many times at Tools For Humanity


