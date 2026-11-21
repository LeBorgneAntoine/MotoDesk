use thiserror::Error;

#[derive(Debug, Error)]
pub enum FiscalError {
    #[error("canonical JSON does not accept the non-integer number {0}; monetary values must be integer cents")]
    NonIntegerNumber(String),

    #[error("arithmetic overflow computing {0}")]
    Overflow(&'static str),

    #[error("VAT rate {0} basis points is outside the accepted 0..=10000 range")]
    VatRateOutOfRange(i64),

    #[error("discount {0} basis points is outside the accepted 0..=10000 range")]
    DiscountOutOfRange(i64),

    #[error("invalid hexadecimal digest: {0}")]
    BadDigest(String),

    #[error("invalid signing key: {0}")]
    BadKey(String),

    #[error("signature verification failed for sequence {seq}")]
    BadSignature { seq: i64 },

    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

/// Where and how a chain stopped being trustworthy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainBreak {
    /// Sequence numbers must run 1, 2, 3 with no hole and no repeat.
    SequenceGap { expected: i64, found: i64 },
    /// The entry does not point at the entry that precedes it.
    PrevHashMismatch {
        seq: i64,
        expected: String,
        found: String,
    },
    /// The stored payload no longer hashes to the recorded digest, meaning the
    /// row was edited underneath the application.
    PayloadTampered {
        seq: i64,
        expected: String,
        found: String,
    },
    /// The recorded chain hash is not what the inputs produce.
    ChainHashMismatch {
        seq: i64,
        expected: String,
        found: String,
    },
    /// Clocks may drift between devices but a single chain must not go back.
    TimeWentBackwards {
        seq: i64,
        previous: String,
        found: String,
    },
    /// The signature does not verify against the register's public key.
    BadSignature { seq: i64 },
}

impl ChainBreak {
    pub fn seq(&self) -> i64 {
        match self {
            ChainBreak::SequenceGap { found, .. } => *found,
            ChainBreak::PrevHashMismatch { seq, .. }
            | ChainBreak::PayloadTampered { seq, .. }
            | ChainBreak::ChainHashMismatch { seq, .. }
            | ChainBreak::TimeWentBackwards { seq, .. }
            | ChainBreak::BadSignature { seq } => *seq,
        }
    }
}

impl std::fmt::Display for ChainBreak {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChainBreak::SequenceGap { expected, found } => write!(
                f,
                "séquence interrompue : entrée {expected} attendue, {found} trouvée"
            ),
            ChainBreak::PrevHashMismatch { seq, .. } => write!(
                f,
                "entrée {seq} : le chaînage ne correspond pas à l'entrée précédente"
            ),
            ChainBreak::PayloadTampered { seq, .. } => {
                write!(
                    f,
                    "entrée {seq} : les données ont été modifiées après enregistrement"
                )
            }
            ChainBreak::ChainHashMismatch { seq, .. } => {
                write!(f, "entrée {seq} : empreinte de chaînage incorrecte")
            }
            ChainBreak::TimeWentBackwards { seq, .. } => {
                write!(
                    f,
                    "entrée {seq} : horodatage antérieur à l'entrée précédente"
                )
            }
            ChainBreak::BadSignature { seq } => {
                write!(f, "entrée {seq} : signature électronique invalide")
            }
        }
    }
}
