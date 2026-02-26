# enject — Architecture

## What is enject?

`enject` is a CLI tool that prevents AI agents and other processes from reading secrets out of `.env` files by ensuring plaintext secrets **never exist on disk**. Instead, `.env` files contain only `en://` references. Secrets are decrypted from an encrypted local store and injected directly into a subprocess's environment at launch — they exist only in process memory, never in any file.

```
# .env (safe to commit, safe for AI agents to read)
DATABASE_URL=en://database_url
STRIPE_KEY=en://stripe_key
PORT=3000
```

```bash
enject run -- npm start
# prompts for master password → decrypts store → spawns subprocess with resolved env
# npm start sees real values; .env on disk never changes
```

---

## The Problem Being Solved

Tools like Claude Code can read `.env` files from the project directory, accidentally leaking API keys, database passwords, and other secrets into AI context. Even when AI tools claim not to read `.env`, there is no enforcement mechanism. The only reliable solution is to ensure `.env` files contain nothing worth leaking.

`enject` is the free, open-source, zero-dependency alternative to using `1password CLI` or similar commercial tools for this purpose.

---

## Mental Model

```
.env  (committed to git, safe to read)     .enject/store  (encrypted blob, gitignored)
──────────────────────────────────         ───────────────────────────────────────────
DATABASE_URL=en://database_url             AES-256-GCM encrypted JSON:
STRIPE_KEY=en://stripe_key                 { "database_url": "postgres://...",
PORT=3000                                    "stripe_key": "sk_live_..." }

                     ↓  enject run -- npm start

          1. prompt for master password
          2. derive key via Argon2id
          3. decrypt store in memory
          4. resolve en:// references
          5. zeroize key material
          6. exec subprocess with resolved env

          subprocess env:
          DATABASE_URL=postgres://user:pass@localhost/mydb
          STRIPE_KEY=sk_live_abc123
          PORT=3000
```

Plain `KEY=VALUE` pairs in `.env` pass through unchanged. Only `en://` references are resolved.

---

## Project Directory Layout

```
my-project/
├── .env                    # template file — en:// refs + plain values. Commit this.
├── .enject/
│   ├── config.toml         # backend type, KDF params, format version
│   └── store               # encrypted secrets blob. NEVER commit this.
└── .gitignore              # must include .enject/
```

A global store lives at `~/.enject/` for secrets shared across multiple projects. These are referenced with `en://global/key_name`.

---

## CLI Surface

```bash
enject init                  # initialize store in current directory, choose backend
enject set <key>             # add/update a secret (value prompted interactively, never as CLI arg)
enject list                  # list key names only, never values
enject delete <key>          # remove a secret from the store
enject run -- <cmd> [args]   # resolve .env → inject → exec subprocess
enject import <file>         # ingest a plaintext .env, encrypt all values, rewrite file as template
enject rotate                # re-encrypt store with a new master password
```

### Deliberately Omitted Commands

- **No `get`** — printing a secret value to stdout creates an AI-readable leakage vector
- **No `export`** — same reason; printing all resolved values defeats the purpose

If a user loses their master password, secrets must be re-added manually via `enject set`. This is intentional: there is no recovery path that doesn't involve decrypting the store.

---

## Secret Backend: Master Password (AES-256-GCM + Argon2id)

This is the only supported backend in v1. It is fully self-contained with no external dependencies.

### Encryption Scheme

```
config.toml contains:
  backend = "password"
  version = 1
  kdf = "argon2id"
  m_cost = 65536      # 64 MB memory
  t_cost = 3          # 3 iterations
  p_cost = 4          # 4 parallelism threads
  salt = "<hex>"      # 32 random bytes, generated at init, never changes

store file contains:
  nonce = first 12 bytes   # random, rotated on every write
  ciphertext = remaining bytes
```

### Runtime Decryption Flow

```
1. Read salt from config.toml
2. Prompt for master password via rpassword (never echoed, never in shell history)
3. argon2id(password, salt, m_cost, t_cost, p_cost) → 32-byte key
4. Read nonce from store file header (first 12 bytes)
5. aes-256-gcm decrypt(key, nonce, ciphertext) → plaintext JSON bytes
6. zeroize(key), zeroize(password) immediately
7. Parse JSON → HashMap<String, String>
```

### Plaintext Store Format (inside encryption)

```json
{
  "database_url": "postgres://user:pass@localhost/mydb",
  "stripe_key": "sk_live_abc123",
  "sendgrid_api_key": "SG.xxxx"
}
```

### Write Flow (set, delete, rotate)

```
1. Decrypt store → mutate HashMap → serialize to JSON
2. Generate fresh random 12-byte nonce
3. aes-256-gcm encrypt(key, nonce, JSON) → ciphertext
4. Write nonce + ciphertext atomically to store file
5. zeroize all key material
```

Nonce is rotated on every write to prevent nonce reuse — a critical AES-GCM security property.

---

## .env Template Parsing

### Rules

| Line format | Behavior |
|---|---|
| `# comment` | Ignored |
| `KEY=value` | Passed through as-is to subprocess env |
| `KEY=en://secret_name` | Resolved from local `.enject/store` |
| `KEY=en://global/secret_name` | Resolved from `~/.enject/store` |
| `KEY=en://unknown_key` | **Hard error** — enject refuses to run |
| Malformed line | **Hard error** — enject refuses to run |

### Error Philosophy

enject fails loudly rather than silently. If any `en://` reference cannot be resolved, the subprocess is never launched. Partial injection with some secrets missing is worse than no injection at all.

---

## subprocess Execution

`enject run` uses `std::process::Command` with an **explicitly constructed environment**:

- Starts from the OS baseline environment (so `PATH`, `HOME`, etc. are present)
- Adds all resolved `.env` values on top
- Does NOT inherit arbitrary parent shell variables unless they were in `.env`

This means the subprocess gets exactly what was declared in `.env`, resolved — no more, no less.

The `--` separator is required. Everything after `--` is passed verbatim to the OS as the command and arguments.

---

## Security Properties

### What enject protects against

- AI agents, editors, log scrapers, or any process reading `.env` from disk — they see only `en://` references
- Accidental commits of plaintext secrets to git
- Shell history exposure (values are never passed as CLI arguments)

### What enject does NOT protect against

- A compromised OS or process that can read `/proc/<pid>/environ` — once secrets are in a subprocess's environment, they're in memory. This is the same trust boundary as `op run` from 1Password CLI and is considered acceptable.
- An attacker with access to the encrypted store AND the master password

### Deliberate security decisions

- `set` accepts no value argument — value is always prompted interactively via `rpassword`
- Nonce rotates on every write — no AES-GCM nonce reuse
- `zeroize` is called on all key material immediately after use
- `get` and `export` commands do not exist
- Hard errors on missing references — no silent partial injection
- Store file has no plaintext headers — no key names visible without decryption

---

## Rust Crate Dependencies

| Purpose | Crate |
|---|---|
| CLI argument parsing | `clap` (derive API) |
| AES-256-GCM encryption | `aes-gcm` (RustCrypto) |
| Argon2id KDF | `argon2` (RustCrypto) |
| Secure memory zeroing | `zeroize`, `secrecy` |
| Interactive password prompt | `rpassword` |
| Serialization | `serde` + `serde_json` |
| Config file parsing | `toml` |
| Error handling | `anyhow` + `thiserror` |
| Random number generation | `rand` |

No async runtime. All I/O is synchronous, keeping the binary small and the trust surface minimal.

---

## Module Structure

```
src/
├── main.rs                  # entry point, clap dispatch
├── cli.rs                   # clap struct definitions and argument types
├── config.rs                # .enject/config.toml read/write, KDF params
├── store/
│   ├── mod.rs               # Store trait: open(), get(), set(), delete(), list()
│   └── password.rs          # AES-256-GCM + Argon2id implementation
├── env_template.rs          # .env file parsing, en:// reference extraction
├── runner.rs                # subprocess construction and exec
├── commands/
│   ├── init.rs              # enject init
│   ├── set.rs               # enject set <key>
│   ├── list.rs              # enject list
│   ├── delete.rs            # enject delete <key>
│   ├── run.rs               # enject run -- <cmd>
│   ├── import.rs            # enject import <file>
│   └── rotate.rs            # enject rotate
└── error.rs                 # thiserror error type definitions
```

The `Store` trait is the key abstraction boundary. `runner.rs` and all commands interact only with the trait — they have no knowledge of the encryption implementation.

---

## Future Considerations (Out of Scope for v1)

- **Age encryption backend** — for teams wanting to commit encrypted stores to git with per-developer keypairs
- **Shell completions** — `clap` can generate these trivially once the CLI is stable  
- **Session caching** — avoid re-prompting for password within a terminal session (requires careful design around security tradeoffs)
- **Other config file formats** — for `database.yml`, `config.toml` etc.; deferred because it requires templated file resolution with temporary plaintext files, which complicates the security model
