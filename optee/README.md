# OP-TEE Client Apps (CAs) and Trusted Apps (TAs)

The home for all rust FOSS OP-TEE CAs and TAs. Does not contain the OS or
supplicant.

## Build instuctions

### How to build CAs

Build them like any other regular binary in the *toplevel* workspace. Note that
unlike most of the rest of the codebase, only aarch64-unknown-linux-gnu is a
supported target.

### How to build TAs

You must pass `RUSTC_BOOTSTRAP=1` in front of all your cargo commands to use
some necessary nightly features.

Alternatively, you can call `cargo x optee ta build -p <your_optee_package>`

### How to sign TAs

`cargo x optee ta sign -p <your_optee_package>`. Note that this assumes you have set up
an aws profile called `trustzone-stage` or `trustzone-prod`. Try adding this to your
`~/.aws/config` directory:

```ini
[profile trustzone-stage]
sso_session = my-sso
sso_account_id = 510867353226
sso_role_name = PowerUserAccess
region = eu-central-1

[profile trustzone-prod]
sso_session = my-sso
sso_account_id = 573252405782 
sso_role_name = PowerUserAccess
region = eu-central-1

[sso-session my-sso]
sso_start_url = https://d-90676ede48.awsapps.com/start/#
sso_region = us-east-1
sso_registration_scopes = sso:account:access
```

## Troubleshooting

- If Uuid::parse_str() returns an InvalidLength error, there may be an extra
  newline in your uuid.txt file. You can remove it by running
  `truncate -s 36 uuid.txt`.
- TAs do not share the top-level cargo workspace, but CAs do. For this reason,
  to get your LSP to work for TAs, you need to open your editor in the `optee`
  directory instead of the regular toplevel directory. The two cargo workspaces
  are mutually exclusive so you may have to switch betweeen two instances of
  vscode / LSPs. 
