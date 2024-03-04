# Security @ Worldcoin

Security is an essential part of Worldcoin, and we take it very seriously. From the beginning, we have incorporated secure design and applied best practices across the project. However, security doesn’t just end with us. Security audits by third-party firms and research by community members are critical to establishing a solid foundation. If you have identified a security concern, we encourage you to submit your findings to our Product Security Incident Response Team (PSIRT) — your findings may even qualify for a reward!

## Think you’ve found a security issue?

We accept vulnerability reports in two ways: through our public bug bounty program or through our security disclosure email. **Please note that only reports submitted through the HackerOne bug bounty program may be eligible for bounties.**

### 1. (Preferred, bounty-eligible) [Tools For Humanity Bug Bounty Program](https://hackerone.com/toolsforhumanity)

_Tools For Humanity_ manages the bug bounty program on behalf of Worldcoin through the HackerOne platform. Reports submitted to this program may be eligible for rewards as governed by the bug bounty policy. **This repository is [within scope](https://hackerone.com/toolsforhumanity/policy_scopes) for the bounty program.**

### 2. (Not bounty-eligible) Security mailing list `security@worldcoin.org`

We also accept vulnerability reports in multiple languages via email at [security@worldcoin.org](mailto:security@worldcoin.org). If reporting via email, please encrypt the content and attachments using Worldcoin PSIRT’s PGP Key (please see [https://worldcoin.org/pgp-key.txt](https://worldcoin.org/pgp-key.txt) or check [https://worldcoin.org/.well-known/security.txt](https://worldcoin.org/.well-known/security.txt) for more info). The current fingerprint for this ECC curve25519 key is `45FE 2A09 90DD 7B04 EE51 1FEB 5F22 D67F 2C43 B48D`.

Please include the following details in your report (if applicable):

- Product/Repository and version/branch that contains the vulnerability
- Type of vulnerability
- Security impact of the vulnerability
- Instructions for reproduction
- Proof-of-concept (POC) demonstrating the vulnerability

### Issues in third-party libraries or modules

Please report security bugs in third-party software to the original maintainers.

### Responsible Disclosure

We believe that responsible disclosure is a net benefit for the community and subsequently encourage researchers to publish their findings after the issues have been remediated. We do ask, however, that you allow sufficient time for patches to be deployed globally, so please coordinate with Worldcoin PSIRT prior to publishing, either through the bug bounty program or over email. For more information on responsible disclosure, please see Google Project Zero’s [Vulnerability Disclosure policy](https://googleprojectzero.blogspot.com/p/vulnerability-disclosure-policy.html) as an example.
