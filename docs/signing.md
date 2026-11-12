# Local signing

Check for a valid Apple Development identity with `security find-identity -v -p codesigning`. If none exists, sign in with an Apple ID in Xcode Settings → Accounts, select the account, open Manage Certificates and add an Apple Development certificate. A free account is sufficient; no paid membership or notarisation is required.

Copy `Config/Local.xcconfig.example` to `Config/Local.xcconfig` and set your team ID. The local file is ignored by Git. Xcode uses automatic signing. Select your personal team in Xcode if necessary. CI disables signing.

Use the same development identity across rebuilds: ad-hoc signatures change with each build, causing macOS to reset Accessibility, Screen Recording and Microphone grants. Unnotarised downloads may require right-click → Open on first launch.
