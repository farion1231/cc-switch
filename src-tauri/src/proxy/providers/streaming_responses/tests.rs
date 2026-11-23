use super::*;
use futures::stream;
use futures::StreamExt;
use std::collections::HashMap;

async fn convert_stream_text(input: impl Into<Bytes>) -> String {
    let upstream = stream::iter(vec![Ok::<_, std::io::Error>(input.into())]);
    create_anthropic_sse_stream_from_responses(upstream)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .map(|chunk| String::from_utf8_lossy(chunk.unwrap().as_ref()).to_string())
        .collect()
}

async fn convert_stream_text_with_web_search_name(
    input: impl Into<Bytes>,
    hosted_web_search_name: &str,
) -> String {
    let upstream = stream::iter(vec![Ok::<_, std::io::Error>(input.into())]);
    create_anthropic_sse_stream_from_responses_with_web_search_options(
        upstream,
        Some(hosted_web_search_name.to_string()),
        None,
    )
    .collect::<Vec<_>>()
    .await
    .into_iter()
    .map(|chunk| String::from_utf8_lossy(chunk.unwrap().as_ref()).to_string())
    .collect()
}

async fn convert_stream_text_with_web_search_limit(
    input: impl Into<Bytes>,
    hosted_web_search_name: &str,
    max_web_search_uses: u64,
) -> String {
    let upstream = stream::iter(vec![Ok::<_, std::io::Error>(input.into())]);
    create_anthropic_sse_stream_from_responses_with_web_search_options(
        upstream,
        Some(hosted_web_search_name.to_string()),
        Some(max_web_search_uses),
    )
    .collect::<Vec<_>>()
    .await
    .into_iter()
    .map(|chunk| String::from_utf8_lossy(chunk.unwrap().as_ref()).to_string())
    .collect()
}

fn sse_data_values(output: &str) -> Vec<Value> {
    output
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|data| serde_json::from_str(data).ok())
        .collect()
}

async fn convert_raw_stream_text_with_web_search(input: impl Into<Bytes>) -> String {
    let upstream = stream::iter(vec![Ok::<_, std::io::Error>(input.into())]);
    create_anthropic_sse_stream_from_responses_raw(upstream, "web_search".to_string(), None, true)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .map(|chunk| String::from_utf8_lossy(chunk.unwrap().as_ref()).to_string())
        .collect()
}

#[test]
fn test_streamed_text_state_returns_only_the_missing_terminal_suffix() {
    let mut state = StreamedTextState::default();
    let delta = json!({
        "item_id": "msg_partial",
        "output_index": 0,
        "content_index": 0
    });
    state.record_delta(&delta, "Already ");
    state.record_delta(&delta, "streamed");

    assert_eq!(
        state.missing_suffix(
            "Already streamed and completed.",
            Some(0),
            Some("msg_partial"),
            0
        ),
        " and completed."
    );
    assert_eq!(
        state.missing_suffix(
            "Already streamed and completed.",
            Some(0),
            Some("msg_partial"),
            0
        ),
        ""
    );
}

#[test]
fn test_unkeyed_streamed_text_reconciles_with_later_terminal_keys() {
    let mut streamed = StreamedTextState::default();
    streamed.record_delta(&json!({}), "Before search.");
    assert!(streamed.has_part_matching_terminal("Before search.", Some(0), Some("msg_before"), 0));

    let item = json!({
        "id": "msg_before",
        "type": "message",
        "content": [{
            "type": "output_text",
            "text": "Before search.",
            "annotations": []
        }]
    });
    let mut buffered = BufferedCitationTextState::default();
    assert!(
        missing_message_text_parts(&item, Some(0), &mut streamed, Some(&mut buffered)).is_empty()
    );
}

#[test]
fn test_streamed_text_preserves_keyed_then_unkeyed_delta_order() {
    let mut state = StreamedTextState::default();
    let keyed = json!({
        "item_id": "msg_transition",
        "output_index": 0,
        "content_index": 0
    });
    state.record_delta(&keyed, "Hello ");
    state.record_delta(&json!({}), "world");

    assert_eq!(
        state.missing_suffix("Hello world!", Some(0), Some("msg_transition"), 0),
        "!"
    );
}

#[test]
fn test_streamed_text_scopes_unkeyed_order_after_prior_part_finishes() {
    let mut state = StreamedTextState::default();
    let old = json!({
        "item_id": "msg_old",
        "output_index": 0,
        "content_index": 0
    });
    state.record_delta(&old, "Old.");
    state.finish_part(&old);

    state.record_delta(&json!({}), "Hello ");
    let current = json!({
        "item_id": "msg_current",
        "output_index": 1,
        "content_index": 0
    });
    state.record_delta(&current, "world");

    assert_eq!(
        state.missing_suffix("Hello world!", Some(1), Some("msg_current"), 0),
        "!"
    );
}

#[test]
fn test_streamed_text_binds_output_and_item_aliases_to_one_aggregate() {
    let mut state = StreamedTextState::default();
    state.record_delta(
        &json!({"item_id": "msg_alias", "content_index": 0}),
        "Item ",
    );
    state.record_delta(&json!({"output_index": 3, "content_index": 0}), "output ");
    state.record_delta(
        &json!({
            "item_id": "msg_alias",
            "output_index": 3,
            "content_index": 0
        }),
        "joined",
    );

    assert_eq!(
        state.missing_suffix("Item output joined!", Some(3), Some("msg_alias"), 0),
        "!"
    );
    assert_eq!(
        state.missing_suffix("Item output joined!", None, Some("msg_alias"), 0),
        ""
    );
}

#[test]
fn test_streamed_text_preserves_repeated_deltas_when_aliases_merge() {
    let mut state = StreamedTextState::default();
    state.record_delta(&json!({"output_index": 3, "content_index": 0}), "abc");
    state.record_delta(&json!({"item_id": "msg_repeat", "content_index": 0}), "abc");
    state.record_delta(
        &json!({
            "item_id": "msg_repeat",
            "output_index": 3,
            "content_index": 0
        }),
        "!",
    );

    assert_eq!(
        state.missing_suffix("abcabc! tail", Some(3), Some("msg_repeat"), 0),
        " tail"
    );
}

#[test]
fn test_streamed_text_rejects_crossed_established_aliases() {
    let mut state = StreamedTextState::default();
    let first = json!({
        "item_id": "msg_first",
        "output_index": 0,
        "content_index": 0
    });
    let second = json!({
        "item_id": "msg_second",
        "output_index": 1,
        "content_index": 0
    });
    state.record_delta(&first, "First");
    state.record_delta(&second, "Second");
    state.record_delta(
        &json!({
            "item_id": "msg_second",
            "output_index": 0,
            "content_index": 0
        }),
        " crossed",
    );

    assert_eq!(
        state.missing_suffix("First!", Some(0), Some("msg_first"), 0),
        "!"
    );
}

#[test]
fn test_streamed_text_keeps_distinct_unkeyed_tail_after_keyed_terminal_part() {
    let mut state = StreamedTextState::default();
    state.record_delta(
        &json!({
            "item_id": "msg_first",
            "output_index": 0,
            "content_index": 0
        }),
        "First.",
    );
    state.record_delta(&json!({}), "Second.");

    assert_eq!(
        state.missing_suffix("First.", Some(0), Some("msg_first"), 0),
        ""
    );
    assert_eq!(
        state.missing_suffix("Second.", Some(1), Some("msg_second"), 0),
        ""
    );
}

#[test]
fn test_buffered_citations_reject_stale_done_before_next_unkeyed_delta() {
    let mut state = BufferedCitationTextState::default();
    let unkeyed = json!({});

    state.record_part(&unkeyed, &json!({"type": "output_text", "text": ""}), false);
    state.record_delta(&unkeyed, "First.");
    state.record_done_event(&json!({"text": "First."}));
    state.record_part(
        &unkeyed,
        &json!({"type": "output_text", "text": "First.", "annotations": []}),
        true,
    );

    state.record_part(&unkeyed, &json!({"type": "output_text", "text": ""}), false);
    assert!(!state.record_part(
        &unkeyed,
        &json!({"type": "output_text", "text": "First.", "annotations": []}),
        true,
    ));
    state.record_delta(&unkeyed, "Second.");
    state.record_done_event(&json!({"text": "Second."}));

    assert_eq!(state.render_pending_parts(), vec!["First.", "Second."]);
}

#[test]
fn test_buffered_citations_preserve_new_unkeyed_substring_after_part_boundary() {
    let mut state = BufferedCitationTextState::default();
    let unkeyed = json!({});

    state.record_delta(&unkeyed, "foobar");
    state.record_done_event(&json!({"text": "foobar"}));
    assert_eq!(state.render_pending_parts(), vec!["foobar"]);

    state.record_part(&unkeyed, &json!({"type": "output_text", "text": ""}), false);
    assert!(state.record_part(
        &unkeyed,
        &json!({
            "type": "output_text",
            "text": "bar",
            "annotations": [{
                "type": "url_citation",
                "start_index": 0,
                "end_index": 3,
                "url": "https://example.com/bar",
                "title": "Bar"
            }]
        }),
        true,
    ));

    assert_eq!(
        state.render_pending_parts(),
        vec!["[bar](https://example.com/bar)"]
    );
}

#[test]
fn test_buffered_citations_merge_unkeyed_prefix_when_keys_appear() {
    let mut state = BufferedCitationTextState::default();
    state.record_delta(&json!({}), "Prefix ");
    state.record_delta(&json!({"output_index": 2, "content_index": 0}), "suffix.");

    assert_eq!(state.render_pending_parts(), vec!["Prefix suffix."]);
}

#[test]
fn test_buffered_citations_preserve_unkeyed_then_keyed_part_order() {
    let mut state = BufferedCitationTextState::default();
    let unkeyed = json!({});
    state.record_delta(&unkeyed, "First.");
    state.record_done_event(&json!({"text": "First."}));
    state.record_delta(&json!({"output_index": 2, "content_index": 0}), "Second.");

    assert_eq!(state.render_pending_parts(), vec!["First.", "Second."]);
}

#[test]
fn test_buffered_citations_do_not_alias_empty_keyed_part_to_prior_unkeyed_part() {
    let mut state = BufferedCitationTextState::default();
    let unkeyed = json!({});
    state.record_delta(&unkeyed, "First.");
    state.record_done_event(&json!({"text": "First."}));

    let keyed = json!({"output_index": 2, "content_index": 0});
    state.record_part(&keyed, &json!({"type": "output_text", "text": ""}), false);
    state.record_delta(&keyed, "Second.");

    assert_eq!(state.render_pending_parts(), vec!["First.", "Second."]);
}

#[test]
fn test_buffered_citations_merge_item_only_part_when_output_key_appears() {
    let mut state = BufferedCitationTextState::default();
    state.record_delta(
        &json!({"item_id": "msg_transition", "content_index": 0}),
        "Full ",
    );
    state.record_delta(
        &json!({
            "item_id": "msg_transition",
            "output_index": 3,
            "content_index": 0
        }),
        "text.",
    );

    assert_eq!(state.render_pending_parts(), vec!["Full text."]);
}

#[test]
fn test_buffered_citations_keep_item_only_parts_in_arrival_order() {
    let mut state = BufferedCitationTextState::default();
    state.record_delta(&json!({"item_id": "z_item", "content_index": 0}), "First.");
    state.record_done_event(&json!({"item_id": "z_item", "content_index": 0, "text": "First."}));
    state.record_delta(&json!({"item_id": "a_item", "content_index": 0}), "Second.");

    assert_eq!(state.render_pending_parts(), vec!["First.", "Second."]);
}

#[test]
fn test_buffered_citations_render_fuller_aggregate_than_stale_terminal_snapshot() {
    let mut state = BufferedCitationTextState::default();
    let keyed = json!({
        "item_id": "msg_fuller",
        "output_index": 0,
        "content_index": 0
    });
    state.record_delta(&keyed, "Full text.");

    let terminal = json!({"text": "Full", "annotations": []});
    assert_eq!(
        state.render_message_part(&terminal, Some(0), Some("msg_fuller"), 0),
        Some("Full text.".to_string())
    );
    assert!(state.render_pending_parts().is_empty());
}

#[test]
fn test_buffered_citations_preserve_keyed_delta_after_part_was_flushed() {
    let mut state = BufferedCitationTextState::default();
    let keyed = json!({
        "item_id": "msg_delayed",
        "output_index": 0,
        "content_index": 0
    });
    state.record_delta(&keyed, "Before");
    assert_eq!(state.render_pending_parts(), vec!["Before"]);

    state.record_delta(&keyed, " after");
    state.record_done_event(&json!({
        "item_id": "msg_delayed",
        "output_index": 0,
        "content_index": 0,
        "text": "Before after"
    }));
    let terminal = json!({"text": "Before", "annotations": []});
    assert_eq!(
        state.render_message_part(&terminal, Some(0), Some("msg_delayed"), 0),
        Some(" after".to_string())
    );
    assert!(state.render_pending_parts().is_empty());
}

#[test]
fn test_buffered_citations_merge_emitted_and_pending_aliases_without_dropping_tail() {
    let mut state = BufferedCitationTextState::default();
    let output_only = json!({"output_index": 0, "content_index": 0});
    state.record_delta(&output_only, "Before");
    assert_eq!(state.render_pending_parts(), vec!["Before"]);

    let item_only = json!({"item_id": "msg_alias", "content_index": 0});
    state.record_delta(&item_only, " after");
    state.record_delta(
        &json!({
            "item_id": "msg_alias",
            "output_index": 0,
            "content_index": 0
        }),
        "!",
    );

    assert_eq!(state.render_pending_parts(), vec![" after!"]);
}

#[test]
fn test_buffered_citations_keep_post_flush_delta_across_stale_done_snapshot() {
    let mut state = BufferedCitationTextState::default();
    let keyed = json!({
        "item_id": "msg_stale",
        "output_index": 0,
        "content_index": 0
    });
    state.record_delta(&keyed, "Before");
    assert_eq!(state.render_pending_parts(), vec!["Before"]);
    state.record_delta(&keyed, " after");
    state.record_done_event(&json!({
        "item_id": "msg_stale",
        "output_index": 0,
        "content_index": 0,
        "text": "Before"
    }));

    assert_eq!(state.render_pending_parts(), vec![" after"]);
}

#[test]
fn test_buffered_citations_unkeyed_done_after_flush() {
    for (case, done_text) in [
        (
            "test_buffered_citations_reject_stale_unkeyed_done_after_flush",
            "Before",
        ),
        (
            "test_buffered_citations_reconcile_cumulative_unkeyed_done_after_flush",
            "Before after",
        ),
        (
            "test_buffered_citations_ignore_partial_stale_unkeyed_done_after_flush",
            "Bef",
        ),
    ] {
        let mut state = BufferedCitationTextState::default();
        let unkeyed = json!({});
        state.record_delta(&unkeyed, "Before");
        assert_eq!(state.render_pending_parts(), vec!["Before"], "{case}");

        state.record_delta(&unkeyed, " after");
        state.record_done_event(&json!({"text": done_text}));

        assert_eq!(state.render_pending_parts(), vec![" after"], "{case}");
    }
}

#[test]
fn test_buffered_citations_reject_incompatible_cumulative_unkeyed_done() {
    let mut state = BufferedCitationTextState::default();
    state.record_delta(&json!({}), "Before");
    assert_eq!(state.render_pending_parts(), vec!["Before"]);

    state.record_delta(&json!({}), " after");
    state.record_done_event(&json!({"text": "Before stale"}));

    assert_eq!(state.render_pending_parts(), vec![" after"]);
}

#[test]
fn test_buffered_citations_merge_fuller_duplicate_unkeyed_done() {
    let mut state = BufferedCitationTextState::default();
    state.record_delta(&json!({}), "Before");
    state.record_done_event(&json!({"text": "Before"}));
    state.record_done_event(&json!({"text": "Before after"}));

    assert_eq!(state.render_pending_parts(), vec!["Before after"]);
}

#[test]
fn test_buffered_citations_metadata_only_done_closes_keyed_part() {
    let mut state = BufferedCitationTextState::default();
    let keyed = json!({
        "item_id": "msg_first",
        "output_index": 0,
        "content_index": 0
    });
    state.record_delta(&keyed, "First.");
    state.record_done_event(&keyed);
    state.record_delta(&json!({}), "Second.");

    assert_eq!(state.render_pending_parts(), vec!["First.", "Second."]);
}

#[test]
fn test_buffered_citations_do_not_adopt_open_part_with_conflicting_keys() {
    let mut state = BufferedCitationTextState::default();
    state.record_delta(
        &json!({
            "item_id": "msg_first",
            "output_index": 0,
            "content_index": 0
        }),
        "First.",
    );
    state.record_delta(
        &json!({
            "item_id": "msg_second",
            "output_index": 1,
            "content_index": 0
        }),
        "Second.",
    );

    assert_eq!(state.render_pending_parts(), vec!["First.", "Second."]);
}

#[test]
fn test_buffered_citations_preserve_repeated_post_flush_delta() {
    let mut state = BufferedCitationTextState::default();
    let keyed = json!({"output_index": 0, "content_index": 0});
    state.record_delta(&keyed, "abc");
    assert_eq!(state.render_pending_parts(), vec!["abc"]);

    state.record_delta(&keyed, "abc");
    assert_eq!(state.render_pending_parts(), vec!["abc"]);
}

#[test]
fn test_buffered_citations_preserve_repeated_alias_deltas() {
    let mut state = BufferedCitationTextState::default();
    let output_only = json!({"output_index": 0, "content_index": 0});
    state.record_delta(&output_only, "abc");
    assert_eq!(state.render_pending_parts(), vec!["abc"]);

    state.record_delta(&json!({"item_id": "msg_repeat", "content_index": 0}), "abc");
    state.record_delta(
        &json!({
            "item_id": "msg_repeat",
            "output_index": 0,
            "content_index": 0
        }),
        "!",
    );

    assert_eq!(state.render_pending_parts(), vec!["abc!"]);
}

#[test]
fn test_buffered_citations_empty_delta_does_not_make_snapshots_additive() {
    let mut state = BufferedCitationTextState::default();
    let output_only = json!({"output_index": 0, "content_index": 0});
    assert!(state.record_text(&output_only, "abc"));
    state.record_delta(&output_only, "");

    let item_only = json!({"item_id": "msg_snapshot", "content_index": 0});
    state.record_delta(&item_only, "abc");
    let both = json!({
        "item_id": "msg_snapshot",
        "output_index": 0,
        "content_index": 0
    });
    assert!(state.record_text(&both, "abc"));

    assert_eq!(state.render_pending_parts(), vec!["abc"]);
}

#[test]
fn test_buffered_citations_do_not_reuse_last_unkeyed_part_for_conflicting_snapshot_key() {
    let mut state = BufferedCitationTextState::default();
    state.record_delta(&json!({}), "Same.");
    assert!(state.record_text(
        &json!({
            "item_id": "msg_first",
            "output_index": 0,
            "content_index": 0
        }),
        "Same."
    ));
    assert!(state.record_text(
        &json!({
            "item_id": "msg_second",
            "output_index": 1,
            "content_index": 0
        }),
        "Same."
    ));

    assert_eq!(state.render_pending_parts(), vec!["Same.", "Same."]);
}

#[test]
fn test_buffered_citations_reject_crossed_established_aliases() {
    let mut state = BufferedCitationTextState::default();
    state.record_delta(
        &json!({
            "item_id": "msg_first",
            "output_index": 0,
            "content_index": 0
        }),
        "First.",
    );
    state.record_done_event(&json!({
        "item_id": "msg_first",
        "output_index": 0,
        "content_index": 0,
        "text": "First."
    }));
    state.record_delta(
        &json!({
            "item_id": "msg_second",
            "output_index": 1,
            "content_index": 0
        }),
        "Second.",
    );
    state.record_delta(
        &json!({
            "item_id": "msg_second",
            "output_index": 0,
            "content_index": 0
        }),
        "Crossed.",
    );

    assert_eq!(state.render_pending_parts(), vec!["First.", "Second."]);
}

#[test]
fn test_buffered_citations_reconcile_fuller_terminal_after_keyed_flush() {
    let mut state = BufferedCitationTextState::default();
    let keyed = json!({
        "item_id": "msg_terminal",
        "output_index": 0,
        "content_index": 0
    });
    state.record_delta(&keyed, "Before");
    assert_eq!(state.render_pending_parts(), vec!["Before"]);

    let terminal = json!({
        "text": "Before after",
        "annotations": [{
            "type": "url_citation",
            "start_index": 7,
            "end_index": 12,
            "url": "https://example.com/after",
            "title": "After"
        }]
    });
    let rendered = state
        .render_message_part(&terminal, Some(0), Some("msg_terminal"), 0)
        .expect("fuller terminal suffix and citation should be emitted");
    assert!(rendered.starts_with(" after"));
    assert!(rendered.contains("https://example.com/after"));
}

#[test]
fn test_buffered_citations_consume_pending_unkeyed_suffix_after_emitted_history() {
    let mut state = BufferedCitationTextState::default();
    let unkeyed = json!({});
    state.record_delta(&unkeyed, "First");
    assert_eq!(state.render_pending_parts(), vec!["First"]);
    state.record_delta(&unkeyed, "Second");

    let terminal = json!({"text": "FirstSecond", "annotations": []});
    assert_eq!(
        state.render_message_part(&terminal, Some(0), Some("msg_combined"), 0),
        Some("Second".to_string())
    );
    assert!(state.render_pending_parts().is_empty());
}

#[test]
fn test_buffered_citations_do_not_discard_keyed_tail_for_stale_unkeyed_snapshot() {
    let mut state = BufferedCitationTextState::default();
    state.record_delta(&json!({}), "Before");
    assert_eq!(state.render_pending_parts(), vec!["Before"]);

    let keyed = json!({
        "item_id": "msg_tail",
        "output_index": 0,
        "content_index": 0
    });
    state.record_delta(&keyed, " after");
    let stale = json!({"text": "Before", "annotations": []});
    assert_eq!(
        state.render_message_part(&stale, Some(0), Some("msg_tail"), 0),
        Some(" after".to_string())
    );

    let complete = json!({"text": "Before after", "annotations": []});
    assert_eq!(
        state.render_message_part(&complete, Some(0), Some("msg_tail"), 0),
        None
    );
}

#[test]
fn test_buffered_citations_render_later_unkeyed_text_after_keyed_part() {
    let mut state = BufferedCitationTextState::default();
    let keyed = json!({"output_index": 0, "content_index": 0});
    state.record_delta(&keyed, "Keyed.");
    assert_eq!(state.render_pending_parts(), vec!["Keyed."]);

    let unkeyed = json!({});
    state.record_delta(&unkeyed, "Rust docs.");
    state.record_annotation(
        &unkeyed,
        &json!({
            "type": "url_citation",
            "start_index": 0,
            "end_index": 4,
            "url": "https://www.rust-lang.org/",
            "title": "Rust"
        }),
    );

    assert_eq!(
        state.render_pending_parts(),
        vec!["[Rust](https://www.rust-lang.org/) docs."]
    );
}

#[test]
fn test_buffered_citations_do_not_replay_emitted_unkeyed_history() {
    let mut state = BufferedCitationTextState::default();
    let unkeyed = json!({});
    state.record_delta(&unkeyed, "First.");
    assert_eq!(state.render_pending_parts(), vec!["First."]);

    state.record_delta(&unkeyed, "Second.");
    let first = json!({"text": "First.", "annotations": []});
    assert_eq!(
        state.render_message_part(&first, Some(0), Some("msg_first"), 0),
        None
    );
    let second = json!({"text": "Second.", "annotations": []});
    assert_eq!(
        state.render_message_part(&second, Some(1), Some("msg_second"), 0),
        Some("Second.".to_string())
    );
    assert!(state.render_pending_parts().is_empty());
}

#[test]
fn test_buffered_citations_filter_citations_emitted_with_unkeyed_history() {
    let mut state = BufferedCitationTextState::default();
    let already_emitted = json!({
        "type": "url_citation",
        "start_index": 0,
        "end_index": 4,
        "url": "https://www.rust-lang.org/",
        "title": "Rust"
    });
    state.record_delta(&json!({}), "Rust");
    state.record_annotation(&json!({}), &already_emitted);
    assert_eq!(
        state.render_pending_parts(),
        vec!["[Rust](https://www.rust-lang.org/)"]
    );

    let terminal_only = json!({
        "type": "url_citation",
        "start_index": 0,
        "end_index": 4,
        "url": "https://doc.rust-lang.org/",
        "title": "Rust docs"
    });
    let terminal = json!({
        "text": "Rust",
        "annotations": [already_emitted, terminal_only]
    });
    let rendered = state
        .render_message_part(&terminal, Some(0), Some("msg_rust"), 0)
        .expect("the terminal-only citation should still be emitted");
    assert!(!rendered.contains("https://www.rust-lang.org/"));
    assert!(rendered.contains("https://doc.rust-lang.org/"));
}

#[test]
fn test_buffered_citations_keep_same_citation_on_distinct_keyed_tail() {
    let mut state = BufferedCitationTextState::default();
    let citation = json!({
        "type": "url_citation",
        "start_index": 0,
        "end_index": 4,
        "url": "https://www.rust-lang.org/",
        "title": "Rust"
    });
    state.record_delta(&json!({}), "Rust");
    state.record_annotation(&json!({}), &citation);
    assert_eq!(
        state.render_pending_parts(),
        vec!["[Rust](https://www.rust-lang.org/)"]
    );

    let keyed = json!({
        "item_id": "msg_tail",
        "output_index": 0,
        "content_index": 0
    });
    state.record_delta(&keyed, "Rust");
    let terminal = json!({"text": "Rust", "annotations": [citation]});
    assert_eq!(
        state.render_message_part(&terminal, Some(0), Some("msg_tail"), 0),
        Some("[Rust](https://www.rust-lang.org/)".to_string())
    );
}

#[test]
fn test_buffered_citations_collects_only_pending_keyed_annotations() {
    let mut state = BufferedCitationTextState::default();
    let keyed = json!({
        "item_id": "msg_annotation",
        "output_index": 0,
        "content_index": 0
    });
    let citation = json!({
        "type": "url_citation",
        "start_index": 0,
        "end_index": 4,
        "url": "https://www.rust-lang.org/",
        "title": "Rust"
    });
    state.record_annotation(&keyed, &citation);
    assert_eq!(
        state.render_pending_parts(),
        vec!["Sources: [Rust](https://www.rust-lang.org/)"]
    );

    let terminal = json!({"text": "Rust", "annotations": [citation]});
    assert!(state
        .collect_annotations(
            Some((0, 0)),
            Some(&("msg_annotation".to_string(), 0)),
            Some(&terminal)
        )
        .is_empty());
}

#[test]
fn test_buffered_citations_retain_terminal_only_part_for_late_enrichment() {
    let mut state = BufferedCitationTextState::default();
    let first = json!({"text": "Before", "annotations": []});
    assert_eq!(
        state.render_message_part(&first, Some(0), Some("msg_terminal_only"), 0),
        Some("Before".to_string())
    );

    let enriched = json!({
        "text": "Before after",
        "annotations": [{
            "type": "url_citation",
            "start_index": 7,
            "end_index": 12,
            "url": "https://example.com/after",
            "title": "After"
        }]
    });
    let rendered = state
        .render_message_part(&enriched, Some(0), Some("msg_terminal_only"), 0)
        .expect("late suffix and citation should be emitted");
    assert!(rendered.starts_with(" after"));
    assert!(rendered.contains("https://example.com/after"));
}

#[test]
fn test_buffered_citations_win_over_unrelated_pre_search_unkeyed_text() {
    let mut streamed = StreamedTextState::default();
    streamed.record_delta(&json!({}), "Before search.");

    let mut buffered = BufferedCitationTextState::default();
    buffered.record_delta(
        &json!({
            "item_id": "msg_after",
            "output_index": 2,
            "content_index": 0
        }),
        "After search.",
    );
    let item = json!({
        "id": "msg_after",
        "type": "message",
        "content": [{
            "type": "output_text",
            "text": "After search.",
            "annotations": []
        }]
    });

    assert_eq!(
        missing_message_text_parts(&item, Some(2), &mut streamed, Some(&mut buffered)),
        vec!["After search."]
    );
    assert_eq!(
        streamed.missing_suffix("Before search.", Some(0), Some("msg_before"), 0),
        ""
    );
}

#[test]
fn test_map_responses_stop_reason_tool_use() {
    assert_eq!(
        map_responses_stop_reason(Some("completed"), true, None),
        Some("tool_use")
    );
    assert_eq!(
        map_responses_stop_reason(Some("completed"), false, None),
        Some("end_turn")
    );
    assert_eq!(
        map_responses_stop_reason(Some("incomplete"), false, Some("max_output_tokens")),
        Some("max_tokens")
    );
    assert_eq!(
        map_responses_stop_reason(Some("incomplete"), false, Some("content_filter")),
        Some("end_turn")
    );
}

#[test]
fn test_response_object_from_event_with_wrapper() {
    let data = json!({
        "type": "response.created",
        "response": {
            "id": "resp_1",
            "model": "gpt-4o"
        }
    });
    let obj = response_object_from_event(&data);
    assert_eq!(obj["id"], "resp_1");
    assert_eq!(obj["model"], "gpt-4o");
}

#[tokio::test]
async fn test_response_failed_event_becomes_anthropic_error() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-5\"}}\n\n",
        "event: response.failed\n",
        "data: {\"type\":\"response.failed\",\"response\":{\"status\":\"failed\",\"error\":{\"type\":\"server_error\",\"message\":\"backend exploded\"}}}\n\n"
    );

    let merged = convert_stream_text(input).await;
    assert!(merged.contains("event: error"));
    assert!(merged.contains("backend exploded"));
    assert!(!merged.contains("event: message_stop"));
}

#[tokio::test]
async fn test_late_delta_after_failure_does_not_emit_message_start() {
    let input = concat!(
        "event: response.failed\n",
        "data: {\"type\":\"response.failed\",\"response\":{\"status\":\"failed\",\"error\":{\"message\":\"boom\"}}}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"too late\"}\n\n"
    );

    let merged = convert_stream_text(input).await;
    assert!(merged.contains("event: error"));
    assert!(!merged.contains("event: message_start"));
    assert!(!merged.contains("too late"));
}

#[tokio::test]
async fn test_completed_event_with_failed_status_is_error() {
    let input = concat!(
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"failed\",\"error\":{\"type\":\"server_error\",\"message\":\"failed wrapper\"},\"output\":[]}}\n\n"
    );

    let merged = convert_stream_text(input).await;
    assert!(merged.contains("event: error"));
    assert!(merged.contains("failed wrapper"));
    assert!(!merged.contains("event: message_stop"));
}

#[tokio::test]
async fn test_response_incomplete_event_terminates_with_max_tokens() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-5\"}}\n\n",
        "event: response.incomplete\n",
        "data: {\"type\":\"response.incomplete\",\"response\":{\"status\":\"incomplete\",\"incomplete_details\":{\"reason\":\"max_output_tokens\"},\"usage\":{\"input_tokens\":10,\"output_tokens\":3}}}\n\n"
    );

    let merged = convert_stream_text(input).await;
    assert!(merged.contains("\"stop_reason\":\"max_tokens\""));
    assert!(merged.contains("event: message_stop"));
    assert!(!merged.contains("event: error"));
}

#[tokio::test]
async fn test_response_incomplete_event_without_status_uses_event_fallback() {
    let input = concat!(
        "event: response.incomplete\n",
        "data: {\"type\":\"response.incomplete\",\"response\":{\"usage\":{\"output_tokens\":3}}}\n\n"
    );

    let merged = convert_stream_text(input).await;
    assert!(merged.contains("\"stop_reason\":\"max_tokens\""));
    assert!(merged.contains("event: message_stop"));
}

#[tokio::test]
async fn test_response_incomplete_zero_usage_empty_output_is_upstream_error() {
    // Shape returned by the ChatGPT Codex backend when it rejects the
    // request before processing it (e.g. over a tool schema pattern):
    // incomplete/max_output_tokens with zero usage and an empty output.
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-5\"}}\n\n",
        "event: response.incomplete\n",
        "data: {\"type\":\"response.incomplete\",\"response\":{\"id\":\"resp_1\",\"status\":\"incomplete\",\"incomplete_details\":{\"reason\":\"max_output_tokens\"},\"usage\":{\"input_tokens\":0,\"output_tokens\":0},\"output\":[]}}\n\n"
    );

    let merged = convert_stream_text(input).await;
    assert!(merged.contains("event: message_start"));
    assert!(merged.contains("event: error"));
    assert!(merged.contains("Upstream rejected the request before processing"));
    assert!(!merged.contains("\"stop_reason\":\"max_tokens\""));
    assert!(!merged.contains("event: message_stop"));
}

#[tokio::test]
async fn test_response_incomplete_zero_usage_without_output_array_is_upstream_error() {
    // Same rejection without an `output` array, which skips the
    // empty-output fast path and takes the regular terminal handling.
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-5\"}}\n\n",
        "event: response.incomplete\n",
        "data: {\"type\":\"response.incomplete\",\"response\":{\"id\":\"resp_1\",\"status\":\"incomplete\",\"incomplete_details\":{\"reason\":\"max_output_tokens\"},\"usage\":{\"input_tokens\":0,\"output_tokens\":0}}}\n\n"
    );

    let merged = convert_stream_text(input).await;
    assert!(merged.contains("event: error"));
    assert!(merged.contains("Upstream rejected the request before processing"));
    assert!(!merged.contains("\"stop_reason\":\"max_tokens\""));
}

#[tokio::test]
async fn test_response_incomplete_zero_input_nonzero_output_keeps_max_tokens() {
    // Non-zero output usage means tokens were actually generated: keep
    // reporting it as truncation.
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-5\"}}\n\n",
        "event: response.incomplete\n",
        "data: {\"type\":\"response.incomplete\",\"response\":{\"id\":\"resp_1\",\"status\":\"incomplete\",\"incomplete_details\":{\"reason\":\"max_output_tokens\"},\"usage\":{\"input_tokens\":0,\"output_tokens\":7},\"output\":[]}}\n\n"
    );

    let merged = convert_stream_text(input).await;
    assert!(merged.contains("\"stop_reason\":\"max_tokens\""));
    assert!(merged.contains("event: message_stop"));
    assert!(!merged.contains("event: error"));
}

#[test]
fn test_responses_json_to_anthropic_sse_zero_usage_incomplete_is_error() {
    // Also covers the compatible-gateway path that feeds a full Responses
    // JSON document straight into the converter.
    let events = responses_json_to_anthropic_sse(
        json!({
            "id": "resp_1",
            "status": "incomplete",
            "incomplete_details": {"reason": "max_output_tokens"},
            "usage": {"input_tokens": 0, "output_tokens": 0},
            "output": []
        }),
        None,
        None,
    );
    let merged = events
        .iter()
        .map(|event| String::from_utf8_lossy(event).to_string())
        .collect::<String>();
    assert!(merged.contains("event: message_start"));
    assert!(merged.contains("event: error"));
    assert!(merged.contains("Upstream rejected the request before processing"));
    assert!(!merged.contains("\"stop_reason\":\"max_tokens\""));
}

#[tokio::test]
async fn test_streaming_hosted_web_search_emits_anthropic_server_tool_blocks() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_search\",\"model\":\"gpt-5.6\"}}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"ws_123\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"ws_123\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"Rust official documentation\",\"sources\":[{\"type\":\"url\",\"url\":\"https://doc.rust-lang.org/\"}]}}}\n\n",
        "event: response.content_part.added\n",
        "data: {\"type\":\"response.content_part.added\",\"output_index\":1,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"\"}}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"output_index\":1,\"content_index\":0,\"delta\":\"Rust docs are online.\"}\n\n",
        "event: response.output_text.annotation.added\n",
        "data: {\"type\":\"response.output_text.annotation.added\",\"output_index\":1,\"content_index\":0,\"annotation_index\":0,\"annotation\":{\"type\":\"url_citation\",\"start_index\":0,\"end_index\":9,\"url\":\"https://doc.rust-lang.org/\",\"title\":\"Rust Documentation\"}}\n\n",
        "event: response.output_text.done\n",
        "data: {\"type\":\"response.output_text.done\",\"output_index\":1,\"content_index\":0,\"text\":\"Rust docs are online.\"}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":1,\"item\":{\"id\":\"msg_123\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Rust docs are online.\",\"annotations\":[{\"type\":\"url_citation\",\"start_index\":0,\"end_index\":9,\"url\":\"https://doc.rust-lang.org/\",\"title\":\"Rust Documentation\"}]}]}}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_search\",\"status\":\"completed\",\"usage\":{\"input_tokens\":10,\"output_tokens\":12}}}\n\n"
    );

    let merged = convert_stream_text_with_web_search_name(input, "web_search_next").await;
    let events = sse_data_values(&merged);
    let content_block_types: Vec<&str> = events
        .iter()
        .filter_map(|event| event.get("content_block"))
        .filter_map(|block| block.get("type").and_then(Value::as_str))
        .collect();
    assert_eq!(
        content_block_types,
        vec!["server_tool_use", "web_search_tool_result", "text"]
    );
    assert_eq!(merged.matches("\"type\":\"server_tool_use\"").count(), 1);
    assert_eq!(
        merged
            .matches("\"type\":\"web_search_tool_result\"")
            .count(),
        1
    );
    assert!(merged.contains("\"id\":\"ws_123\""));
    assert!(merged.contains("\"name\":\"web_search_next\""));
    assert!(
        merged.contains("\"partial_json\":\"{\\\"query\\\":\\\"Rust official documentation\\\"}\"")
    );
    assert!(merged.contains("https://doc.rust-lang.org/"));
    assert!(merged.contains("\"title\":\"Rust Documentation\""));
    let text_deltas: Vec<&str> = events
        .iter()
        .filter(|event| event.pointer("/delta/type").and_then(Value::as_str) == Some("text_delta"))
        .filter_map(|event| event.pointer("/delta/text").and_then(Value::as_str))
        .collect();
    assert_eq!(
        text_deltas,
        vec!["[Rust docs](https://doc.rust-lang.org/) are online."]
    );
    assert!(merged.contains("\"stop_reason\":\"end_turn\""));
    assert!(!merged.contains("\"stop_reason\":\"tool_use\""));
    assert!(merged.contains("\"web_search_requests\":1"));
    assert!(merged.contains("event: message_stop"));
}

#[tokio::test]
async fn test_buffered_hosted_web_search_text_emits_keepalive_ping() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_keepalive\",\"model\":\"gpt-5.6\"}}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"ws_keepalive\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"ws_keepalive\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"Rust docs\"}}}\n\n",
        "event: response.content_part.added\n",
        "data: {\"type\":\"response.content_part.added\",\"output_index\":1,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"\"}}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"output_index\":1,\"content_index\":0,\"delta\":\"Still synthesizing the answer.\"}\n\n"
    );

    let merged = convert_raw_stream_text_with_web_search(input).await;

    assert_eq!(merged.matches("event: ping").count(), 1);
    assert!(!merged.contains("\"type\":\"text_delta\""));
}

#[tokio::test]
async fn test_terminal_search_recovery_reuses_reserved_text_index() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_recovery\",\"model\":\"gpt-5.6\"}}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"ws_recovery\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"ws_recovery\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"Rust docs\",\"sources\":[{\"type\":\"url\",\"url\":\"https://doc.rust-lang.org/\",\"title\":\"Rust Documentation\"}]}}}\n\n",
        "event: response.content_part.added\n",
        "data: {\"type\":\"response.content_part.added\",\"item_id\":\"msg_recovery\",\"output_index\":1,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"\"}}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_recovery\",\"output_index\":1,\"content_index\":0,\"delta\":\"Rust docs are online.\"}\n\n",
        "event: response.output_text.annotation.added\n",
        "data: {\"type\":\"response.output_text.annotation.added\",\"item_id\":\"msg_recovery\",\"output_index\":1,\"content_index\":0,\"annotation_index\":0,\"annotation\":{\"type\":\"url_citation\",\"start_index\":0,\"end_index\":9,\"url\":\"https://doc.rust-lang.org/\",\"title\":\"Rust Documentation\"}}\n\n",
        "event: response.output_text.done\n",
        "data: {\"type\":\"response.output_text.done\",\"item_id\":\"msg_recovery\",\"output_index\":1,\"content_index\":0,\"text\":\"Rust docs are online.\"}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_recovery\",\"model\":\"gpt-5.6\",\"status\":\"completed\",\"output\":[{\"id\":\"ws_recovery\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"Rust docs\",\"sources\":[{\"type\":\"url\",\"url\":\"https://doc.rust-lang.org/\",\"title\":\"Rust Documentation\"}]}},{\"id\":\"msg_recovery\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Rust docs are online.\",\"annotations\":[{\"type\":\"url_citation\",\"start_index\":0,\"end_index\":9,\"url\":\"https://doc.rust-lang.org/\",\"title\":\"Rust Documentation\"}]}]}],\"usage\":{\"input_tokens\":8,\"output_tokens\":5}}}\n\n"
    );

    let merged = convert_stream_text_with_web_search_name(input, "web_search").await;
    let events = sse_data_values(&merged);
    let started_indices: Vec<u64> = events
        .iter()
        .filter(|event| event.get("type").and_then(Value::as_str) == Some("content_block_start"))
        .filter_map(|event| event.get("index").and_then(Value::as_u64))
        .collect();
    let text_deltas: Vec<&str> = events
        .iter()
        .filter(|event| event.pointer("/delta/type").and_then(Value::as_str) == Some("text_delta"))
        .filter_map(|event| event.pointer("/delta/text").and_then(Value::as_str))
        .collect();

    assert_eq!(started_indices, vec![0, 1, 2]);
    assert_eq!(
        text_deltas,
        vec!["[Rust docs](https://doc.rust-lang.org/) are online."]
    );
    assert!(merged.contains("event: message_stop"));
}

#[tokio::test]
async fn test_terminal_search_reconciles_unkeyed_buffered_text() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_unkeyed\",\"model\":\"gpt-5.6\"}}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"ws_unkeyed\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"ws_unkeyed\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"Rust docs\"}}}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Rust docs are online.\"}\n\n",
        "event: response.output_text.annotation.added\n",
        "data: {\"type\":\"response.output_text.annotation.added\",\"annotation\":{\"type\":\"url_citation\",\"start_index\":0,\"end_index\":9,\"url\":\"https://doc.rust-lang.org/\",\"title\":\"Rust Documentation\"}}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_unkeyed\",\"model\":\"gpt-5.6\",\"status\":\"completed\",\"output\":[{\"id\":\"ws_unkeyed\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"Rust docs\"}},{\"id\":\"msg_unkeyed\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Rust docs are online.\",\"annotations\":[]}]}],\"usage\":{\"input_tokens\":8,\"output_tokens\":5}}}\n\n"
    );

    let merged = convert_stream_text_with_web_search_name(input, "web_search").await;
    let text_deltas: Vec<String> = sse_data_values(&merged)
        .into_iter()
        .filter(|event| event.pointer("/delta/type").and_then(Value::as_str) == Some("text_delta"))
        .filter_map(|event| {
            event
                .pointer("/delta/text")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
        .collect();

    assert_eq!(
        text_deltas,
        vec!["[Rust docs](https://doc.rust-lang.org/) are online."]
    );
    assert!(merged.contains("event: message_stop"));
}

#[tokio::test]
async fn test_terminal_search_partitions_unkeyed_citations_across_text_parts() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_unkeyed_parts\",\"model\":\"gpt-5.6\"}}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"ws_unkeyed_parts\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"ws_unkeyed_parts\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"Rust and Cargo docs\"}}}\n\n",
        "event: response.content_part.added\n",
        "data: {\"type\":\"response.content_part.added\",\"part\":{\"type\":\"output_text\",\"text\":\"\"}}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Rust docs.\"}\n\n",
        "event: response.output_text.annotation.added\n",
        "data: {\"type\":\"response.output_text.annotation.added\",\"annotation\":{\"type\":\"url_citation\",\"start_index\":0,\"end_index\":4,\"url\":\"https://www.rust-lang.org/\",\"title\":\"Rust\"}}\n\n",
        "event: response.output_text.done\n",
        "data: {\"type\":\"response.output_text.done\",\"text\":\"Rust docs.\",\"annotations\":[{\"type\":\"url_citation\",\"start_index\":0,\"end_index\":4,\"url\":\"https://www.rust-lang.org/\",\"title\":\"Rust\"}]}\n\n",
        "event: response.content_part.done\n",
        "data: {\"type\":\"response.content_part.done\",\"part\":{\"type\":\"output_text\",\"text\":\"Rust docs.\",\"annotations\":[{\"type\":\"url_citation\",\"start_index\":0,\"end_index\":4,\"url\":\"https://www.rust-lang.org/\",\"title\":\"Rust\"}]}}\n\n",
        "event: response.content_part.added\n",
        "data: {\"type\":\"response.content_part.added\",\"part\":{\"type\":\"output_text\",\"text\":\"\"}}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Cargo docs.\"}\n\n",
        "event: response.content_part.done\n",
        "data: {\"type\":\"response.content_part.done\",\"part\":{\"type\":\"output_text\",\"text\":\"Rust docs.\",\"annotations\":[{\"type\":\"url_citation\",\"start_index\":0,\"end_index\":4,\"url\":\"https://www.rust-lang.org/\",\"title\":\"Rust\"}]}}\n\n",
        "event: response.output_text.annotation.added\n",
        "data: {\"type\":\"response.output_text.annotation.added\",\"annotation\":{\"type\":\"url_citation\",\"start_index\":0,\"end_index\":5,\"url\":\"https://doc.rust-lang.org/cargo/\",\"title\":\"Cargo\"}}\n\n",
        "event: response.output_text.done\n",
        "data: {\"type\":\"response.output_text.done\",\"text\":\"Cargo docs.\",\"annotations\":[{\"type\":\"url_citation\",\"start_index\":0,\"end_index\":5,\"url\":\"https://doc.rust-lang.org/cargo/\",\"title\":\"Cargo\"}]}\n\n",
        "event: response.content_part.done\n",
        "data: {\"type\":\"response.content_part.done\",\"part\":{\"type\":\"output_text\",\"text\":\"Cargo docs.\",\"annotations\":[{\"type\":\"url_citation\",\"start_index\":0,\"end_index\":5,\"url\":\"https://doc.rust-lang.org/cargo/\",\"title\":\"Cargo\"}]}}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_unkeyed_parts\",\"model\":\"gpt-5.6\",\"status\":\"completed\",\"output\":[{\"id\":\"ws_unkeyed_parts\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"Rust and Cargo docs\"}},{\"id\":\"msg_unkeyed_parts\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Rust docs.\",\"annotations\":[]},{\"type\":\"output_text\",\"text\":\"Cargo docs.\",\"annotations\":[]}]}],\"usage\":{\"input_tokens\":8,\"output_tokens\":5}}}\n\n"
    );

    let merged = convert_stream_text_with_web_search_name(input, "web_search").await;
    let text_deltas: Vec<String> = sse_data_values(&merged)
        .into_iter()
        .filter(|event| event.pointer("/delta/type").and_then(Value::as_str) == Some("text_delta"))
        .filter_map(|event| {
            event
                .pointer("/delta/text")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
        .collect();

    assert_eq!(
        text_deltas,
        vec![
            "[Rust](https://www.rust-lang.org/) docs.",
            "[Cargo](https://doc.rust-lang.org/cargo/) docs."
        ]
    );
    assert!(merged.contains("event: message_stop"));
}

#[tokio::test]
async fn test_text_streamed_before_search_is_not_replayed_after_buffering_starts() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_transition\",\"model\":\"gpt-5.6\"}}\n\n",
        "event: response.content_part.added\n",
        "data: {\"type\":\"response.content_part.added\",\"item_id\":\"msg_before\",\"output_index\":0,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"\"}}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_before\",\"output_index\":0,\"content_index\":0,\"delta\":\"Before search.\"}\n\n",
        "event: response.output_text.done\n",
        "data: {\"type\":\"response.output_text.done\",\"item_id\":\"msg_before\",\"output_index\":0,\"content_index\":0,\"text\":\"Before search.\"}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":1,\"item\":{\"id\":\"ws_transition\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":1,\"item\":{\"id\":\"ws_transition\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"Rust docs\",\"sources\":[{\"type\":\"url\",\"url\":\"https://doc.rust-lang.org/\",\"title\":\"Rust Documentation\"}]}}}\n\n",
        "event: response.content_part.added\n",
        "data: {\"type\":\"response.content_part.added\",\"item_id\":\"msg_after\",\"output_index\":2,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"\"}}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_after\",\"output_index\":2,\"content_index\":0,\"delta\":\"Rust docs are online.\"}\n\n",
        "event: response.output_text.annotation.added\n",
        "data: {\"type\":\"response.output_text.annotation.added\",\"item_id\":\"msg_after\",\"output_index\":2,\"content_index\":0,\"annotation_index\":0,\"annotation\":{\"type\":\"url_citation\",\"start_index\":0,\"end_index\":9,\"url\":\"https://doc.rust-lang.org/\",\"title\":\"Rust Documentation\"}}\n\n",
        "event: response.output_text.done\n",
        "data: {\"type\":\"response.output_text.done\",\"item_id\":\"msg_after\",\"output_index\":2,\"content_index\":0,\"text\":\"Rust docs are online.\"}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":2,\"item\":{\"id\":\"msg_after\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Rust docs are online.\",\"annotations\":[{\"type\":\"url_citation\",\"start_index\":0,\"end_index\":9,\"url\":\"https://doc.rust-lang.org/\",\"title\":\"Rust Documentation\"}]}]}}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_transition\",\"model\":\"gpt-5.6\",\"status\":\"completed\",\"output\":[{\"id\":\"msg_before\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Before search.\",\"annotations\":[]}]},{\"id\":\"ws_transition\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"Rust docs\",\"sources\":[{\"type\":\"url\",\"url\":\"https://doc.rust-lang.org/\",\"title\":\"Rust Documentation\"}]}},{\"id\":\"msg_after\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Rust docs are online.\",\"annotations\":[{\"type\":\"url_citation\",\"start_index\":0,\"end_index\":9,\"url\":\"https://doc.rust-lang.org/\",\"title\":\"Rust Documentation\"}]}]}],\"usage\":{\"input_tokens\":8,\"output_tokens\":8}}}\n\n"
    );

    let merged = convert_stream_text_with_web_search_name(input, "web_search").await;
    let text_deltas: Vec<String> = sse_data_values(&merged)
        .into_iter()
        .filter(|event| event.pointer("/delta/type").and_then(Value::as_str) == Some("text_delta"))
        .filter_map(|event| {
            event
                .pointer("/delta/text")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
        .collect();

    assert_eq!(
        text_deltas,
        vec![
            "Before search.",
            "[Rust docs](https://doc.rust-lang.org/) are online."
        ]
    );
}

#[tokio::test]
async fn test_streaming_hosted_web_search_preserves_failed_result() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_failed_search\",\"model\":\"gpt-5.6\"}}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"ws_failed\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"ws_failed\",\"type\":\"web_search_call\",\"status\":\"failed\",\"action\":{\"type\":\"search\",\"query\":\"latest docs\"},\"error\":{\"code\":\"invalid_search_query\"}}}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_failed_search\",\"status\":\"completed\",\"usage\":{\"input_tokens\":4,\"output_tokens\":1}}}\n\n"
    );

    let merged = convert_stream_text_with_web_search_name(input, "web_search_future").await;
    let events = sse_data_values(&merged);
    let result = events
        .iter()
        .filter_map(|event| event.get("content_block"))
        .find(|block| block.get("type").and_then(Value::as_str) == Some("web_search_tool_result"))
        .unwrap();

    assert_eq!(result["tool_use_id"], "ws_failed");
    assert_eq!(
        result["content"],
        json!({
            "type": "web_search_tool_result_error",
            "error_code": "invalid_tool_input"
        })
    );
    assert!(merged.contains("\"web_search_requests\":1"));
    assert!(merged.contains("event: message_stop"));
}

#[tokio::test]
async fn test_streaming_incomplete_hosted_web_search_emits_error_result() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_unfinished_search\",\"model\":\"gpt-5.6\"}}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"ws_unfinished\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
        "event: response.incomplete\n",
        "data: {\"type\":\"response.incomplete\",\"response\":{\"id\":\"resp_unfinished_search\",\"status\":\"incomplete\",\"incomplete_details\":{\"reason\":\"max_output_tokens\"},\"usage\":{\"input_tokens\":4,\"output_tokens\":1}}}\n\n"
    );

    let merged = convert_stream_text_with_web_search_name(input, "web_search_future").await;
    let events = sse_data_values(&merged);
    let result = events
        .iter()
        .filter_map(|event| event.get("content_block"))
        .find(|block| block.get("type").and_then(Value::as_str) == Some("web_search_tool_result"))
        .unwrap();

    assert_eq!(result["tool_use_id"], "ws_unfinished");
    assert_eq!(
        result["content"],
        json!({
            "type": "web_search_tool_result_error",
            "error_code": "unavailable"
        })
    );
    assert!(merged.contains("\"stop_reason\":\"max_tokens\""));
    assert!(merged.contains("event: message_stop"));
}

#[tokio::test]
async fn test_streaming_hosted_web_search_enforces_max_uses() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_search_limit\",\"model\":\"gpt-5.6\"}}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"ws_allowed\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"ws_allowed\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"allowed query\",\"sources\":[{\"type\":\"url\",\"url\":\"https://example.com/allowed\",\"title\":\"Allowed\"}]}}}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":1,\"item\":{\"id\":\"ws_over_limit\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"must not leak\"}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_search_limit\",\"status\":\"completed\",\"usage\":{\"input_tokens\":4,\"output_tokens\":3}}}\n\n"
    );

    let merged = convert_stream_text_with_web_search_limit(input, "web_search_future", 1).await;
    let events = sse_data_values(&merged);
    let result_blocks: Vec<&Value> = events
        .iter()
        .filter_map(|event| event.get("content_block"))
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("web_search_tool_result"))
        .collect();

    assert_eq!(merged.matches("\"type\":\"server_tool_use\"").count(), 2);
    assert_eq!(result_blocks.len(), 2);
    assert_eq!(result_blocks[0]["tool_use_id"], "ws_allowed");
    assert_eq!(result_blocks[0]["content"][0]["title"], "Allowed");
    assert_eq!(result_blocks[1]["tool_use_id"], "ws_over_limit");
    assert_eq!(
        result_blocks[1]["content"],
        json!({
            "type": "web_search_tool_result_error",
            "error_code": "max_uses_exceeded"
        })
    );
    assert!(!merged.contains("must not leak"));
    assert!(merged.contains("\"web_search_requests\":1"));
    assert!(merged.contains("event: message_stop"));
}

#[tokio::test]
async fn test_post_search_activity_preserves_mixed_block_order_and_emits_ping() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_mixed_order\",\"model\":\"gpt-5.6\"}}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"ws_order\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"ws_order\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"Rust docs\",\"sources\":[{\"type\":\"url\",\"url\":\"https://doc.rust-lang.org/\",\"title\":\"Rust Documentation\"}]}}}\n\n",
        "event: response.content_part.added\n",
        "data: {\"type\":\"response.content_part.added\",\"output_index\":1,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"\"}}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"output_index\":1,\"content_index\":0,\"delta\":\"Search answer before function.\"}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":2,\"item\":{\"id\":\"fc_after_search\",\"type\":\"function_call\",\"call_id\":\"call_after_search\",\"name\":\"lookup\",\"arguments\":\"\"}}\n\n",
        "event: response.function_call_arguments.done\n",
        "data: {\"type\":\"response.function_call_arguments.done\",\"item_id\":\"fc_after_search\",\"output_index\":2,\"arguments\":\"{\\\"query\\\":\\\"rust\\\"}\"}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_mixed_order\",\"status\":\"completed\",\"usage\":{\"input_tokens\":8,\"output_tokens\":5}}}\n\n"
    );

    let merged = convert_stream_text_with_web_search_name(input, "web_search_future").await;
    let events = sse_data_values(&merged);
    let starts: Vec<(&str, u64)> = events
        .iter()
        .filter(|event| event.get("type").and_then(Value::as_str) == Some("content_block_start"))
        .filter_map(|event| {
            Some((
                event.pointer("/content_block/type")?.as_str()?,
                event.get("index")?.as_u64()?,
            ))
        })
        .collect();

    assert_eq!(
        starts,
        vec![
            ("server_tool_use", 0),
            ("web_search_tool_result", 1),
            ("text", 2),
            ("tool_use", 3),
        ]
    );
    assert!(events
        .iter()
        .any(|event| event.get("type").and_then(Value::as_str) == Some("ping")));
    assert!(events.iter().any(|event| {
        event.get("type").and_then(Value::as_str) == Some("message_delta")
            && event.pointer("/delta/stop_reason").and_then(Value::as_str) == Some("tool_use")
    }));
}

#[tokio::test]
async fn test_streaming_web_search_done_with_late_id_reuses_output_index_identity() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_late_id\",\"model\":\"gpt-5.6\"}}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"ws_late_id\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"Rust docs\",\"sources\":[{\"type\":\"url\",\"url\":\"https://doc.rust-lang.org/\",\"title\":\"Rust Documentation\"}]}}}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_late_id\",\"status\":\"completed\",\"usage\":{\"input_tokens\":4,\"output_tokens\":1}}}\n\n"
    );

    let merged = convert_stream_text_with_web_search_limit(input, "web_search_future", 1).await;
    let events = sse_data_values(&merged);
    let result_blocks: Vec<&Value> = events
        .iter()
        .filter_map(|event| event.get("content_block"))
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("web_search_tool_result"))
        .collect();

    assert_eq!(merged.matches("\"type\":\"server_tool_use\"").count(), 1);
    assert_eq!(result_blocks.len(), 1);
    assert_eq!(result_blocks[0]["tool_use_id"], "ws_stream_0");
    assert_eq!(
        result_blocks[0]["content"][0]["url"],
        "https://doc.rust-lang.org/"
    );
    assert!(!merged.contains("max_uses_exceeded"));
    assert!(merged.contains("\"web_search_requests\":1"));
    assert!(merged.contains("event: message_stop"));
}

#[tokio::test]
async fn test_streaming_web_search_limit_closes_parallel_in_flight_blocks() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_parallel_limit\",\"model\":\"gpt-5.6\"}}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"ws_in_flight\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":1,\"item\":{\"id\":\"ws_over_limit\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n"
    );

    let merged = convert_stream_text_with_web_search_limit(input, "web_search_future", 1).await;
    let events = sse_data_values(&merged);
    let mut started_indices: Vec<u64> = events
        .iter()
        .filter(|event| event.get("type").and_then(Value::as_str) == Some("content_block_start"))
        .filter_map(|event| event.get("index").and_then(Value::as_u64))
        .collect();
    let mut stopped_indices: Vec<u64> = events
        .iter()
        .filter(|event| event.get("type").and_then(Value::as_str) == Some("content_block_stop"))
        .filter_map(|event| event.get("index").and_then(Value::as_u64))
        .collect();
    started_indices.sort_unstable();
    stopped_indices.sort_unstable();

    assert_eq!(started_indices, stopped_indices);
    assert_eq!(merged.matches("\"type\":\"server_tool_use\"").count(), 2);
    assert!(merged.contains("\"error_code\":\"unavailable\""));
    assert!(merged.contains("\"error_code\":\"max_uses_exceeded\""));
    assert!(merged.contains("\"web_search_requests\":1"));
    assert!(merged.contains("event: message_stop"));
}

#[tokio::test]
async fn test_streaming_web_search_limit_does_not_finalize_parallel_function() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_parallel_function_limit\",\"model\":\"gpt-5.6\"}}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"fc_open\",\"type\":\"function_call\",\"call_id\":\"call_open\",\"name\":\"lookup\",\"arguments\":\"\"}}\n\n",
        "event: response.function_call_arguments.delta\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_open\",\"output_index\":0,\"delta\":\"{\\\"query\\\":\"}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":1,\"item\":{\"id\":\"ws_allowed\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":1,\"item\":{\"id\":\"ws_allowed\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"allowed\"}}}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":2,\"item\":{\"id\":\"ws_over_limit\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n"
    );

    let merged = convert_stream_text_with_web_search_limit(input, "web_search_future", 1).await;
    let events = sse_data_values(&merged);
    let function_index = events
        .iter()
        .find(|event| {
            event.pointer("/content_block/type").and_then(Value::as_str) == Some("tool_use")
        })
        .and_then(|event| event.get("index"))
        .and_then(Value::as_u64)
        .unwrap();

    assert!(!events.iter().any(|event| {
        event.get("type").and_then(Value::as_str) == Some("content_block_stop")
            && event.get("index").and_then(Value::as_u64) == Some(function_index)
    }));
    assert!(events.iter().any(|event| {
        event.get("type").and_then(Value::as_str) == Some("error")
            && event.pointer("/error/type").and_then(Value::as_str) == Some("stream_truncated")
    }));
    assert!(!merged.contains("event: message_stop"));
}

#[tokio::test]
async fn test_streaming_web_search_limit_preserves_completed_function_stop_reason() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_completed_function_limit\",\"model\":\"gpt-5.6\"}}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"fc_done\",\"type\":\"function_call\",\"call_id\":\"call_done\",\"name\":\"lookup\",\"arguments\":\"\"}}\n\n",
        "event: response.function_call_arguments.done\n",
        "data: {\"type\":\"response.function_call_arguments.done\",\"item_id\":\"fc_done\",\"output_index\":0,\"arguments\":\"{\\\"query\\\":\\\"rust\\\"}\"}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":1,\"item\":{\"id\":\"ws_allowed\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":1,\"item\":{\"id\":\"ws_allowed\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"allowed\"}}}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":2,\"item\":{\"id\":\"ws_over_limit\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n"
    );

    let merged = convert_stream_text_with_web_search_limit(input, "web_search_future", 1).await;
    let events = sse_data_values(&merged);

    assert!(events.iter().any(|event| {
        event.get("type").and_then(Value::as_str) == Some("message_delta")
            && event.pointer("/delta/stop_reason").and_then(Value::as_str) == Some("tool_use")
    }));
    assert!(!events.iter().any(|event| {
        event.get("type").and_then(Value::as_str) == Some("message_delta")
            && event.pointer("/delta/stop_reason").and_then(Value::as_str) == Some("end_turn")
    }));
    assert!(merged.contains("event: message_stop"));
    assert!(!merged.contains("stream_truncated"));
}

#[tokio::test]
async fn test_streaming_done_only_search_limit_does_not_finalize_parallel_reasoning() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_parallel_reasoning_limit\",\"model\":\"gpt-5.6\"}}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"rs_open\",\"type\":\"reasoning\",\"summary\":[]}}\n\n",
        "event: response.reasoning_summary_part.added\n",
        "data: {\"type\":\"response.reasoning_summary_part.added\",\"item_id\":\"rs_open\",\"output_index\":0,\"summary_index\":0,\"part\":{\"type\":\"summary_text\",\"text\":\"\"}}\n\n",
        "event: response.reasoning_summary_text.delta\n",
        "data: {\"type\":\"response.reasoning_summary_text.delta\",\"item_id\":\"rs_open\",\"output_index\":0,\"summary_index\":0,\"delta\":\"Still reasoning\"}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":1,\"item\":{\"id\":\"ws_allowed\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"allowed\"}}}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":2,\"item\":{\"id\":\"ws_over_limit\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"over limit\"}}}\n\n"
    );

    let merged = convert_stream_text_with_web_search_limit(input, "web_search_future", 1).await;
    let events = sse_data_values(&merged);
    let reasoning_index = events
        .iter()
        .find(|event| {
            event.pointer("/content_block/type").and_then(Value::as_str) == Some("thinking")
        })
        .and_then(|event| event.get("index"))
        .and_then(Value::as_u64)
        .unwrap();

    assert!(!events.iter().any(|event| {
        event.get("type").and_then(Value::as_str) == Some("content_block_stop")
            && event.get("index").and_then(Value::as_u64) == Some(reasoning_index)
    }));
    assert!(events.iter().any(|event| {
        event.get("type").and_then(Value::as_str) == Some("error")
            && event.pointer("/error/type").and_then(Value::as_str) == Some("stream_truncated")
    }));
    assert!(!merged.contains("event: message_stop"));
}

#[tokio::test]
async fn test_failed_search_sources_do_not_consume_successful_search_citations() {
    let input = concat!(
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_shared_source\",\"model\":\"gpt-5.6\",\"status\":\"completed\",\"output\":[{\"id\":\"ws_failed\",\"type\":\"web_search_call\",\"status\":\"failed\",\"action\":{\"type\":\"search\",\"query\":\"failed query\",\"sources\":[{\"type\":\"url\",\"url\":\"https://example.com/shared\",\"title\":\"Failed source\"}]},\"error\":{\"code\":\"invalid_search_query\"}},{\"id\":\"ws_success\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"successful query\"}},{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Successful answer.\",\"annotations\":[{\"type\":\"url_citation\",\"url\":\"https://example.com/shared\",\"title\":\"Successful citation\"}]}]}],\"usage\":{\"input_tokens\":8,\"output_tokens\":5}}}\n\n"
    );

    let merged = convert_stream_text_with_web_search_name(input, "web_search_future").await;
    let events = sse_data_values(&merged);
    let result_blocks: Vec<&Value> = events
        .iter()
        .filter_map(|event| event.get("content_block"))
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("web_search_tool_result"))
        .collect();

    assert_eq!(result_blocks.len(), 2);
    assert_eq!(result_blocks[0]["tool_use_id"], "ws_failed");
    assert_eq!(
        result_blocks[0]["content"]["type"],
        "web_search_tool_result_error"
    );
    assert_eq!(result_blocks[1]["tool_use_id"], "ws_success");
    assert_eq!(
        result_blocks[1]["content"][0]["url"],
        "https://example.com/shared"
    );
    assert!(merged.contains("\"web_search_requests\":2"));
    assert!(merged.contains("event: message_stop"));
}

#[tokio::test]
async fn test_terminal_web_search_reuses_stream_synthesized_id() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_no_search_id\",\"model\":\"gpt-5.6\"}}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"Rust docs\"}}}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_no_search_id\",\"status\":\"completed\",\"output\":[{\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"Rust docs\"}}],\"usage\":{\"input_tokens\":4,\"output_tokens\":1}}}\n\n"
    );

    let merged = convert_stream_text(input).await;
    let events = sse_data_values(&merged);
    let result_blocks: Vec<&Value> = events
        .iter()
        .filter_map(|event| event.get("content_block"))
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("web_search_tool_result"))
        .collect();

    assert_eq!(merged.matches("\"type\":\"server_tool_use\"").count(), 1);
    assert_eq!(result_blocks.len(), 1);
    assert_eq!(result_blocks[0]["tool_use_id"], "ws_stream_0");
    assert!(merged.contains("\"web_search_requests\":1"));
}

#[tokio::test]
async fn test_clean_eof_after_completed_web_search_is_truncated() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_truncated_search\",\"model\":\"gpt-5.6\"}}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"ws_truncated\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"ws_truncated\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"Rust docs\"}}}\n\n"
    );

    let merged = convert_stream_text(input).await;
    assert!(merged.contains("event: error"));
    assert!(merged.contains("stream_truncated"));
    assert!(!merged.contains("event: message_stop"));
}

#[tokio::test]
async fn test_streaming_hosted_web_search_pairs_every_call_with_its_sources() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_multi_search\",\"model\":\"gpt-5.6\"}}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"ws_rust\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"ws_rust\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"Rust language\",\"sources\":[{\"type\":\"url\",\"url\":\"https://www.rust-lang.org/\",\"title\":\"Rust\"}]}}}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":1,\"item\":{\"id\":\"ws_cargo\",\"type\":\"web_search_call\",\"status\":\"in_progress\"}}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":1,\"item\":{\"id\":\"ws_cargo\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"Cargo documentation\",\"sources\":[{\"type\":\"url\",\"url\":\"https://doc.rust-lang.org/cargo/\",\"title\":\"Cargo\"}]}}}\n\n",
        "event: response.content_part.added\n",
        "data: {\"type\":\"response.content_part.added\",\"output_index\":2,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"\"}}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"output_index\":2,\"content_index\":0,\"delta\":\"Rust and Cargo have official documentation.\"}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":2,\"item\":{\"id\":\"msg_multi\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Rust and Cargo have official documentation.\",\"annotations\":[{\"type\":\"url_citation\",\"url\":\"https://www.rust-lang.org/\",\"title\":\"Rust\"},{\"type\":\"url_citation\",\"url\":\"https://doc.rust-lang.org/cargo/\",\"title\":\"Cargo\"}]}]}}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_multi_search\",\"status\":\"completed\",\"usage\":{\"input_tokens\":12,\"output_tokens\":14}}}\n\n"
    );

    let merged = convert_stream_text_with_web_search_name(input, "web_search_next").await;
    let events = sse_data_values(&merged);
    let result_blocks: Vec<&Value> = events
        .iter()
        .filter_map(|event| event.get("content_block"))
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("web_search_tool_result"))
        .collect();

    assert_eq!(merged.matches("\"type\":\"server_tool_use\"").count(), 2);
    assert_eq!(result_blocks.len(), 2);
    assert_eq!(result_blocks[0]["tool_use_id"], "ws_rust");
    assert_eq!(
        result_blocks[0]["content"][0]["url"],
        "https://www.rust-lang.org/"
    );
    assert_eq!(result_blocks[1]["tool_use_id"], "ws_cargo");
    assert_eq!(
        result_blocks[1]["content"][0]["url"],
        "https://doc.rust-lang.org/cargo/"
    );
    assert!(merged.contains("\"web_search_requests\":2"));
    assert!(merged.contains("event: message_stop"));
}

#[tokio::test]
async fn test_streaming_hosted_web_search_pairs_calls_without_sources() {
    let input = concat!(
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_multi_fallback\",\"model\":\"gpt-5.6\",\"status\":\"completed\",\"output\":[{\"id\":\"ws_first\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"first query\"}},{\"id\":\"ws_second\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"second query\"}},{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Combined answer.\",\"annotations\":[{\"type\":\"url_citation\",\"start_index\":9,\"end_index\":15,\"url\":\"https://example.com/result\",\"title\":\"Combined result\"}]}]}],\"usage\":{\"input_tokens\":8,\"output_tokens\":5}}}\n\n"
    );

    let merged = convert_stream_text_with_web_search_name(input, "web_search_next").await;
    let events = sse_data_values(&merged);
    let result_blocks: Vec<&Value> = events
        .iter()
        .filter_map(|event| event.get("content_block"))
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("web_search_tool_result"))
        .collect();

    assert_eq!(merged.matches("\"type\":\"server_tool_use\"").count(), 2);
    assert_eq!(result_blocks.len(), 2);
    assert_eq!(result_blocks[0]["tool_use_id"], "ws_first");
    assert_eq!(result_blocks[0]["content"], json!([]));
    assert_eq!(result_blocks[1]["tool_use_id"], "ws_second");
    assert_eq!(
        result_blocks[1]["content"][0]["url"],
        "https://example.com/result"
    );
    let text_deltas: Vec<&str> = events
        .iter()
        .filter(|event| event.pointer("/delta/type").and_then(Value::as_str) == Some("text_delta"))
        .filter_map(|event| event.pointer("/delta/text").and_then(Value::as_str))
        .collect();
    assert_eq!(
        text_deltas,
        vec!["Combined [answer](https://example.com/result)."]
    );
    assert!(merged.contains("\"web_search_requests\":2"));
    assert!(merged.contains("event: message_stop"));
}

#[tokio::test]
async fn test_terminal_only_web_search_preserves_output_order() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_order\",\"model\":\"gpt-5.6\"}}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_order\",\"model\":\"gpt-5.6\",\"status\":\"completed\",\"output\":[{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Before\",\"annotations\":[]}]},{\"id\":\"ws_order\",\"type\":\"web_search_call\",\"status\":\"completed\",\"action\":{\"type\":\"search\",\"query\":\"Rust\"}},{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"After\",\"annotations\":[]}]}],\"usage\":{\"input_tokens\":8,\"output_tokens\":4}}}\n\n"
    );

    let completed_only = &input[input.find("event: response.completed").unwrap()..];
    let incomplete = input
        .replace("response.completed", "response.incomplete")
        .replace("\"status\":\"completed\",\"output\"", "\"output\"");
    for (stream, stop_reason) in [
        (input, "end_turn"),
        (completed_only, "end_turn"),
        (incomplete.as_str(), "max_tokens"),
    ] {
        let merged =
            convert_stream_text_with_web_search_name(stream.to_owned(), "web_search").await;
        let events = sse_data_values(&merged);
        let block_types: Vec<&str> = events
            .iter()
            .filter(|event| {
                event.get("type").and_then(Value::as_str) == Some("content_block_start")
            })
            .filter_map(|event| event.pointer("/content_block/type").and_then(Value::as_str))
            .collect();
        let text_deltas: Vec<&str> = events
            .iter()
            .filter(|event| {
                event.pointer("/delta/type").and_then(Value::as_str) == Some("text_delta")
            })
            .filter_map(|event| event.pointer("/delta/text").and_then(Value::as_str))
            .collect();

        assert_eq!(
            block_types,
            vec!["text", "server_tool_use", "web_search_tool_result", "text"]
        );
        assert_eq!(text_deltas, vec!["Before", "After"]);
        assert_eq!(merged.matches("event: message_start").count(), 1);
        assert!(merged.contains(&format!("\"stop_reason\":\"{stop_reason}\"")));
        assert!(merged.contains("event: message_stop"));
    }
}

#[tokio::test]
async fn test_terminal_only_completed_without_status_keeps_stop_reason() {
    for (output, stop_reason) in [
        (
            json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"Done"}]}),
            "end_turn",
        ),
        (
            json!({"type":"function_call","call_id":"call_1","name":"run","arguments":"{}"}),
            "tool_use",
        ),
    ] {
        let input = format!(
            "event: response.completed\ndata: {}\n\n",
            json!({"type":"response.completed","response":{"id":"resp_statusless","model":"gpt-5.6","output":[output]}})
        );
        let merged = convert_stream_text(input).await;
        assert!(merged.contains(&format!("\"stop_reason\":\"{stop_reason}\"")));
        if stop_reason == "tool_use" {
            assert!(merged.contains("\"type\":\"tool_use\""));
        }
        assert!(merged.contains("event: message_stop"));
    }
}

#[tokio::test]
async fn test_terminal_output_does_not_duplicate_done_reasoning() {
    let input = concat!(
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"rs_done\",\"type\":\"reasoning\",\"summary\":[{\"type\":\"summary_text\",\"text\":\"Need a tool.\"}]}}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_reasoning\",\"model\":\"gpt-5.6\",\"status\":\"completed\",\"output\":[{\"id\":\"rs_done\",\"type\":\"reasoning\",\"summary\":[{\"type\":\"summary_text\",\"text\":\"Need a tool.\"}]}]}}\n\n"
    );

    let merged = convert_stream_text(input).await;

    assert_eq!(merged.matches("\"type\":\"thinking_delta\"").count(), 1);
    assert!(merged.contains("event: message_stop"));
}

#[tokio::test]
async fn test_terminal_output_does_not_duplicate_streamed_text() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_text\",\"model\":\"gpt-5.6\"}}\n\n",
        "event: response.content_part.added\n",
        "data: {\"type\":\"response.content_part.added\",\"item_id\":\"msg_text\",\"output_index\":0,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"\"}}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_text\",\"output_index\":0,\"content_index\":0,\"delta\":\"Already streamed.\"}\n\n",
        "event: response.output_text.done\n",
        "data: {\"type\":\"response.output_text.done\",\"item_id\":\"msg_text\",\"output_index\":0,\"content_index\":0,\"text\":\"Already streamed.\"}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"msg_text\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Already streamed.\",\"annotations\":[]}]}}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_text\",\"model\":\"gpt-5.6\",\"status\":\"completed\",\"output\":[{\"id\":\"msg_text\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Already streamed.\",\"annotations\":[]}]}]}}\n\n"
    );

    let merged = convert_stream_text(input).await;
    let text_deltas: Vec<String> = sse_data_values(&merged)
        .into_iter()
        .filter(|event| event.pointer("/delta/type").and_then(Value::as_str) == Some("text_delta"))
        .filter_map(|event| {
            event
                .pointer("/delta/text")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
        .collect();

    assert_eq!(text_deltas, vec!["Already streamed."]);
    assert!(merged.contains("event: message_stop"));
}

#[tokio::test]
async fn test_output_item_done_emits_text_when_deltas_are_missing() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_done_text\",\"model\":\"gpt-5.6\"}}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"msg_done_text\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Recovered from the completed item.\",\"annotations\":[]}]}}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_done_text\",\"model\":\"gpt-5.6\",\"status\":\"completed\"}}\n\n"
    );

    let merged = convert_stream_text(input).await;
    let text_deltas: Vec<String> = sse_data_values(&merged)
        .into_iter()
        .filter(|event| event.pointer("/delta/type").and_then(Value::as_str) == Some("text_delta"))
        .filter_map(|event| {
            event
                .pointer("/delta/text")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
        .collect();

    assert_eq!(text_deltas, vec!["Recovered from the completed item."]);
    assert!(merged.contains("event: message_stop"));
}

#[tokio::test]
async fn test_final_event_without_blank_line_is_processed() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-5\"}}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n"
    );

    let merged = convert_stream_text(input).await;
    assert!(merged.contains("\"stop_reason\":\"end_turn\""));
    assert_eq!(merged.matches("event: message_stop").count(), 1);
    assert!(!merged.contains("stream_truncated"));
}

#[tokio::test]
async fn test_clean_eof_after_partial_text_is_explicitly_incomplete() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-5\"}}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n"
    );

    let merged = convert_stream_text(input).await;
    assert!(merged.contains("\"stop_reason\":\"max_tokens\""));
    assert!(merged.contains("event: content_block_stop"));
    assert!(merged.contains("event: message_stop"));
}

#[tokio::test]
async fn test_hosted_web_search_option_does_not_buffer_without_search() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-5.6\"}}\n\n",
        "event: response.content_part.added\n",
        "data: {\"type\":\"response.content_part.added\",\"output_index\":0,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"\"}}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"content_index\":0,\"delta\":\"partial\"}\n\n"
    );

    let merged = convert_stream_text_with_web_search_name(input, "web_search").await;
    let text_deltas: Vec<String> = sse_data_values(&merged)
        .into_iter()
        .filter(|event| event.pointer("/delta/type").and_then(Value::as_str) == Some("text_delta"))
        .filter_map(|event| {
            event
                .pointer("/delta/text")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
        .collect();

    assert_eq!(text_deltas, vec!["partial"]);
    assert!(!merged.contains("event: ping"));
    assert!(merged.contains("\"stop_reason\":\"max_tokens\""));
    assert!(merged.contains("event: message_stop"));
}

#[tokio::test]
async fn test_clean_eof_during_tool_arguments_is_error() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-5\"}}\n\n",
        "event: response.function_call_arguments.delta\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_1\",\"call_id\":\"call_1\",\"name\":\"exec\",\"delta\":\"{\\\"cmd\\\":\"}\n\n"
    );

    let merged = convert_stream_text(input).await;
    assert!(merged.contains("event: error"));
    assert!(merged.contains("stream_truncated"));
    assert!(!merged.contains("event: message_stop"));
}

#[tokio::test]
async fn test_stream_request_with_complete_json_response_is_converted() {
    let input = r#"{
            "id":"resp_json",
            "status":"completed",
            "model":"gpt-5",
            "output":[{"type":"message","content":[{"type":"output_text","text":"hello"}]}],
            "usage":{"input_tokens":4,"output_tokens":1}
        }"#;

    let merged = convert_stream_text(input).await;
    assert!(merged.contains("event: message_start"));
    assert!(merged.contains("\"text\":\"hello\""));
    assert!(merged.contains("event: message_stop"));
}

#[tokio::test]
async fn test_stream_request_with_failed_json_response_is_error() {
    let input = r#"{
            "id":"resp_json",
            "status":"failed",
            "error":{"type":"server_error","message":"json backend failed"},
            "output":[]
        }"#;

    let merged = convert_stream_text(input).await;
    assert!(merged.contains("event: error"));
    assert!(merged.contains("json backend failed"));
    assert!(!merged.contains("event: message_stop"));
}

#[tokio::test]
async fn test_streaming_conversion_with_wrapped_response_events() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-4o\",\"usage\":{\"input_tokens\":12,\"output_tokens\":0}}}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"type\":\"function_call\",\"call_id\":\"call_1\",\"name\":\"get_weather\"}}\n\n",
        "event: response.function_call_arguments.delta\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"delta\":\"{\\\"city\\\":\\\"Tokyo\\\"}\"}\n\n",
        "event: response.function_call_arguments.done\n",
        "data: {\"type\":\"response.function_call_arguments.done\"}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":12,\"output_tokens\":3}}}\n\n"
    );

    let upstream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(
        input.as_bytes().to_vec(),
    ))]);
    let converted = create_anthropic_sse_stream_from_responses(upstream);
    let chunks: Vec<_> = converted.collect().await;

    let merged = chunks
        .into_iter()
        .map(|c| String::from_utf8_lossy(c.unwrap().as_ref()).to_string())
        .collect::<String>();

    assert!(merged.contains("\"type\":\"message_start\""));
    assert!(merged.contains("\"id\":\"resp_1\""));
    assert!(merged.contains("\"model\":\"gpt-4o\""));
    assert!(merged.contains("\"type\":\"tool_use\""));
    assert!(merged.contains("\"name\":\"get_weather\""));
    assert!(merged.contains("\"type\":\"input_json_delta\""));
    assert!(merged.contains("\"stop_reason\":\"tool_use\""));
    assert!(merged.contains("\"input_tokens\":12"));
    assert!(merged.contains("\"output_tokens\":3"));
    assert!(merged.contains("\"type\":\"message_stop\""));
}

#[tokio::test]
async fn test_streaming_read_tool_drops_empty_pages() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_read\",\"model\":\"gpt-5.5\"}}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"fc_read\",\"type\":\"function_call\",\"call_id\":\"call_read\",\"name\":\"Read\"}}\n\n",
        "event: response.function_call_arguments.delta\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_read\",\"delta\":\"{\\\"file_path\\\":\\\"/tmp/demo.py\\\",\\\"limit\\\":2000,\\\"offset\\\":0,\\\"pages\\\":\\\"\\\"}\"}\n\n",
        "event: response.function_call_arguments.done\n",
        "data: {\"type\":\"response.function_call_arguments.done\",\"item_id\":\"fc_read\",\"arguments\":\"{\\\"file_path\\\":\\\"/tmp/demo.py\\\",\\\"limit\\\":2000,\\\"offset\\\":0,\\\"pages\\\":\\\"\\\"}\"}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n"
    );

    let upstream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(
        input.as_bytes().to_vec(),
    ))]);
    let converted = create_anthropic_sse_stream_from_responses(upstream);
    let chunks: Vec<_> = converted.collect().await;

    let merged = chunks
        .into_iter()
        .map(|c| String::from_utf8_lossy(c.unwrap().as_ref()).to_string())
        .collect::<String>();

    assert!(merged.contains("\"name\":\"Read\""));
    assert!(merged.contains("\"partial_json\":\"{\\\"file_path\\\":\\\"/tmp/demo.py\\\",\\\"limit\\\":2000,\\\"offset\\\":0}"));
    assert!(!merged.contains("\\\"pages\\\":\\\"\\\""));
}

#[tokio::test]
async fn test_streaming_read_tool_duplicate_start_preserves_buffered_args() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_read\",\"model\":\"gpt-5.5\"}}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"fc_read\",\"type\":\"function_call\",\"call_id\":\"call_read\",\"name\":\"Read\"}}\n\n",
        "event: response.function_call_arguments.delta\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_read\",\"delta\":\"{\\\"file_path\\\":\\\"/tmp/demo.py\\\",\\\"limit\\\":2000,\\\"offset\\\":0,\\\"pages\\\":\\\"\\\"}\"}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"fc_read\",\"type\":\"function_call\",\"call_id\":\"call_read\",\"name\":\"Read\"}}\n\n",
        "event: response.function_call_arguments.done\n",
        "data: {\"type\":\"response.function_call_arguments.done\",\"item_id\":\"fc_read\"}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n"
    );

    let upstream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(
        input.as_bytes().to_vec(),
    ))]);
    let converted = create_anthropic_sse_stream_from_responses(upstream);
    let chunks: Vec<_> = converted.collect().await;

    let merged = chunks
        .into_iter()
        .map(|c| String::from_utf8_lossy(c.unwrap().as_ref()).to_string())
        .collect::<String>();

    assert_eq!(merged.matches("event: content_block_start").count(), 1);
    assert_eq!(merged.matches("event: content_block_stop").count(), 1);
    assert!(merged.contains("\"partial_json\":\"{\\\"file_path\\\":\\\"/tmp/demo.py\\\",\\\"limit\\\":2000,\\\"offset\\\":0}"));
    assert!(!merged.contains("\\\"pages\\\":\\\"\\\""));
}

#[tokio::test]
async fn test_streaming_conversion_interleaved_tool_deltas_by_item_id() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_2\",\"model\":\"gpt-4o\"}}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"fc_1\",\"type\":\"function_call\",\"call_id\":\"call_1\",\"name\":\"first_tool\"}}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"fc_2\",\"type\":\"function_call\",\"call_id\":\"call_2\",\"name\":\"second_tool\"}}\n\n",
        "event: response.function_call_arguments.delta\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_2\",\"delta\":\"{\\\"b\\\":2}\"}\n\n",
        "event: response.function_call_arguments.delta\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_1\",\"delta\":\"{\\\"a\\\":1}\"}\n\n",
        "event: response.function_call_arguments.done\n",
        "data: {\"type\":\"response.function_call_arguments.done\",\"item_id\":\"fc_1\"}\n\n",
        "event: response.function_call_arguments.done\n",
        "data: {\"type\":\"response.function_call_arguments.done\",\"item_id\":\"fc_2\"}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":8,\"output_tokens\":4}}}\n\n"
    );

    let upstream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(
        input.as_bytes().to_vec(),
    ))]);
    let converted = create_anthropic_sse_stream_from_responses(upstream);
    let chunks: Vec<_> = converted.collect().await;
    let merged = chunks
        .into_iter()
        .map(|c| String::from_utf8_lossy(c.unwrap().as_ref()).to_string())
        .collect::<String>();

    let events: Vec<Value> = merged
        .split("\n\n")
        .filter_map(|block| {
            let data = block
                .lines()
                .find_map(|line| strip_sse_field(line, "data"))?;
            serde_json::from_str::<Value>(data).ok()
        })
        .collect();

    let mut tool_index_by_call: HashMap<String, u64> = HashMap::new();
    for event in &events {
        if event.get("type").and_then(|v| v.as_str()) == Some("content_block_start") {
            let cb = event.get("content_block");
            if cb.and_then(|v| v.get("type")).and_then(|v| v.as_str()) == Some("tool_use") {
                if let (Some(call_id), Some(index)) = (
                    cb.and_then(|v| v.get("id")).and_then(|v| v.as_str()),
                    event.get("index").and_then(|v| v.as_u64()),
                ) {
                    tool_index_by_call.insert(call_id.to_string(), index);
                }
            }
        }
    }

    let delta_indices: Vec<u64> = events
        .iter()
        .filter(|event| {
            event.get("type").and_then(|v| v.as_str()) == Some("content_block_delta")
                && event.pointer("/delta/type").and_then(|v| v.as_str()) == Some("input_json_delta")
        })
        .filter_map(|event| event.get("index").and_then(|v| v.as_u64()))
        .collect();

    assert_eq!(delta_indices.len(), 2);
    assert_eq!(delta_indices[0], *tool_index_by_call.get("call_2").unwrap());
    assert_eq!(delta_indices[1], *tool_index_by_call.get("call_1").unwrap());
    assert_ne!(
        tool_index_by_call.get("call_1"),
        tool_index_by_call.get("call_2")
    );
}

#[tokio::test]
async fn test_streaming_tool_done_arguments_fallback_without_deltas() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_done\",\"model\":\"gpt-5.6\"}}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"fc_done\",\"type\":\"function_call\",\"call_id\":\"call_done\",\"name\":\"lookup\",\"arguments\":\"\"}}\n\n",
        "event: response.function_call_arguments.done\n",
        "data: {\"type\":\"response.function_call_arguments.done\",\"item_id\":\"fc_done\",\"output_index\":0,\"item\":{\"id\":\"fc_done\",\"type\":\"function_call\",\"arguments\":\"{\\\"q\\\":\\\"rust\\\"}\"}}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n"
    );
    let upstream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(input))]);
    let merged = create_anthropic_sse_stream_from_responses(upstream)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .map(|chunk| String::from_utf8_lossy(chunk.unwrap().as_ref()).to_string())
        .collect::<String>();

    assert!(merged.contains("\"partial_json\":\"{\\\"q\\\":\\\"rust\\\"}\""));
    assert_eq!(merged.matches("event: content_block_stop").count(), 1);
}

#[tokio::test]
async fn test_official_reasoning_events_emit_signature_before_stop() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_reason\",\"model\":\"gpt-5.6\"}}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"rs_1\",\"type\":\"reasoning\",\"summary\":[]}}\n\n",
        "event: response.reasoning_summary_part.added\n",
        "data: {\"type\":\"response.reasoning_summary_part.added\",\"item_id\":\"rs_1\",\"output_index\":0,\"summary_index\":0,\"part\":{\"type\":\"summary_text\",\"text\":\"\"}}\n\n",
        "event: response.reasoning_summary_text.delta\n",
        "data: {\"type\":\"response.reasoning_summary_text.delta\",\"item_id\":\"rs_1\",\"output_index\":0,\"summary_index\":0,\"delta\":\"Need a tool.\"}\n\n",
        "event: response.reasoning_summary_text.done\n",
        "data: {\"type\":\"response.reasoning_summary_text.done\",\"item_id\":\"rs_1\",\"output_index\":0,\"summary_index\":0,\"text\":\"Need a tool.\"}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"rs_1\",\"type\":\"reasoning\",\"summary\":[{\"type\":\"summary_text\",\"text\":\"Need a tool.\"}],\"encrypted_content\":\"opaque\"}}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n"
    );
    let upstream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(input))]);
    let merged = create_anthropic_sse_stream_from_responses(upstream)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .map(|chunk| String::from_utf8_lossy(chunk.unwrap().as_ref()).to_string())
        .collect::<String>();

    assert!(merged.contains("\"type\":\"thinking_delta\""));
    assert!(merged.contains("\"type\":\"signature_delta\""));
    let signature_position = merged.find("signature_delta").unwrap();
    let stop_position = merged.find("event: content_block_stop").unwrap();
    assert!(signature_position < stop_position);
    assert!(!merged[stop_position..].contains("content_block_delta"));
}

#[tokio::test]
async fn test_encrypted_only_reasoning_streams_as_empty_thinking_with_signature() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_enc\",\"model\":\"gpt-5.6\"}}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"rs_1\",\"type\":\"reasoning\",\"summary\":[]}}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"rs_1\",\"type\":\"reasoning\",\"summary\":[],\"encrypted_content\":\"opaque\"}}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n"
    );
    let merged = convert_stream_text(input).await;
    let events = sse_data_values(&merged);

    assert!(!merged.contains("redacted_thinking"));
    let start = events
        .iter()
        .find(|event| event["type"] == "content_block_start")
        .unwrap();
    assert_eq!(
        start["content_block"],
        json!({"type": "thinking", "thinking": ""})
    );
    let signature = events
        .iter()
        .find(|event| event["delta"]["type"] == "signature_delta")
        .unwrap();
    let block = json!({
        "type": "thinking",
        "thinking": "",
        "signature": signature["delta"]["signature"]
    });
    assert_eq!(
        super::super::reasoning_bridge::openai_reasoning_item_from_anthropic_block(&block),
        Some(json!({
            "id": "rs_1",
            "type": "reasoning",
            "summary": [],
            "encrypted_content": "opaque"
        }))
    );
}

#[tokio::test]
async fn test_reasoning_summary_tail_missing_from_deltas_is_recovered_on_item_done() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_tail\",\"model\":\"gpt-5.6\"}}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"rs_1\",\"type\":\"reasoning\",\"summary\":[]}}\n\n",
        "event: response.reasoning_summary_text.delta\n",
        "data: {\"type\":\"response.reasoning_summary_text.delta\",\"item_id\":\"rs_1\",\"output_index\":0,\"summary_index\":0,\"delta\":\"First. \"}\n\n",
        "event: response.reasoning_summary_text.done\n",
        "data: {\"type\":\"response.reasoning_summary_text.done\",\"item_id\":\"rs_1\",\"output_index\":0,\"summary_index\":1,\"text\":\"Second.\"}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"rs_1\",\"type\":\"reasoning\",\"summary\":[{\"type\":\"summary_text\",\"text\":\"First. \"},{\"type\":\"summary_text\",\"text\":\"Second.\"}],\"encrypted_content\":\"opaque\"}}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n"
    );
    let events = sse_data_values(&convert_stream_text(input).await);
    let thinking = events
        .iter()
        .filter(|event| event["delta"]["type"] == "thinking_delta")
        .filter_map(|event| event["delta"]["thinking"].as_str())
        .collect::<String>();
    assert_eq!(thinking, "First. Second.");
    assert_eq!(
        events
            .iter()
            .filter(|event| event["delta"]["type"] == "signature_delta")
            .count(),
        1
    );
}

#[tokio::test]
async fn test_reasoning_item_done_does_not_replay_drifted_summary() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_drift\",\"model\":\"gpt-5.6\"}}\n\n",
        "event: response.reasoning_summary_text.delta\n",
        "data: {\"type\":\"response.reasoning_summary_text.delta\",\"item_id\":\"rs_1\",\"output_index\":0,\"summary_index\":0,\"delta\":\"Hello world\"}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"rs_1\",\"type\":\"reasoning\",\"summary\":[{\"type\":\"summary_text\",\"text\":\"Hello  world\"}],\"encrypted_content\":\"opaque\"}}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n"
    );
    let events = sse_data_values(&convert_stream_text(input).await);
    let thinking = events
        .iter()
        .filter(|event| event["delta"]["type"] == "thinking_delta")
        .filter_map(|event| event["delta"]["thinking"].as_str())
        .collect::<String>();
    assert_eq!(thinking, "Hello world");
}

#[tokio::test]
async fn test_streaming_reasoning_delta_emits_thinking_blocks() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_r\",\"model\":\"o3\",\"usage\":{\"input_tokens\":5,\"output_tokens\":0}}}\n\n",
        "event: response.reasoning.delta\n",
        "data: {\"type\":\"response.reasoning.delta\",\"delta\":\"Let me \"}\n\n",
        "event: response.reasoning.delta\n",
        "data: {\"type\":\"response.reasoning.delta\",\"delta\":\"think...\"}\n\n",
        "event: response.reasoning.done\n",
        "data: {\"type\":\"response.reasoning.done\"}\n\n",
        "event: response.content_part.added\n",
        "data: {\"type\":\"response.content_part.added\",\"part\":{\"type\":\"output_text\",\"text\":\"\"},\"output_index\":0,\"content_index\":0}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"42\",\"output_index\":0,\"content_index\":0}\n\n",
        "event: response.content_part.done\n",
        "data: {\"type\":\"response.content_part.done\",\"output_index\":0,\"content_index\":0}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":5,\"output_tokens\":10}}}\n\n"
    );

    let upstream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(
        input.as_bytes().to_vec(),
    ))]);
    let converted = create_anthropic_sse_stream_from_responses(upstream);
    let chunks: Vec<_> = converted.collect().await;
    let merged = chunks
        .into_iter()
        .map(|c| String::from_utf8_lossy(c.unwrap().as_ref()).to_string())
        .collect::<String>();

    // Should contain thinking block start, thinking delta, and text content
    assert!(
        merged.contains("\"type\":\"thinking\""),
        "should emit thinking content_block_start"
    );
    assert!(
        merged.contains("\"type\":\"thinking_delta\""),
        "should emit thinking_delta"
    );
    assert!(
        merged.contains("\"thinking\":\"Let me \"") && merged.contains("\"thinking\":\"think...\""),
        "should contain both thinking deltas"
    );
    assert!(
        merged.contains("\"type\":\"text_delta\""),
        "should also emit text content"
    );
    assert!(
        merged.contains("\"text\":\"42\""),
        "should contain text delta"
    );
    assert!(merged.contains("\"stop_reason\":\"end_turn\""));

    let events: Vec<Value> = merged
        .split("\n\n")
        .filter_map(|block| {
            block
                .lines()
                .find_map(|line| line.strip_prefix("data: "))
                .and_then(|data| serde_json::from_str(data).ok())
        })
        .collect();
    let thinking_starts: Vec<&Value> = events
        .iter()
        .filter(|event| {
            event.get("type").and_then(Value::as_str) == Some("content_block_start")
                && event.pointer("/content_block/type").and_then(Value::as_str) == Some("thinking")
        })
        .collect();
    assert_eq!(
        thinking_starts.len(),
        1,
        "keyless deltas must share one block"
    );
    let thinking_index = thinking_starts[0]
        .get("index")
        .and_then(Value::as_u64)
        .unwrap();
    let thinking_delta_indices: Vec<u64> = events
        .iter()
        .filter(|event| {
            event.pointer("/delta/type").and_then(Value::as_str) == Some("thinking_delta")
        })
        .filter_map(|event| event.get("index").and_then(Value::as_u64))
        .collect();
    assert_eq!(thinking_delta_indices, vec![thinking_index, thinking_index]);

    let stop_position = events
        .iter()
        .position(|event| {
            event.get("type").and_then(Value::as_str) == Some("content_block_stop")
                && event.get("index").and_then(Value::as_u64) == Some(thinking_index)
        })
        .expect("legacy reasoning done must close the thinking block");
    let text_start_position = events
        .iter()
        .position(|event| {
            event.get("type").and_then(Value::as_str) == Some("content_block_start")
                && event.pointer("/content_block/type").and_then(Value::as_str) == Some("text")
        })
        .expect("text block must start");
    assert!(stop_position < text_start_position);
}

#[tokio::test]
async fn test_streaming_text_parts_are_merged_into_one_text_block() {
    let input = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_merge\",\"model\":\"gpt-5.4\",\"usage\":{\"input_tokens\":5,\"output_tokens\":0}}}\n\n",
        "event: response.content_part.added\n",
        "data: {\"type\":\"response.content_part.added\",\"part\":{\"type\":\"output_text\",\"text\":\"\"},\"output_index\":0,\"content_index\":0}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"你\",\"output_index\":0,\"content_index\":0}\n\n",
        "event: response.content_part.done\n",
        "data: {\"type\":\"response.content_part.done\",\"output_index\":0,\"content_index\":0}\n\n",
        "event: response.content_part.added\n",
        "data: {\"type\":\"response.content_part.added\",\"part\":{\"type\":\"output_text\",\"text\":\"\"},\"output_index\":0,\"content_index\":1}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"好\",\"output_index\":0,\"content_index\":1}\n\n",
        "event: response.content_part.done\n",
        "data: {\"type\":\"response.content_part.done\",\"output_index\":0,\"content_index\":1}\n\n",
        "event: response.output_text.done\n",
        "data: {\"type\":\"response.output_text.done\",\"output_index\":0,\"content_index\":1}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":5,\"output_tokens\":2}}}\n\n"
    );

    let upstream = stream::iter(vec![Ok::<_, std::io::Error>(Bytes::from(
        input.as_bytes().to_vec(),
    ))]);
    let converted = create_anthropic_sse_stream_from_responses(upstream);
    let chunks: Vec<_> = converted.collect().await;
    let events: Vec<Value> = chunks
        .into_iter()
        .flat_map(|chunk| {
            let bytes = chunk.unwrap();
            let text = String::from_utf8_lossy(bytes.as_ref()).to_string();
            text.split("\n\n")
                .filter_map(|block| {
                    block.lines().find_map(|line| {
                        strip_sse_field(line, "data")
                            .and_then(|payload| serde_json::from_str::<Value>(payload).ok())
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect();

    let text_starts = events
        .iter()
        .filter(|event| {
            event.get("type").and_then(|v| v.as_str()) == Some("content_block_start")
                && event
                    .pointer("/content_block/type")
                    .and_then(|v| v.as_str())
                    == Some("text")
        })
        .count();
    let text_stops = events
        .iter()
        .filter(|event| event.get("type").and_then(|v| v.as_str()) == Some("content_block_stop"))
        .count();
    let text_deltas: Vec<String> = events
        .iter()
        .filter(|event| {
            event.get("type").and_then(|v| v.as_str()) == Some("content_block_delta")
                && event.pointer("/delta/type").and_then(|v| v.as_str()) == Some("text_delta")
        })
        .filter_map(|event| {
            event
                .pointer("/delta/text")
                .and_then(|v| v.as_str())
                .map(ToString::to_string)
        })
        .collect();

    assert_eq!(text_starts, 1);
    assert_eq!(text_stops, 1);
    assert_eq!(text_deltas, vec!["你".to_string(), "好".to_string()]);
}

#[tokio::test]
async fn test_streaming_responses_chinese_split_across_chunks_no_replacement_chars() {
    // Chinese text delta split across two TCP chunks.
    let full = concat!(
        "event: response.created\n",
        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_cn\",\"model\":\"gpt-4o\",\"usage\":{\"input_tokens\":5,\"output_tokens\":0}}}\n\n",
        "event: response.output_text.delta\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"你好世界\"}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":5,\"output_tokens\":4}}}\n\n"
    );
    let bytes = full.as_bytes();

    // Find "你" and split inside it
    let ni_start = bytes.windows(3).position(|w| w == "你".as_bytes()).unwrap();
    let split_point = ni_start + 2; // split after second byte of "你"

    let chunk1 = Bytes::from(bytes[..split_point].to_vec());
    let chunk2 = Bytes::from(bytes[split_point..].to_vec());

    let upstream = stream::iter(vec![
        Ok::<_, std::io::Error>(chunk1),
        Ok::<_, std::io::Error>(chunk2),
    ]);
    let converted = create_anthropic_sse_stream_from_responses(upstream);
    let chunks: Vec<_> = converted.collect().await;
    let merged = chunks
        .into_iter()
        .map(|c| String::from_utf8_lossy(c.unwrap().as_ref()).to_string())
        .collect::<String>();

    assert!(
        merged.contains("你好世界"),
        "expected '你好世界' in output, got replacement chars (U+FFFD)"
    );
    assert!(
        !merged.contains('\u{FFFD}'),
        "output must not contain U+FFFD replacement characters"
    );
}
async fn assert_diagnostic_late_error_preserves_terminal(
    event: &str,
    response: Value,
    stops: usize,
    errors: usize,
) {
    let prefix = format!(
        "event: response.created\ndata: {{\"type\":\"response.created\",\"response\":{{\"id\":\"resp_1\",\"model\":\"gpt-5\"}}}}\n\nevent: response.output_text.delta\ndata: {{\"type\":\"response.output_text.delta\",\"delta\":\"visible\"}}\n\nevent: {event}\ndata: {}\n\n",
        json!({"type":event,"response":response})
    );
    let expected = convert_stream_text(prefix.clone()).await;
    assert_eq!(expected.matches("event: message_stop\n").count(), stops);
    assert_eq!(expected.matches("event: error\n").count(), errors);
    let upstream = stream::iter(vec![
        Ok(Bytes::from(prefix)),
        Err(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "late error",
        )),
    ]);
    let chunks = create_anthropic_sse_stream_from_responses(upstream)
        .collect::<Vec<_>>()
        .await;
    let actual: String = chunks
        .into_iter()
        .map(|chunk| String::from_utf8(chunk.unwrap().to_vec()).unwrap())
        .collect();
    assert_eq!(
        actual, expected,
        "a terminal response must not gain a second terminal event"
    );
}

#[tokio::test]
async fn diagnostic_late_error_after_completed_is_ignored() {
    assert_diagnostic_late_error_preserves_terminal(
        "response.completed",
        json!({"status":"completed","usage":{"input_tokens":1,"output_tokens":1}}),
        1,
        0,
    )
    .await;
}

#[tokio::test]
async fn diagnostic_late_error_after_incomplete_is_ignored() {
    assert_diagnostic_late_error_preserves_terminal("response.incomplete", json!({"status":"incomplete","incomplete_details":{"reason":"max_output_tokens"},"usage":{"input_tokens":1,"output_tokens":1}}), 1, 0).await;
}

#[tokio::test]
async fn diagnostic_late_error_after_failed_is_ignored() {
    assert_diagnostic_late_error_preserves_terminal(
        "response.failed",
        json!({"status":"failed","error":{"type":"server_error","message":"semantic failure"}}),
        0,
        1,
    )
    .await;
}
