### New Features

- `idx stockbit login` takes the `credentialStorage` cookie pasted in the terminal while its local page waits, so signing in from a remote terminal needs no port forward. (#0)

### Bug Fixes

- `idx stockbit login` on a machine with no display now says how to reach its page from another machine (the `ssh -L` forward, or the paste), which it only did over SSH. (#0)
- The `ssh -L` forward that `idx stockbit login` and `stockbit_login` suggest names this host when `HOSTNAME` is not exported, where it said `<this host>`. (#0)

### Output Changes

- `stockbit_login` and `stockbit_status` answer `"remote": true` for a page sign-in on a machine with no display, as they did over SSH. (#0)
