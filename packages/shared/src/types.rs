use cosmwasm_schema::cw_serde;
use cosmwasm_std::Uint64;

// --- Sequence & Proof State (#256) ---
#[cw_serde]
pub struct Proof {
    pub id: String,
    pub issuer: String,
    pub status: ProofStatus,
    pub sequence: Uint64, // Monotonic lifecycle sequence
}

#[cw_serde]
pub enum ProofStatus {
    Active,
    Disputed,
    Resolved,
    Revoked,
    Archived,
}

// --- Checkpoint & Digest Definitions (#254, #255) ---
#[cw_serde]
pub struct CheckpointDigestResponse {
    pub version: u32,
    pub sequence: Uint64,
    pub ledger_height: Uint64,
    pub component_counters: ComponentCounters,
    pub digest: String, // Deterministic hash of aggregate state
}

#[cw_serde]
pub struct ComponentCounters {
    pub issuers: Uint64,
    pub schemas: Uint64,
    pub proofs: Uint64,
}

#[cw_serde]
pub struct RevocationCheckpointResponse {
    pub version: u32,
    pub sequence: Uint64,
    pub ledger_height: Uint64,
    pub algorithm: String,
    pub root_commitment: String,
}

// --- Dispute Queries (#257) ---
#[cw_serde]
pub enum DisputeQueryMsg {
    OpenDisputes {
        start_after: Option<String>,
        limit: Option<u32>,
    },
    ClosedDisputes {
        start_after: Option<String>,
        limit: Option<u32>,
    },
}