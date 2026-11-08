use super::*;

#[test]
fn sse_cr_only_and_split_crlf_are_equivalent() {
    for chunks in [
        vec![b"data: one\r\r".as_slice()],
        vec![b"data: one\r".as_slice(), b"\n\r", b"\n"],
    ] {
        let mut decoder = SseDecoder::default();
        let mut events = Vec::new();
        for chunk in chunks {
            decoder.feed(chunk, &mut |event| events.push(event.to_string()));
        }
        decoder.finish(&mut |event| events.push(event.to_string()));
        assert_eq!(events, vec!["one"]);
    }
}

#[test]
fn sse_empty_data_lines_count_toward_event_budget() {
    let mut decoder = SseDecoder::default();
    for _ in 0..(SseDecoder::MAX_EVENT_BYTES / 6 + 2) {
        decoder.feed(b"data:\n", &mut |_| {});
        if decoder.failed {
            break;
        }
    }
    assert!(
        decoder.failed,
        "empty data lines must not grow the event unbounded"
    );
    assert!(decoder.data_lines.is_empty());
}

#[test]
fn sse_invalid_utf8_is_an_explicit_error() {
    let mut decoder = SseDecoder::default();
    let mut events = Vec::new();
    decoder.feed(b"data: \xff\n\n", &mut |event| {
        events.push(event.to_string())
    });
    assert!(decoder.failed);
    assert!(events.iter().any(|event| event.contains("UTF-8")));
}

#[tokio::test]
async fn sse_enforces_a_total_wire_budget_across_small_events() {
    let event = format!(
        "data: {{\"choices\":[{{\"delta\":{{\"content\":\"{}\"}}}}]}}\n\n",
        "x".repeat(1024)
    );
    let count = MAX_PROVIDER_RESPONSE_BYTES / event.len() + 1;
    let (base, server) = tests::sse_server(event.repeat(count) + "data: [DONE]\n\n").await;
    let result = tests::stream_against(&base).await;
    server.abort();
    assert!(result
        .expect_err("total response must be bounded")
        .contains("size limit"));
}

#[tokio::test]
async fn sse_ignores_events_after_done_in_the_same_packet() {
    let (base, server) = tests::sse_server(
        concat!(
            "data: {\"choices\":[{\"delta\":{\"content\":\"correct\"}}]}\n\n",
            "data: [DONE]\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"incorrect\"}}]}\n\n"
        )
        .into(),
    )
    .await;
    let result = tests::stream_against(&base).await.unwrap();
    let _ = server.await;
    assert_eq!(result.content, "correct");
}
