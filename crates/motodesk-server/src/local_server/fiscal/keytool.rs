//! `motodesk-keytool`: diagnose and repair a register whose signing key is lost.
//!
//! ```text
//! motodesk-keytool diagnose --db <local.db> [--register C9329]
//! motodesk-keytool repair   --db <local.db> --out <repaired.db> [--register C9329]
//!                           [--keystore database|keychain] --yes
//! ```
//!
//! `diagnose` only reads. `repair` makes a copy of the database at `--out` and repairs
//! the copy: the original is never written (replace it yourself, with the app closed,
//! once you are satisfied). Repairing the original in place needs `--in-place`.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use sqlx::sqlite::SqliteConnectOptions;
use sqlx::SqlitePool;

use super::keyrepair::{diagnose, repair, Diagnosis, KeepKeyIn, Verdict};
use crate::local_server::db;

struct Args {
    command: String,
    db: Option<PathBuf>,
    out: Option<PathBuf>,
    register: Option<String>,
    keystore: KeepKeyIn,
    yes: bool,
    in_place: bool,
    accept_unverifiable: bool,
}

const USAGE: &str = "Usage :
  motodesk-keytool diagnose --db <local.db> [--register CODE]
  motodesk-keytool repair   --db <local.db> --out <copie-réparée.db> [--register CODE]
                            [--keystore database|keychain] [--accept-unverifiable] --yes

  diagnose   lit la base et dit dans quel état est la clé de signature de chaque caisse.
  repair     copie la base vers --out et répare la copie ; l'original n'est jamais modifié.
             --in-place répare la base donnée par --db elle-même (application fermée !).
  --accept-unverifiable
             quand des signatures ne correspondent à AUCUNE clé disponible : le journal
             déclare ces entrées « invérifiables » (leurs empreintes restent chaînées) et la
             caisse repart avec une nouvelle clé. C'est une déclaration, à faire en connaissance
             de cause ; sans cette option, rien n'est écrit dans ce cas.
  --keystore où ranger la nouvelle clé : « keychain » (trousseau du système, par défaut)
             ou « database » (dans la base, à côté des données qu'elle signe).";

fn parse(args: Vec<String>) -> Result<Args, String> {
    let mut it = args.into_iter();
    let command = it.next().ok_or("commande manquante")?;
    let mut a = Args {
        command,
        db: None,
        out: None,
        register: None,
        keystore: KeepKeyIn::Keychain,
        yes: false,
        in_place: false,
        accept_unverifiable: false,
    };
    while let Some(flag) = it.next() {
        let mut value = |name: &str| it.next().ok_or(format!("{name} demande une valeur"));
        match flag.as_str() {
            "--db" => a.db = Some(PathBuf::from(value("--db")?)),
            "--out" => a.out = Some(PathBuf::from(value("--out")?)),
            "--register" => a.register = Some(value("--register")?),
            "--keystore" => {
                a.keystore = match value("--keystore")?.as_str() {
                    "keychain" => KeepKeyIn::Keychain,
                    "database" => KeepKeyIn::Database,
                    other => return Err(format!("--keystore : « {other} » inconnu (keychain ou database)")),
                }
            }
            "--yes" => a.yes = true,
            "--in-place" => a.in_place = true,
            "--accept-unverifiable" => a.accept_unverifiable = true,
            other => return Err(format!("option inconnue : {other}")),
        }
    }
    Ok(a)
}

async fn open_read_only(path: &Path) -> anyhow::Result<SqlitePool> {
    let opts = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))?
        .read_only(true)
        .create_if_missing(false);
    Ok(SqlitePool::connect_with(opts).await?)
}

fn describe(d: &Diagnosis) -> String {
    let mut out = format!(
        "Caisse {}  ({} entrée(s), jusqu'à l'entrée n° {})\n  clé publique enregistrée : {}\n",
        d.register_code, d.entries, d.last_seq, d.recorded_public_key
    );
    if d.keys_at_hand.is_empty() {
        out.push_str("  clé secrète disponible    : aucune (ni trousseau, ni base)\n");
    }
    for (source, key) in &d.keys_at_hand {
        out.push_str(&format!("  clé secrète disponible    : {key}  ({source:?})\n"));
    }
    out.push_str(&match &d.verdict {
        Verdict::Healthy => "  ÉTAT : saine. Rien à faire.\n".to_string(),
        Verdict::KeyLost => "  ÉTAT : clé perdue. La chaîne est intacte ; plus aucune clé ne peut signer pour elle.\n  \u{2192} `repair` déclare une nouvelle clé dans le journal et la caisse repart (même code, même chaîne, même numérotation).\n".to_string(),
        Verdict::TailSignedByOtherKey { from_seq, public_key } => format!("  ÉTAT : à partir de l'entrée {from_seq}, le journal est signé par une autre clé ({public_key}), que vous avez encore.\n  \u{2192} `repair` déclare cette clé dans le journal à partir de l'entrée {from_seq}.\n"),
        Verdict::TailUnverifiable { from_seq, to_seq } => format!("  ÉTAT : les signatures des entrées {from_seq} à {to_seq} ne correspondent à aucune clé disponible ici (la chaîne, elle, est intacte).\n  \u{2192} Retrouvez la clé qui les a produites ; sinon `repair --accept-unverifiable` fait déclarer ces entrées invérifiables par le journal.\n"),
        Verdict::ChainBroken { seq, detail } => format!("  ÉTAT : chaîne rompue à l'entrée {seq} ({detail}). Ce n'est pas une clé perdue : le journal a été altéré.\n  \u{2192} Aucune réparation.\n"),
    });
    out
}

async fn run(a: Args) -> anyhow::Result<i32> {
    let Some(db_path) = a.db.clone() else {
        eprintln!("--db est obligatoire\n\n{USAGE}");
        return Ok(2);
    };
    if !db_path.exists() {
        eprintln!("base introuvable : {}", db_path.display());
        return Ok(1);
    }

    match a.command.as_str() {
        "diagnose" => {
            let pool = open_read_only(&db_path).await?;
            let diagnoses = diagnose(&pool, a.register.as_deref()).await.map_err(|e| anyhow::anyhow!("{e}"))?;
            for d in &diagnoses {
                print!("{}", describe(d));
            }
            Ok(0)
        }
        "repair" => {
            if !a.yes {
                eprintln!("`repair` écrit dans le journal fiscal : ajoutez --yes pour confirmer.\n\n{USAGE}");
                return Ok(2);
            }
            let target = match (&a.out, a.in_place) {
                (Some(_), true) => {
                    eprintln!("--out et --in-place s'excluent.");
                    return Ok(2);
                }
                (Some(out), false) => {
                    if out.exists() {
                        eprintln!("{} existe déjà : choisissez un autre fichier.", out.display());
                        return Ok(1);
                    }
                    // A consistent copy, WAL included, written by SQLite itself.
                    let source = open_read_only(&db_path).await?;
                    sqlx::query("VACUUM INTO $1")
                        .bind(out.to_string_lossy().to_string())
                        .execute(&source)
                        .await?;
                    source.close().await;
                    println!("Copie faite : {}", out.display());
                    out.clone()
                }
                (None, true) => db_path.clone(),
                (None, false) => {
                    eprintln!("Indiquez --out <copie> (recommandé) ou --in-place.\n\n{USAGE}");
                    return Ok(2);
                }
            };

            let pool = db::init_pool(&target).await?;
            // The journal's own rules (and the sync queue) need the current schema.
            db::run_migrations(&pool).await?;

            let diagnoses = diagnose(&pool, a.register.as_deref()).await.map_err(|e| anyhow::anyhow!("{e}"))?;
            let mut status = 0;
            for d in diagnoses {
                print!("{}", describe(&d));
                if matches!(d.verdict, Verdict::Healthy) {
                    continue;
                }
                match repair(&pool, &d.register_code, a.keystore, a.accept_unverifiable).await {
                    Ok(done) => println!(
                        "  RÉPARÉE : nouvelle clé {} déclarée à l'entrée n° {} (valable à partir de l'entrée {}), rangée dans « {} ».\n",
                        done.new_public_key, done.entry_seq, done.effective_from_seq, done.key_store
                    ),
                    Err(e) => {
                        println!("  NON RÉPARÉE : {e}\n");
                        status = 1;
                    }
                }
            }
            // Leave the file whole: no write-ahead log left beside it.
            sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)").execute(&pool).await.ok();
            pool.close().await;
            if a.out.is_some() {
                println!("Vérifiez, application fermée, puis remplacez local.db par {}.", target.display());
            }
            Ok(status)
        }
        _ => {
            eprintln!("commande inconnue : {}\n\n{USAGE}", a.command);
            Ok(2)
        }
    }
}

pub fn main(args: Vec<String>) -> i32 {
    let parsed = match parse(args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}\n\n{USAGE}");
            return 2;
        }
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    match runtime.block_on(run(parsed)) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("Erreur : {e:#}");
            1
        }
    }
}
