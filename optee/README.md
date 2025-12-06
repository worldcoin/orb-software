# OP-TEE Client Apps (CAs) and Trusted Apps (TAs)

The home for all rust FOSS OP-TEE CAs and TAs. Does not contain the OS or supplicant.

## Troubleshooting

- If Uuid::parse_str() returns an InvalidLength error, there may be an extra
  newline in your uuid.txt file. You can remove it by running
  `truncate -s 36 uuid.txt`.
