# enject

Hide .env secrets from prAIng eyes.

(Note: this project was previously called enveil and has been renamed to enject)

AI coding tools like Claude Code, Copilot, Cursor, and others can read files in your project directory, which means a plaintext `.env` file is an accidental secret dump waiting to happen. This isn’t theoretical. It is a known issue that has happened to me several times (even after explicitly telling Claude not to peek in Claude Code’s settings.json file). `enject` solves this by ensuring plaintext secrets **never exist on disk at all**. Your `.env` file contains only symbolic references; the real values live in an encrypted local store and are injected directly into your subprocess at launch. 

This project is inspired by Filip Hric’s solution/[blog post](https://filiphric.com/dont-let-ai-read-your-env-files), which uses a similar concept leveraging 1Password. I wanted a self-contained solution that didn’t rely on a third party services giving rise to this solution. And yes, this project was built almost entirely with Claude Code with a bunch of manual verification and testing.

## Benefits and Caveats

This project is primarily designed to mitigate the known issue of AI/LLM tools accidentally reading `.env` secrets in your project. Additional benefits include preventing secret leakage if a `.env` is accidentally committed to a repository, the ability to share `.env` files containing references instead of plaintext secrets, and the option to share the encrypted store itself.                               
This project is not a silver bullet for preventing an AI agent from obtaining your secrets. For example, an agent can still write code (by accident or via prompt injection) that exfiltrates secrets to terminal output or a file at runtime. We strongly advise against relying on this tool, or `.env` files in general, to store production secrets

## How it works

Your `.env` file looks like this:

```
DATABASE_URL=en://database_url
STRIPE_KEY=en://stripe_key
PORT=3000
```

Technically it is safe to commit (maybe don’t do that, though), and more importantly: safe for any AI tools accidentally (or perhaps not-so-accidentally) snooping in on it.

When you run `enject run -- npm start`, it:

1. Prompts for your master password (never echoed, never in shell history)
2. Derives a 256-bit AES key from your password using **Argon2id** (64 MB memory, 3 iterations)
3. Decrypts the local store with **AES-256-GCM** — the store file is a 12-byte random nonce followed by authenticated ciphertext
4. Resolves every `en://` reference against the decrypted map
5. Zeroizes the key and password bytes from memory
6. Spawns your subprocess with the resolved values injected into its environment

The store file is a binary blob. Without the master password, it is indistinguishable from random noise. The nonce is freshly generated on every write, so AES-GCM nonce reuse is impossible. Any modification to the ciphertext — even a single flipped bit — causes authentication to fail and decryption to be refused.

---

## Installation

### Via cargo

```bash
cargo install enject
```

### From source

Requires [Rust](https://rustup.rs) 1.70+.

```bash
git clone https://github.com/greatscott/enject
cd enject
cargo build --release
```

The compiled binary is at `target/release/enject`. Install it once to a location on your `PATH` so you can run it from any project:

**macOS / Linux (bash or zsh)**

```bash
# Option A: ~/.local/bin (no sudo required, common on Linux)
mkdir -p ~/.local/bin
cp target/release/enject ~/.local/bin/

# Option B: /usr/local/bin (requires sudo, available system-wide)
sudo cp target/release/enject /usr/local/bin/

# Option C: ~/.cargo/bin (already on PATH if you used rustup)
cp target/release/enject ~/.cargo/bin/
```

If you used option A and `~/.local/bin` is not already on your `PATH`, add this to your shell config (`~/.zshrc`, `~/.bashrc`, or `~/.bash_profile`):

```bash
export PATH="$HOME/.local/bin:$PATH"
```

Then reload it:

```bash
source ~/.zshrc   # or ~/.bashrc
```

Verify it worked:

```bash
enject --version
```

### Per-project setup (run once per project)

The binary is installed globally — you never reinstall it. But each project gets its own encrypted store:

```bash
cd your-project
enject init
```

This creates `.enject/` in the current directory with the project's config and encrypted store. Add it to `.gitignore` — it should never be committed.

---

## Usage

### Initialize a store

Run this once per project, in the project root:

```bash
enject init
```

This generates a random 32-byte salt, writes `.enject/config.toml`, creates an empty encrypted store at `.enject/store`, and prompts you to set a master password. Add `.enject/` to your `.gitignore` — the store should never be committed.

### Add secrets

```bash
enject set some_database_url
# prompts: Value for 'database_url': (hidden)

enject set some_api_key
```

Values are always entered interactively. There is no way to pass a value as a command-line argument — this prevents secrets from appearing in shell history or `ps` output.

### Reference secrets in `.env`

```
DATABASE_URL=en://some_database_url
MY_API_KEY=en://stripe_key
PORT=3000
```

Plain `KEY=VALUE` lines pass through unchanged. Only `en://` references are resolved.

### Run your app

```bash
enject run -- npm start
enject run -- python manage.py runserver
enject run -- cargo run
```

Everything after `--` is passed verbatim to the OS. The subprocess inherits your full shell environment (so `PATH`, `HOME`, etc. are present) with `.env` values layered on top.

### Other commands

```bash
enject list              # print stored key names (never values)
enject delete <key>      # remove a secret
enject import <file>     # encrypt all values in a plaintext .env, rewrite it as en:// template
enject rotate            # re-encrypt the store with a new master password
```

### Deliberately missing commands

There is no `get` and no `export`. Printing a secret value to stdout creates an AI-readable leakage vector — the entire point of enject is to keep values off disk and out of any readable output stream.

---

## Security verification

Every security invariant has a corresponding automated test and a manual inspection path.

### Run all automated tests

```bash
cargo test
```

31 tests, all covering the claims below.

---

### 1. Secrets never written to disk as plaintext

**Automated:** `store::password::tests::test_encrypt_decrypt_roundtrip`

Saves a secret, persists the store, reloads it from disk, decrypts, and checks the value round-trips correctly. Only passes if the bytes on disk are valid ciphertext — plaintext would fail decryption.

```bash
cargo test store::password::tests::test_encrypt_decrypt_roundtrip
```

**Manual inspection:**

```bash
enject init          # password: test123
enject set mykey     # value: my-super-secret

xxd .enject/store | head -5
strings .enject/store
```

`xxd` will show binary data. `strings` will return nothing — there are no ASCII sequences to extract. The first 12 bytes are the random nonce; everything after is AES-GCM ciphertext with a 16-byte authentication tag appended.

---

### 2. Fresh random nonce on every write

**Automated:** `store::password::tests::test_nonce_changes_on_each_save`

Saves the store twice in a row, reads the first 12 bytes of the file each time, and asserts they differ.

```bash
cargo test store::password::tests::test_nonce_changes_on_each_save
```

**Manual inspection:**

```bash
xxd .enject/store | head -1    # note the first 12 bytes
enject set anotherkey          # any write rotates the nonce
xxd .enject/store | head -1    # first 12 bytes are now different
```

---

### 3. Wrong password returns an error

**Automated:** `store::password::tests::test_wrong_password_returns_err`

Creates a store with one password, then attempts to unlock it with a different password and asserts `Err` is returned.

```bash
cargo test store::password::tests::test_wrong_password_returns_err
```

**Manual:**

```bash
enject list    # enter the wrong password
# output: "Wrong master password or corrupted store."
# exit code: 1
```

---

### 4. Tampered ciphertext is rejected (AES-GCM authentication)

AES-GCM produces a 16-byte authentication tag over the ciphertext. Any modification — even a single flipped bit — causes verification to fail before decryption proceeds. The plaintext is never exposed.

**Automated:** `store::password::tests::test_tampered_ciphertext_returns_err`

Flips one byte in the ciphertext region of the store file (past the 12-byte nonce), then attempts decryption and asserts `Err`.

```bash
cargo test store::password::tests::test_tampered_ciphertext_returns_err
```

**Manual:**

```bash
# Flip byte 20 (inside ciphertext, past the nonce)
python3 -c "
data = open('.enject/store', 'rb').read()
bad  = data[:20] + bytes([data[20] ^ 0xFF]) + data[21:]
open('.enject/store', 'wb').write(bad)
"
enject list
# output: "Wrong master password or corrupted store."
```

---

### 5. Hard error on any unresolved `en://` reference

If a reference in `.env` has no matching key in the store, `enject run` exits immediately with a non-zero code. The subprocess is never launched.

**Automated:** `env_template::tests::test_unknown_ev_ref_returns_err`

Calls `resolve()` with a reference that has no matching entry and asserts `Err`.

```bash
cargo test env_template::tests::test_unknown_ev_ref_returns_err
```

**Manual:**

```bash
echo "DB=en://nonexistent_key" > .env
enject run -- env
# output: Secret 'nonexistent_key' not found in store. Add it with: enject set nonexistent_key
# exit code: 1  (the `env` subprocess never ran)
```
---
## Future paths

### 1. Global store
Implement optional/additional system-wide store for easier maintenance of secrets used across multiple projects.

### 2. Integration with system keychains, etc.
Reduce the need to to manually enter the store's password whenever making updates


