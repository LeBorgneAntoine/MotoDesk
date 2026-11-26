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

`motodesk-keytool` is built with the app (`cargo build -p motodesk-gpui --bin motodesk-keytool`).
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
| saine | chain verifies, the key the journal names is at hand (keychain or database) | nothing to do |
| clé perdue | chain verifies, no key at hand is the one the journal names | declares a new key from the next entry |
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

## More than one key at hand

The credential store and the database can each hold a key for the same register, and not
always the same one:

- Windows can answer "not kept" for a write it did keep. A new register then keeps its key
  in the database as well (the same key, twice). A `repair --keystore keychain` is refused
  with « le trousseau du système refuse la nouvelle clé », nothing is written to the
  journal, and the key it made may stay in the credential store, declared nowhere.
- A restore onto a computer whose credential store cannot be written puts the backup's key
  in the restored database, while the store may still hold the key of the data that was
  replaced.
- A repair with `--keystore database` leaves whatever the credential store held.

None of this decides which key signs. The register signs with the key its journal names,
wherever that key is kept, and with no other; when the named key is not at hand, nothing
signs and the state is « clé perdue », whatever else is there. `diagnose` lists every key
at hand, where it is kept, and which one the register signs with. A backup carries the
named key.

A key the journal does not name is left where it is: the tool never deletes or replaces a
key on a guess. A refused repair can simply be run again, with either `--keystore`. The
new key is kept only once its declaration is written, in the same transaction for
`database`, just before the commit for `keychain`.

## Cloud

The cloud keeps an off-site copy of each journal and only keeps rows that verify. It follows a
key change only when an **administrator's** session pushes it (an employee's session is
held back and retried), so an employee cannot declare their own key for a register.
Entries the cloud refused before the change (signed by a key it did not know) stay missing
from its copy.
