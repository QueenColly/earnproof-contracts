use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Timestamp, Uint64};

// --- Operation-Scoped Governance Roles (#154) ---
#[cw_serde]
pub enum GovernanceRole {
    Pause,
    Schema,
    Issuer,
    Revocation,
    Upgrade,
    Recovery, // Explicit recovery hierarchy
}

// --- Admin Transfer History (#245) ---
#[cw_serde]
pub struct AdminTransferRecord {
    pub sequence: Uint64,
    pub old_admin: Addr,
    pub proposed_admin: Addr,
    pub actor: Addr,
    pub ledger_height: Uint64,
    pub timestamp: Timestamp,
    pub terminal_reason: String, // "Accepted", "Cancelled", "Expired"
}

// --- Scoped Pause Transition History (#244) ---
#[cw_serde]
pub struct PauseTransitionRecord {
    pub sequence: Uint64,
    pub scope: String, // "Global", "IssuerRegistry", etc.
    pub is_paused: bool,
    pub actor: Addr,
    pub ledger_height: Uint64,
    pub timestamp: Timestamp,
    pub incident_commitment: Option<String>,
}

// --- Pending Upgrade Approvals (#243) ---
#[cw_serde]
pub struct PendingUpgradeApproval {
    pub proposal_id: String,
    pub target_hash: String,
    pub version: String,
    pub expires_at: Timestamp,
    pub approval_count: u32,
}

// --- Bounded Queries ---
#[cw_serde]
pub enum GovernanceQueryMsg {
    AdminTransferHistory { start_after: Option<Uint64>, limit: Option<u32> },
    PauseHistory { scope: Option<String>, start_after: Option<Uint64>, limit: Option<u32> },
    PendingUpgrades { start_after: Option<String>, limit: Option<u32> },
    HasRole { account: String, role: GovernanceRole },
}