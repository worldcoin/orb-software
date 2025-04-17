#!/usr/bin/env python3

import sys
import argparse
from cryptography.hazmat.primitives.asymmetric import ed25519

WLD_TAG = b"$WLD TO THE MOON"
TAG_LEN = len(WLD_TAG)
SIG_LEN_SIZE = 4  # 4 bytes for signature length

def load_raw_ed25519_private_key(path):
    with open(path, "rb") as f:
        raw_key = f.read()
    if len(raw_key) != 32:
        raise ValueError("Expected 32-byte Ed25519 private key")
    return ed25519.Ed25519PrivateKey.from_private_bytes(raw_key)

def read_and_strip_existing_signature(data):
    """If data ends with WLD_TAG and signature length, strip them."""
    if len(data) < TAG_LEN + SIG_LEN_SIZE:
        return data, False

    if data[-TAG_LEN:] != WLD_TAG:
        return data, False

    sig_len_bytes = data[-(TAG_LEN + SIG_LEN_SIZE):-TAG_LEN]
    sig_len = int.from_bytes(sig_len_bytes, "big")

    expected_total = len(data) - TAG_LEN - SIG_LEN_SIZE - sig_len
    if expected_total < 0:
        return data, False  # malformed

    return data[:expected_total], True

def main():
    parser = argparse.ArgumentParser(description="Sign and append signature to binary.")
    parser.add_argument("raw_private_key", help="Path to 32-byte raw Ed25519 private key")
    parser.add_argument("input_binary", help="Input binary file to sign")
    parser.add_argument("output_file", help="Output file with signature and tag appended")
    parser.add_argument("--clobber", action="store_true", help="Allow replacing existing signature if present")
    args = parser.parse_args()

    # Load private key
    private_key = load_raw_ed25519_private_key(args.raw_private_key)

    # Read input binary
    with open(args.input_binary, "rb") as f:
        data = f.read()

    original_data, already_signed = read_and_strip_existing_signature(data)

    if already_signed and not args.clobber:
        print("Error: File is already signed. Use --clobber to overwrite.")
        sys.exit(1)

    # Sign
    signature = private_key.sign(original_data)
    sig_len = len(signature)

    # Write output
    with open(args.output_file, "wb") as f:
        f.write(original_data)
        f.write(signature)
        f.write(sig_len.to_bytes(4, "little"))
        f.write(WLD_TAG)

    print(f"Signed {'(replaced existing signature)' if already_signed else ''}and saved to: {args.output_file}")

if __name__ == "__main__":
    main()
