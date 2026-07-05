//! Echo-loop prevention and bidirectional sync integration tests.
//!
//! Both tests construct a real `SyncEngine` over a fresh in-memory `GrafeoDB`
//! (CDC enabled per-session via `session_with_cdc(true)`) + `LoroDoc`, call
//! `spawn_all`, drive edits through both bridges, and assert convergence
//! without echo loops.

#![allow(missing_docs)]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use grafeo::GrafeoDB;
use loro::{LoroDoc, LoroValue, ToJson};
use parking_lot::RwLock;

use grafeo_loro::bridge::SyncEngine;
use grafeo_loro::constants::{DEFAULT_BATCH_MS, OUTBOUND_POLL_MS};

/// Build a fresh `LoroValue::Map` from string-keyed scalars.
fn lmap(pairs: impl IntoIterator<Item = (&'static str, LoroValue)>) -> LoroValue {
    let mut m = HashMap::new();
    for (k, v) in pairs {
        m.insert(k.to_string(), v);
    }
    LoroValue::Map(m.into())
}

/// Insert a vertex into the LoroDoc root `V` map and commit.
fn loro_insert_vertex(doc: &LoroDoc, key: &str, props: LoroValue) {
    let v_map = doc.get_map("V");
    v_map.insert(key, props).expect("loro insert");
    doc.commit();
}

/// Wait long enough for the batcher to flush and the CDC poller to cycle.
async fn settle_inbound() {
    tokio::time::sleep(Duration::from_millis(DEFAULT_BATCH_MS * 3)).await;
}

/// Wait long enough for the CDC poller to pick up an event and the outbound
/// worker to apply it to Loro.
async fn settle_outbound() {
    tokio::time::sleep(Duration::from_millis(OUTBOUND_POLL_MS * 4)).await;
}

/// Read the `V[k]` property map from the LoroDoc as a `HashMap`, or `None`
/// if the key is absent / not a map.
fn loro_vertex_props(doc: &LoroDoc, key: &str) -> Option<HashMap<String, LoroValue>> {
    use loro::ValueOrContainer;
    let v_map = doc.get_map("V");
    match v_map.get(key) {
        Some(ValueOrContainer::Value(LoroValue::Map(m))) => {
            Some(m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        }
        _ => None,
    }
}

/// Drive a single user edit through the Loro→Grafeo bridge, then drive a
/// direct Grafeo mutation back through the Grafeo→Loro bridge. Asserts:
/// (a) the Loro edit lands in Grafeo, (b) the Grafeo edit lands in Loro,
/// (c) neither direction produces an echo (no infinite recursion).
#[tokio::test]
async fn echo_loop_prevention() {
    let grafeo_db = Arc::new(GrafeoDB::new_in_memory());
    let loro_doc = Arc::new(RwLock::new(LoroDoc::new()));
    let (engine, inbound_rx, outbound_rx) = SyncEngine::new(grafeo_db.clone(), loro_doc.clone());
    let engine = Arc::new(engine);
    let handles = engine.clone().spawn_all(inbound_rx, outbound_rx).await;

    // --- (a) Loro → Grafeo: insert vertex "k1" with {name: "Alice"} ---
    {
        let doc = loro_doc.read();
        loro_insert_vertex(&doc, "k1", lmap([("name", LoroValue::String("Alice".into()))]));
    }
    settle_inbound().await;

    // Assert: grafeo has the node, and the bridge recorded the loro_key mapping.
    let node_id = engine
        .maps()
        .node_id_map
        .read()
        .get("k1")
        .copied()
        .expect("bridge should have mapped k1 → grafeo NodeId");
    {
        let session = grafeo_db.session();
        let node = session
            .get_node(node_id)
            .expect("grafeo should have the bridge-created node");
        assert_eq!(
            node.get_property("name"),
            Some(&grafeo::Value::String("Alice".into())),
            "grafeo node should carry the Loro-written property"
        );
    }

    // --- (b) Grafeo → Loro: direct SET on the bridge-created node ---
    // session.execute goes through CdcGraphStore, so this records a CDC
    // Update event. The outbound worker looks up node_key_map → "k1" and
    // updates the LoroDoc's V map with the new property.
    {
        let mut session = grafeo_db.session_with_cdc(true);
        session
            .begin_transaction()
            .expect("begin tx");
        session
            .execute("MATCH (n {name: 'Alice'}) SET n.age = 42")
            .expect("MATCH SET");
        session.commit().expect("commit");
    }
    settle_outbound().await;

    // Assert: Loro V[k1] now has both name=Alice (original) and age=42 (new).
    // The read-modify-write in `apply_change_event_to_loro` must preserve
    // the existing property.
    {
        let doc = loro_doc.read();
        let props = loro_vertex_props(&doc, "k1").expect("V[k1] should exist after outbound update");
        assert_eq!(
            props.get("name"),
            Some(&LoroValue::String("Alice".into())),
            "original property should survive the merge"
        );
        assert_eq!(
            props.get("age"),
            Some(&LoroValue::I64(42)),
            "new property should be present after outbound update"
        );
    }

    // --- (c) No echo: the outbound worker's Loro write used origin
    // `ORIGIN_GRAFEO_BRIDGE`, so the Loro subscriber filters it out. After
    // another settle window, the Loro state must be unchanged (no second
    // grafeo mutation propagating back).
    let snapshot_before = {
        let doc = loro_doc.read();
        doc.get_deep_value().to_json_value()
    };
    settle_outbound().await;
    let snapshot_after = {
        let doc = loro_doc.read();
        doc.get_deep_value().to_json_value()
    };
    assert_eq!(
        snapshot_before, snapshot_after,
        "Loro state must not change after the outbound update settled (no echo)"
    );

    engine.shutdown();
    for h in handles {
        let _ = h.await;
    }
}

/// Bidirectional sync with interleaved edits from both sides. Verifies that
/// Loro→Grafeo and Grafeo→Loro both converge, and that concurrent edits do
/// not produce echo loops.
#[tokio::test]
async fn bidirectional_sync_with_delay() {
    let grafeo_db = Arc::new(GrafeoDB::new_in_memory());
    let loro_doc = Arc::new(RwLock::new(LoroDoc::new()));
    let (engine, inbound_rx, outbound_rx) = SyncEngine::new(grafeo_db.clone(), loro_doc.clone());
    let engine = Arc::new(engine);
    let handles = engine.clone().spawn_all(inbound_rx, outbound_rx).await;

    // Step 1: Loro → Grafeo. Insert vertex "k1" with {city: "Lyon"}.
    {
        let doc = loro_doc.read();
        loro_insert_vertex(&doc, "k1", lmap([("city", LoroValue::String("Lyon".into()))]));
    }
    settle_inbound().await;
    let node_id = engine
        .maps()
        .node_id_map
        .read()
        .get("k1")
        .copied()
        .expect("k1 mapped after Loro→Grafeo flush");
    {
        let session = grafeo_db.session();
        let node = session.get_node(node_id).expect("grafeo node k1 exists");
        assert_eq!(
            node.get_property("city"),
            Some(&grafeo::Value::String("Lyon".into())),
            "Lyon should land in grafeo via bridge"
        );
    }

    // Step 2: Grafeo → Loro. SET n.country = "France" on the bridge-created
    // node. This generates a CDC Update event; the outbound worker merges
    // the new property into Loro V[k1].
    {
        let mut session = grafeo_db.session_with_cdc(true);
        session.begin_transaction().expect("begin tx");
        session
            .execute("MATCH (n {city: 'Lyon'}) SET n.country = 'France'")
            .expect("MATCH SET");
        session.commit().expect("commit");
    }
    settle_outbound().await;
    {
        let doc = loro_doc.read();
        let props = loro_vertex_props(&doc, "k1").expect("V[k1] exists");
        assert_eq!(
            props.get("city"),
            Some(&LoroValue::String("Lyon".into())),
            "Lyon preserved through outbound merge"
        );
        assert_eq!(
            props.get("country"),
            Some(&LoroValue::String("France".into())),
            "France propagated Grafeo→Loro"
        );
    }

    // Step 3: Loro → Grafeo again. Update V[k1] with an extra property. The
    // Loro subscriber fires with a non-bridge origin, so the inbound worker
    // applies it to grafeo. No echo: the resulting grafeo commit (if any)
    // would be filtered by either the epoch side-channel or the origin filter.
    {
        let doc = loro_doc.read();
        let v_map = doc.get_map("V");
        v_map
            .insert("k1", lmap([
                ("city", LoroValue::String("Lyon".into())),
                ("country", LoroValue::String("France".into())),
                ("pop", LoroValue::I64(500_000)),
            ]))
            .expect("loro update");
        doc.commit();
    }
    settle_inbound().await;
    {
        let session = grafeo_db.session();
        let node = session.get_node(node_id).expect("grafeo node k1 still exists");
        assert_eq!(
            node.get_property("pop"),
            Some(&grafeo::Value::Int64(500_000)),
            "second Loro→Grafeo edit should land"
        );
    }

    // Step 4: No echo. After everything settles, the Loro state should be
    // stable (the second Loro edit was applied; no further echo mutations).
    let snapshot_before = {
        let doc = loro_doc.read();
        doc.get_deep_value().to_json_value()
    };
    settle_outbound().await;
    let snapshot_after = {
        let doc = loro_doc.read();
        doc.get_deep_value().to_json_value()
    };
    assert_eq!(
        snapshot_before, snapshot_after,
        "no echo after bidirectional convergence"
    );

    engine.shutdown();
    for h in handles {
        let _ = h.await;
    }
}
