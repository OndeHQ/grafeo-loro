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

use grafeo_loro::constants::{DEFAULT_BATCH_MS, OUTBOUND_POLL_MS};
use grafeo_loro::types::LoroOp;
use grafeo_loro::{InboundMsg, SyncEngine};

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
        loro_insert_vertex(
            &doc,
            "k1",
            lmap([("name", LoroValue::String("Alice".into()))]),
        );
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
        session.begin_transaction().expect("begin tx");
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
        let props =
            loro_vertex_props(&doc, "k1").expect("V[k1] should exist after outbound update");
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
    //
    // Hunter MAJOR 3: the snapshot-comparison below is timing-dependent
    // (an echo slower than the settle window could slip past it). The
    // deterministic check is the `inbound_event_count` counter — it
    // increments on every op that survives the origin filter. If the
    // filter is broken, the echoed Loro write would translate to a
    // LoroOp::UpsertNode and increment the counter. We snapshot the
    // counter BEFORE the settle window and assert it does not move.
    // The grafeo-side assertion (n.age still == 42) is defense-in-depth:
    // a grafeo echo would mutate the node, and even though the value
    // is idempotent for this specific test, the property must still be
    // present after the settle window.
    let count_before = engine.inbound_event_count();
    let snapshot_before = {
        let doc = loro_doc.read();
        doc.get_deep_value().to_json_value()
    };
    settle_outbound().await;
    let count_after = engine.inbound_event_count();
    let snapshot_after = {
        let doc = loro_doc.read();
        doc.get_deep_value().to_json_value()
    };
    assert_eq!(
        count_before, count_after,
        "inbound_event_count must not increase after the outbound update settled (no echo survived the origin filter)"
    );
    assert_eq!(
        snapshot_before, snapshot_after,
        "Loro state must not change after the outbound update settled (no echo)"
    );
    // Defense-in-depth (Hunter MAJOR 3): grafeo-side assertion.
    {
        let session = grafeo_db.session();
        let age = session.get_node_property(node_id, "age");
        assert_eq!(
            age,
            Some(grafeo::Value::Int64(42)),
            "grafeo node age must still be 42 after the settle window (no echo mutated it)"
        );
    }

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
        loro_insert_vertex(
            &doc,
            "k1",
            lmap([("city", LoroValue::String("Lyon".into()))]),
        );
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
            .insert(
                "k1",
                lmap([
                    ("city", LoroValue::String("Lyon".into())),
                    ("country", LoroValue::String("France".into())),
                    ("pop", LoroValue::I64(500_000)),
                ]),
            )
            .expect("loro update");
        doc.commit();
    }
    settle_inbound().await;
    {
        let session = grafeo_db.session();
        let node = session
            .get_node(node_id)
            .expect("grafeo node k1 still exists");
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

/// Hunter MAJOR 2: edge `Update` events from Grafeo→Loro were silently
/// dropped because `lookup_edge_endpoints` reads `event.src_id`/`dst_id`/
/// `edge_type`, all of which grafeo's `record_update` sets to `None` for
/// every Update event (verified in grafeo-engine-0.5.42/src/cdc.rs:~432).
/// The fix splits the edge Create vs Update arms: Update now looks up the
/// EdgeKey via `maps.edge_key_map` (populated at Create time). This test
/// drives the full Create→Update cycle and asserts the Update lands in Loro.
#[tokio::test]
async fn edge_update_propagates() {
    let grafeo_db = Arc::new(GrafeoDB::new_in_memory());
    let loro_doc = Arc::new(RwLock::new(LoroDoc::new()));
    let (engine, inbound_rx, outbound_rx) = SyncEngine::new(grafeo_db.clone(), loro_doc.clone());
    let engine = Arc::new(engine);
    let handles = engine.clone().spawn_all(inbound_rx, outbound_rx).await;

    // --- Setup: insert vertices "a" and "b" via Loro ---
    {
        let doc = loro_doc.read();
        loro_insert_vertex(
            &doc,
            "a",
            lmap([("name", LoroValue::String("Alice".into()))]),
        );
        loro_insert_vertex(&doc, "b", lmap([("name", LoroValue::String("Bob".into()))]));
    }
    settle_inbound().await;

    // --- Setup: insert edge a|b|KNOWS via Loro (creates grafeo edge + binding) ---
    {
        let doc = loro_doc.read();
        let e_map = doc.get_map("E");
        e_map
            .insert("a|b|KNOWS", lmap([("since", LoroValue::I64(2020))]))
            .expect("loro edge insert");
        doc.commit();
    }
    settle_inbound().await;

    // Verify the edge binding was recorded by the inbound apply path.
    let edge_id = engine
        .maps()
        .edge_id_map
        .read()
        .get(&("a".to_string(), "b".to_string(), "KNOWS".to_string()))
        .copied()
        .expect("bridge should have mapped (a, b, KNOWS) → grafeo EdgeId");

    // --- Grafeo → Loro: SET r.weight = 5 on the bridge-created edge ---
    // This produces a CDC Update event whose `src_id`/`dst_id`/`edge_type`
    // are all `None` (per grafeo's `record_update`). The fix must use the
    // `edge_key_map` reverse lookup to find the Loro-side EdgeKey.
    {
        let mut session = grafeo_db.session_with_cdc(true);
        session.begin_transaction().expect("begin tx");
        session
            .execute("MATCH (n {name: 'Alice'})-[r:KNOWS]->(m {name: 'Bob'}) SET r.weight = 5")
            .expect("MATCH SET on edge");
        session.commit().expect("commit");
    }
    settle_outbound().await;

    // Assert: Loro E["a|b|KNOWS"] now carries {since: 2020, weight: 5}.
    {
        let doc = loro_doc.read();
        let e_map = doc.get_map("E");
        use loro::ValueOrContainer;
        let edge_val = e_map.get("a|b|KNOWS").expect("E[a|b|KNOWS] should exist");
        let props = match edge_val {
            ValueOrContainer::Value(LoroValue::Map(m)) => Some(
                m.iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect::<HashMap<_, _>>(),
            ),
            _ => None,
        };
        let props = props.expect("edge value should be a LoroValue::Map");
        assert_eq!(
            props.get("since"),
            Some(&LoroValue::I64(2020)),
            "original edge property should survive the merge"
        );
        assert_eq!(
            props.get("weight"),
            Some(&LoroValue::I64(5)),
            "edge Update from grafeo should land in Loro (Hunter MAJOR 2)"
        );
    }
    // Sanity: grafeo edge itself should also carry weight=5.
    {
        let session = grafeo_db.session();
        let edge = session.get_edge(edge_id).expect("grafeo edge should exist");
        assert_eq!(
            edge.get_property("weight"),
            Some(&grafeo::Value::Int64(5)),
            "grafeo edge should carry the SET property"
        );
    }

    engine.shutdown();
    for h in handles {
        let _ = h.await;
    }
}

/// Hunter MINOR 7: delete paths were completely untested. This test
/// exercises both directions:
/// (a) **Inbound delete**: push `LoroOp::DeleteNode` via `inbound_sender()`,
///     assert grafeo `get_node` returns `None` and the loro_key mapping is
///     cleared.
/// (b) **Outbound delete**: `MATCH (n {name: 'Alice'}) DELETE n` in grafeo,
///     assert Loro `V["k1"]` is absent after `settle_outbound`.
#[tokio::test]
async fn node_delete_round_trip() {
    let grafeo_db = Arc::new(GrafeoDB::new_in_memory());
    let loro_doc = Arc::new(RwLock::new(LoroDoc::new()));
    let (engine, inbound_rx, outbound_rx) = SyncEngine::new(grafeo_db.clone(), loro_doc.clone());
    let engine = Arc::new(engine);
    let handles = engine.clone().spawn_all(inbound_rx, outbound_rx).await;

    // Pre-populate: Loro → Grafeo insert "k1" {name: Alice}.
    {
        let doc = loro_doc.read();
        loro_insert_vertex(
            &doc,
            "k1",
            lmap([("name", LoroValue::String("Alice".into()))]),
        );
    }
    settle_inbound().await;
    let node_id = engine
        .maps()
        .node_id_map
        .read()
        .get("k1")
        .copied()
        .expect("k1 mapped after Loro→Grafeo flush");
    assert!(
        grafeo_db.session().get_node(node_id).is_some(),
        "precondition: grafeo has node k1"
    );

    // --- (a) Inbound delete: push LoroOp::DeleteNode via inbound_sender ---
    engine
        .inbound_sender()
        .send(InboundMsg::Op(LoroOp::DeleteNode {
            loro_key: "k1".to_string(),
        }))
        .await
        .expect("inbound send");
    settle_inbound().await;
    assert!(
        grafeo_db.session().get_node(node_id).is_none(),
        "inbound delete should remove grafeo node"
    );
    assert!(
        engine.maps().node_id_map.read().get("k1").is_none(),
        "inbound delete should clear the loro_key → NodeId mapping"
    );

    // --- Re-create k1 for the outbound delete half ---
    // LoroMap::insert is a no-op when the value is unchanged, so re-inserting
    // the same `{name: Alice}` via LoroDoc would not produce a subscriber
    // diff. Instead, push the UpsertNode op directly via inbound_sender —
    // this exercises the same apply path (lookup-or-create + insert mapping)
    // without depending on Loro diff semantics.
    let mut props = HashMap::new();
    props.insert(
        "name".to_string(),
        grafeo_loro::types::GraphValue::String("Alice".to_string()),
    );
    engine
        .inbound_sender()
        .send(InboundMsg::Op(LoroOp::UpsertNode {
            loro_key: "k1".to_string(),
            labels: Vec::new(),
            properties: props,
        }))
        .await
        .expect("inbound send (re-create)");
    settle_inbound().await;
    let node_id = engine
        .maps()
        .node_id_map
        .read()
        .get("k1")
        .copied()
        .expect("k1 re-mapped after second Loro→Grafeo flush");
    assert!(
        grafeo_db.session().get_node(node_id).is_some(),
        "precondition: grafeo has node k1 again"
    );

    // --- (b) Outbound delete: MATCH (n {name: 'Alice'}) DELETE n ---
    {
        let mut session = grafeo_db.session_with_cdc(true);
        session.begin_transaction().expect("begin tx");
        session
            .execute("MATCH (n {name: 'Alice'}) DELETE n")
            .expect("MATCH DELETE");
        session.commit().expect("commit");
    }
    settle_outbound().await;

    // Assert: Loro V[k1] is absent.
    {
        let doc = loro_doc.read();
        let v_map = doc.get_map("V");
        assert!(
            v_map.get("k1").is_none(),
            "outbound delete should remove Loro V[k1]"
        );
    }

    engine.shutdown();
    for h in handles {
        let _ = h.await;
    }
}
