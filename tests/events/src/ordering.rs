//! Event ordering and payload privacy.
//!
//! ## Ordering
//!
//! Entry points publish in a documented order. Resolve/reject each emit a
//! legacy event followed by a v2 metadata event; other tested mutations emit
//! one. What an indexer needs to know is the ordering across operations too.
//!
//! One consequence of the test environment is worth recording, because it
//! shapes how these tests are written: `env.events().all()` reports the events
//! of the most recent invocation only. A sequence therefore has to be observed
//! step by step; the stream cannot be replayed after the fact.
//!
//! ## Privacy
//!
//! Contracts store hashes, never the values behind them. The payload assertions
//! below are what keeps that true as events evolve: an event carrying an amount
//! or an identity would leak to every indexer on the network, permanently.

use crate::harness::{
    attempt_failure, hash, read_events, Deployment, ObservedEvent, APPROVED_SCHEMA,
};
use soroban_sdk::testutils::{Address as _, Events as _};
use soroban_sdk::{Address, BytesN, Env, Symbol, TryFromVal, Val};

/// A named operation, boxed so one table can hold calls of differing shapes.
type NamedCall<'a> = (&'a str, std::boxed::Box<dyn Fn() + 'a>);

// ─── Ordering ───────────────────────────────────────────────────────────────

#[test]
fn each_mutation_publishes_its_documented_events_in_order() {
    let deployment = Deployment::new();
    let successor = Address::generate(&deployment.env);
    let replacement = Address::generate(&deployment.env);
    let resolution_id = deployment.register_proof(0xA1);
    deployment.proofs.open_dispute(
        &resolution_id,
        &deployment.issuer,
        &hash(&deployment.env, 0xA2),
    );
    let rejection_id = deployment.register_proof(0xA3);
    deployment.proofs.open_dispute(
        &rejection_id,
        &deployment.issuer,
        &hash(&deployment.env, 0xA4),
    );

    let sequence: std::vec::Vec<NamedCall> = std::vec![
        ("pause", std::boxed::Box::new(|| deployment.config.pause())),
        (
            "unpause",
            std::boxed::Box::new(|| deployment.config.unpause())
        ),
        (
            "approve_schema_version",
            std::boxed::Box::new(|| deployment.config.approve_schema_version(&5))
        ),
        (
            "deprecate_schema_version",
            std::boxed::Box::new(|| deployment.config.deprecate_schema_version(&5))
        ),
        (
            "suspend_issuer",
            std::boxed::Box::new(|| deployment.issuers.suspend_issuer(
                &deployment.issuer_id,
                &soroban_sdk::BytesN::from_array(&deployment.env, &[1u8; 32])
            ))
        ),
        (
            "reactivate_issuer",
            std::boxed::Box::new(|| deployment.issuers.reactivate_issuer(
                &deployment.issuer_id,
                &soroban_sdk::BytesN::from_array(&deployment.env, &[1u8; 32])
            ))
        ),
        (
            "update_issuer",
            std::boxed::Box::new(|| {
                deployment
                    .issuers
                    .update_issuer(&deployment.issuer_id, &hash(&deployment.env, 0xC1))
            })
        ),
        (
            "rotate_issuer_address",
            std::boxed::Box::new(|| {
                deployment
                    .issuers
                    .rotate_issuer_address(&deployment.issuer_id, &replacement)
            })
        ),
        (
            "set_admin",
            std::boxed::Box::new(|| {
                deployment.config.nominate_admin(&successor);
                deployment.config.accept_admin()
            })
        ),
        (
            "resolve_dispute",
            std::boxed::Box::new(|| {
                deployment.proofs.resolve_dispute(
                    &resolution_id,
                    &earnproof_shared::DisputeDecisionReason {
                        version: earnproof_shared::DISPUTE_DECISION_REASON_VERSION,
                        code: earnproof_shared::DISPUTE_REASON_EVIDENCE_SUPPORTS_CLAIM,
                        reason_commitment: None,
                    },
                )
            })
        ),
        (
            "reject_dispute",
            std::boxed::Box::new(|| {
                deployment.proofs.reject_dispute(
                    &rejection_id,
                    &earnproof_shared::DisputeDecisionReason {
                        version: earnproof_shared::DISPUTE_DECISION_REASON_VERSION,
                        code: earnproof_shared::DISPUTE_REASON_CLAIM_UNSUBSTANTIATED,
                        reason_commitment: None,
                    },
                )
            })
        ),
    ];

    for (name, call) in sequence {
        let events = deployment.capture(&call);
        if name == "resolve_dispute" || name == "reject_dispute" {
            assert_eq!(
                events.len(),
                2,
                "{name} must publish its compatibility pair"
            );
            assert!(events[1].is(&deployment.env, "dispute_decision_committed_v2"));
            assert!(events[0].is(
                &deployment.env,
                if name == "resolve_dispute" {
                    "dispute_resolved"
                } else {
                    "dispute_rejected"
                }
            ));
        } else {
            assert_eq!(events.len(), 1, "{name} must publish exactly one event");
        }
    }
}

#[test]
fn a_multi_step_sequence_publishes_events_in_invocation_order() {
    // The operator sequence an incident responder would run: contain the
    // protocol, then revoke the compromised issuer, then recover. An indexer
    // replaying the ledger must see these in exactly this order.
    let deployment = Deployment::new();
    let mut observed: std::vec::Vec<std::string::String> = std::vec::Vec::new();

    let mut record = |events: std::vec::Vec<ObservedEvent>| {
        for event in events {
            let symbol = event
                .discriminant(&deployment.env)
                .expect("every event must carry a symbol discriminant");
            observed.push(std::format!("{symbol:?}"));
        }
    };

    record(deployment.capture(|| deployment.config.pause()));
    record(deployment.capture(|| {
        deployment.issuers.suspend_issuer(
            &deployment.issuer_id,
            &soroban_sdk::BytesN::from_array(&deployment.env, &[1u8; 32]),
        )
    }));
    record(deployment.capture(|| {
        deployment.issuers.revoke_issuer(
            &deployment.issuer_id,
            &soroban_sdk::BytesN::from_array(&deployment.env, &[1u8; 32]),
        )
    }));
    record(deployment.capture(|| deployment.config.unpause()));

    assert_eq!(observed.len(), 4, "one event per step");

    // Rendered symbols are compared rather than parsed, so the assertion fails
    // loudly if a topic is ever renamed.
    assert!(observed[0].contains("paused"), "step 0 should be paused");
    assert!(
        observed[1].contains("issuer_suspended"),
        "step 1 should be issuer_suspended"
    );
    assert!(
        observed[2].contains("issuer_revoked"),
        "step 2 should be issuer_revoked"
    );
    assert!(
        observed[3].contains("unpaused"),
        "step 3 should be unpaused"
    );
}

#[test]
fn a_failed_step_leaves_no_gap_in_the_sequence() {
    // If a middle step is rejected, the surrounding steps must still be the
    // only two events. A ghost event here would make the indexer believe a
    // state transition happened between them.
    let deployment = Deployment::new();

    let first = deployment.capture(|| deployment.config.pause());
    assert_eq!(first.len(), 1);

    let rejected = attempt_failure(&deployment, || {
        // Already registered by the harness.
        deployment.issuers.register_issuer(
            &deployment.issuer_id,
            &Address::generate(&deployment.env),
            &hash(&deployment.env, 0xD1),
            &hash(&deployment.env, 0x99),
        );
    });
    assert!(rejected.is_empty(), "the rejected step must be silent");

    let last = deployment.capture(|| deployment.config.unpause());
    assert_eq!(last.len(), 1);
    assert!(last[0].is(&deployment.env, "unpaused"));
}

#[test]
fn cross_contract_rejection_publishes_nothing_from_either_contract() {
    // proof-registry calls into protocol-config and issuer-registry before
    // committing. A partial sequence — the callee publishing while the caller
    // rolls back — would be the hardest ghost event to diagnose.
    let deployment = Deployment::new();
    deployment.config.pause();
    let expires = deployment.env.ledger().timestamp() + 100_000;

    let events = attempt_failure(&deployment, || {
        deployment.proofs.register_proof_with_type_identifier(
            &hash(&deployment.env, 0x91),
            &hash(&deployment.env, 0x92),
            &deployment.issuer,
            &APPROVED_SCHEMA,
            &expires,
            &soroban_sdk::BytesN::from_array(&deployment.env, &[1u8; 32]),
        );
    });

    assert!(
        events.is_empty(),
        "a rejected cross-contract call must publish nothing from any contract"
    );
}

// ─── Privacy ────────────────────────────────────────────────────────────────

/// Payload field names that must never appear on any event.
///
/// The contracts hold only hashes, so these should be impossible by
/// construction. The assertion exists to keep it that way: an event is
/// published to every indexer on the network and cannot be recalled.
const FORBIDDEN_FIELDS: &[&str] = &[
    "amount",
    "balance",
    "income",
    "salary",
    "payment",
    "payment_history",
    "memo",
    "name",
    "email",
    "identity",
    "subject",
    "wallet",
    "secret",
    "signature",
    "key",
    "seed",
    "commitment",
];

/// Asserts that no payload field name resembles protected data.
fn assert_no_protected_fields(env: &Env, event: &ObservedEvent, context: &str) {
    let map: soroban_sdk::Map<Symbol, Val> =
        soroban_sdk::Map::try_from_val(env, &event.data).expect("event payloads are maps");

    for key in map.keys().iter() {
        let rendered = std::format!("{key:?}").to_lowercase();

        for forbidden in FORBIDDEN_FIELDS {
            assert!(
                !rendered.contains(forbidden),
                "{context}: event payload field {rendered} looks like protected data \
                 ({forbidden}); events reach every indexer and cannot be recalled"
            );
        }
    }
}

#[test]
fn no_event_payload_carries_protected_data() {
    let deployment = Deployment::new();
    let successor = Address::generate(&deployment.env);
    let replacement = Address::generate(&deployment.env);

    let checks: std::vec::Vec<NamedCall> = std::vec![
        ("pause", std::boxed::Box::new(|| deployment.config.pause())),
        (
            "unpause",
            std::boxed::Box::new(|| deployment.config.unpause())
        ),
        (
            "approve_schema_version",
            std::boxed::Box::new(|| deployment.config.approve_schema_version(&6))
        ),
        (
            "deprecate_schema_version",
            std::boxed::Box::new(|| deployment.config.deprecate_schema_version(&6))
        ),
        (
            "update_issuer",
            std::boxed::Box::new(|| {
                deployment
                    .issuers
                    .update_issuer(&deployment.issuer_id, &hash(&deployment.env, 0xC2))
            })
        ),
        (
            "suspend_issuer",
            std::boxed::Box::new(|| deployment.issuers.suspend_issuer(
                &deployment.issuer_id,
                &soroban_sdk::BytesN::from_array(&deployment.env, &[1u8; 32])
            ))
        ),
        (
            "reactivate_issuer",
            std::boxed::Box::new(|| deployment.issuers.reactivate_issuer(
                &deployment.issuer_id,
                &soroban_sdk::BytesN::from_array(&deployment.env, &[1u8; 32])
            ))
        ),
        (
            "rotate_issuer_address",
            std::boxed::Box::new(|| {
                deployment
                    .issuers
                    .rotate_issuer_address(&deployment.issuer_id, &replacement)
            })
        ),
        (
            "revoke_issuer",
            std::boxed::Box::new(|| deployment.issuers.revoke_issuer(
                &deployment.issuer_id,
                &soroban_sdk::BytesN::from_array(&deployment.env, &[1u8; 32])
            ))
        ),
        (
            "set_admin",
            std::boxed::Box::new(|| {
                deployment.config.nominate_admin(&successor);
                deployment.config.accept_admin()
            })
        ),
    ];

    for (name, call) in checks {
        for event in deployment.capture(&call) {
            assert_no_protected_fields(&deployment.env, &event, name);
        }
    }
}

#[test]
fn issuer_events_carry_hashes_rather_than_raw_identifiers() {
    // `issuer_id_hash` and `metadata_hash` are 32-byte digests. If either ever
    // became a string or a structured value, the event would start carrying
    // whatever the backend hashed — which is the leak this checks for.
    let deployment = Deployment::new();
    let next = Address::generate(&deployment.env);
    let issuer_id = hash(&deployment.env, 0x03);
    let metadata = hash(&deployment.env, 0xC3);

    let events = deployment.capture(|| {
        deployment
            .issuers
            .register_issuer(&issuer_id, &next, &metadata, &metadata)
    });

    let event = &events[0];

    // Both must decode as fixed 32-byte values, and nothing else.
    let announced_id: BytesN<32> = event
        .field(&deployment.env, "issuer_id_hash")
        .expect("issuer_id_hash must be a 32-byte hash");
    let announced_metadata: BytesN<32> = event
        .field(&deployment.env, "metadata_hash")
        .expect("metadata_hash must be a 32-byte hash");

    assert_eq!(announced_id, issuer_id);
    assert_eq!(announced_metadata, metadata);
}

#[test]
fn initialization_event_carries_only_the_admin_address() {
    // A public Stellar address is not protected data — it is public by
    // construction. The assertion is on arity: nothing else rides along.
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let contract_id = env.register(protocol_config::ProtocolConfigContract, ());
    let config = protocol_config::ProtocolConfigContractClient::new(&env, &contract_id);
    config.initialize(&admin);

    let events = read_events(&env);
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].field_count(&env),
        1,
        "the initialized event should carry exactly one field"
    );
    assert_no_protected_fields(&env, &events[0], "initialize");
}

#[test]
fn no_event_is_published_by_a_contract_that_did_not_act() {
    // Attribution: an event published under the wrong contract id would make an
    // indexer route it to the wrong stream. Each event must name its emitter.
    let deployment = Deployment::new();

    let config_events = deployment.capture(|| deployment.config.pause());
    assert_eq!(config_events[0].contract, deployment.config.address);

    let issuer_events = deployment.capture(|| {
        deployment.issuers.suspend_issuer(
            &deployment.issuer_id,
            &soroban_sdk::BytesN::from_array(&deployment.env, &[1u8; 32]),
        )
    });
    assert_eq!(issuer_events[0].contract, deployment.issuers.address);
}

#[test]
fn event_topics_are_single_symbol_discriminants() {
    // The fixture schema documents the first topic as the discriminant. If a
    // contract started publishing additional indexed topics, indexers filtering
    // on topic arity would silently stop matching.
    let deployment = Deployment::new();

    let events = deployment.capture(|| deployment.config.pause());
    assert_eq!(
        events[0].topics.len(),
        1,
        "events are documented as carrying a single discriminant topic"
    );
    assert!(events[0].discriminant(&deployment.env).is_some());

    // Confirm the environment agrees the stream is non-empty, guarding against
    // an assertion that would pass vacuously.
    let captured = deployment.env.events().all();
    assert!(!captured.events().is_empty());
}
