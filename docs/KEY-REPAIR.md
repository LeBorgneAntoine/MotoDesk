# Register signing key: diagnosis and repair

Every register signs its fiscal journal with a key. The key lives in the operating
system's credential store (or, where there is none, in the database). If the store is
emptied, or the database is copied to another computer, the key is gone and the app
refuses every fiscal write for that register (a document signed with a key nobody can
verify would be worse than no document).

A lost key does **not** lose the journal's proof: its hash chain is intact. Only the
signatures lack a key. The repair therefore never rewrites anything. It appends one
journal entry, `register_key_changed`, signed with the key that signs from then on, and
the register carries on: same code, same chain, same totals, same numbering. The entry
replicates to the other devices and the cloud like any other.

## The tool

`motodesk-keytool` is built with the app (`cargo build -p motodesk-desktop --bin motodesk-keytool`).
It works on a database file, with the app **closed**.

```text
motodesk-keytool diagnose --db <local.db> [--register C9329]
motodesk-keytool repair   --db <local.db> --out <repaired.db> [--register C9329]
                          [--keystore database|keychain] [--accept-unverifiable] --yes
```

- `diagnose` only reads.
- `repair --out` copies the database with SQLite (the original is never written) and
  repairs the copy. Check it, then replace `local.db` with it (app closed). `--in-place`
  repairs the file you give it instead.
- `--keystore keychain` (default) puts the new key in the system credential store;
  `database` keeps it in the database, next to the data it signs (the register records
  which, so an audit can tell).

The app's data folder on Windows is `%APPDATA%\com.motodesk.app\` (an install made under the former name, `com.motogest.lite`, is moved there automatically on first start).

## What `diagnose` can say

| State | Meaning | `repair` |
|---|---|---|
| saine | chain verifies, a key that can sign is at hand | nothing to do |
| clé perdue | chain verifies, no key can sign for it | declares a new key from the next entry |
| signée par une autre clé | an older version silently replaced a lost key; that key is still at hand | declares that key from the first entry it signed |
| signatures invérifiables | some entries' signatures match no key at hand (the key they used is gone too) | refused, unless `--accept-unverifiable` |
| chaîne rompue | the hashes do not link: this is tampering, not a lost key | refused, always |

### `--accept-unverifiable`

When signatures match no available key, the honest options are to find that key (another
computer, an old credential store) or to say so. This flag has the journal declare, in the
repair entry, that those entries (a fixed range, all earlier than the declaration) can no
longer be verified. Their hashes still chain and are still checked; only the signature is
not. The register's integrity check keeps reporting them. A declaration can never excuse an
entry written after it. Do this knowingly: it is a statement the register makes about itself.

## Cloud

The cloud keeps an off-site copy of each journal and only keeps rows that verify. It follows a
key change only when an **administrator's** session pushes it (an employee's session is
held back and retried), so an employee cannot declare their own key for a register.
Entries the cloud refused before the change (signed by a key it did not know) stay missing
from its copy.
