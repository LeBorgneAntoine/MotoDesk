//! Card terminal adapter.
//!
//! The French *Protocole Caisse* is a licensed specification. This module
//! ships a simulator and a manual confirmation path; a licensed Caisse-AP
//! driver plugs in later behind the same enum.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ChargeOutcome {
    Accepted { auth_ref: String },
    Refused { reason: String },
    Timeout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalMode {
    Simulator,
    Manual,
}

impl TerminalMode {
    pub fn parse(s: &str) -> Self {
        match s {
            "manual" => TerminalMode::Manual,
            _ => TerminalMode::Simulator,
        }
    }
}

/// Charge a card. `force` lets tests (and the simulator UI) pick the outcome.
pub fn charge(mode: TerminalMode, amount_cents: i64, force: Option<&str>) -> ChargeOutcome {
    if amount_cents <= 0 {
        return ChargeOutcome::Refused {
            reason: "montant nul".into(),
        };
    }
    match mode {
        TerminalMode::Manual => ChargeOutcome::Accepted {
            auth_ref: "MANUAL".into(),
        },
        TerminalMode::Simulator => match force.unwrap_or("accept") {
            "refuse" => ChargeOutcome::Refused {
                reason: "paiement refusé (simulateur)".into(),
            },
            "timeout" => ChargeOutcome::Timeout,
            _ => ChargeOutcome::Accepted {
                auth_ref: format!("SIM-{amount_cents}"),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simulator_accepts_and_refuses() {
        assert!(matches!(
            charge(TerminalMode::Simulator, 2_520, Some("accept")),
            ChargeOutcome::Accepted { .. }
        ));
        assert!(matches!(
            charge(TerminalMode::Simulator, 2_520, Some("refuse")),
            ChargeOutcome::Refused { .. }
        ));
    }

    #[test]
    fn a_zero_amount_is_always_refused() {
        assert!(matches!(
            charge(TerminalMode::Simulator, 0, Some("accept")),
            ChargeOutcome::Refused { .. }
        ));
    }
}
