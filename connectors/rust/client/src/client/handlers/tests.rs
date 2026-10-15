use super::*;
use crate::testing::{client, MockTransport};
use crate::Config;

fn message_json(id: &str, body: &str) -> String {
    format!(
        "{{\"id\":\"{id}\",\"body\":{body},\"contentType\":\"application/json\",\
         \"receiptToken\":\"rt_{id}\",\"deliveryCount\":2,\"publishedAt\":\"2026-01-01T00:00:00Z\",\
         \"invisibleUntil\":\"2026-01-01T00:02:00Z\"}}"
    )
}

fn messages_json(messages: &[String]) -> String {
    format!("{{\"messages\":[{}]}}", messages.join(","))
}

fn methods(mock: &MockTransport) -> Vec<String> {
    mock.seen()
        .iter()
        .map(|s| s.request.method.clone())
        .collect()
}

/// UC: UC-CNT-034
#[test]
fn consume_with_deletes_by_default_keeps_on_request_and_never_after_an_error() {
    let mock = MockTransport::new();
    mock.reply(200, &[], &messages_json(&[message_json("m1", "{\"n\":1}")]));
    mock.reply(204, &[], "");
    mock.reply(200, &[], &messages_json(&[message_json("m2", "{\"n\":2}")]));
    mock.reply(200, &[], &messages_json(&[message_json("m3", "{\"n\":3}")]));
    let client = client(Config::new("http://taba:8080"), &mock);

    let mut seen = None;
    let handled = client
        .consume_with("orders", &ConsumeOptions::default(), |d| {
            seen = Some((d.message.id.clone(), d.message.delivery_count));
            Ok(())
        })
        .unwrap();
    assert_eq!(handled, 1);
    assert_eq!(seen, Some(("m1".to_owned(), 2)), "the whole message");
    assert_eq!(methods(&mock), ["GET", "DELETE"], "deleted by default");
    assert_eq!(
        mock.seen()[0].request.target,
        "/topics/orders/messages?maxMessages=1&visibilityTimeout=120&waitTime=20",
        "consume-with-handler defaults"
    );

    client
        .consume_with("orders", &ConsumeOptions::default(), |d| {
            d.delete = false;
            Ok(())
        })
        .unwrap();
    assert_eq!(mock.seen().len(), 3, "delete = false: does not delete");

    let err = client
        .consume_with("orders", &ConsumeOptions::default(), |_| {
            Err("it failed".into())
        })
        .unwrap_err();
    assert!(matches!(err, ConsumeError::Handler(e) if e.to_string() == "it failed"));
    assert_eq!(mock.seen().len(), 4, "with an error, does not delete");
}

/// UC: UC-CNT-034
#[test]
fn consume_with_renews_the_visibility_while_the_handler_runs_and_lets_it_change() {
    let mock = MockTransport::new();
    mock.reply(200, &[], &messages_json(&[message_json("m1", "{}")]));
    mock.reply(
        200,
        &[],
        "{\"id\":\"m1\",\"invisibleUntil\":\"2026-01-01T00:01:00Z\"}",
    );
    mock.reply(204, &[], "");
    let client = client(Config::new("http://taba:8080"), &mock);
    let options = ConsumeOptions {
        visibility_timeout: Some(1),
        ..ConsumeOptions::default()
    };

    client
        .consume_with("orders", &options, |d| {
            assert_eq!(d.lease_seconds(), 1);
            // The renewal comes every third of 1 s.
            std::thread::sleep(Duration::from_millis(550));
            Ok(())
        })
        .unwrap();

    let seen = mock.seen();
    assert_eq!(
        methods(&mock),
        ["GET", "PUT", "DELETE"],
        "one renewal while the handler ran"
    );
    assert_eq!(
        seen[1].request.target,
        "/topics/orders/messages/m1/visibility?receiptToken=rt_m1"
    );
    assert_eq!(seen[1].body_text(), "{\"visibilityTimeout\":1}");
}

/// UC: UC-CNT-034
#[test]
fn change_visibility_in_the_handler_becomes_the_renewal_value() {
    let mock = MockTransport::new();
    mock.reply(200, &[], &messages_json(&[message_json("m1", "{}")]));
    mock.reply(200, &[], "{\"id\":\"m1\",\"invisibleUntil\":\"x\"}");
    mock.reply(204, &[], "");
    let client = client(Config::new("http://taba:8080"), &mock);

    client
        .consume_with("orders", &ConsumeOptions::default(), |d| {
            d.change_visibility(300).unwrap();
            assert_eq!(d.lease_seconds(), 300);
            assert!(!d.lease_lost());
            Ok(())
        })
        .unwrap();
    assert_eq!(mock.seen()[1].body_text(), "{\"visibilityTimeout\":300}");
}

#[cfg(feature = "serde")]
mod typed {
    use super::*;

    #[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq)]
    #[serde(rename_all = "camelCase")]
    struct Order {
        number: u32,
        items: Vec<String>,
    }

    /// UC: UC-CNT-033
    #[test]
    fn publish_object_serializes_and_sends_json() {
        let mock = MockTransport::new();
        mock.reply(201, &[], "{\"id\":\"m1\"}");
        let client = client(Config::new("http://taba:8080"), &mock);
        client
            .publish_object(
                "orders",
                &Order {
                    number: 9001,
                    items: vec!["a".into(), "b".into()],
                },
            )
            .unwrap();
        let seen = mock.seen();
        assert_eq!(
            seen[0].body_text(),
            "{\"number\":9001,\"items\":[\"a\",\"b\"]}"
        );
        assert_eq!(seen[0].header("content-type"), Some("application/json"));
    }

    /// UC: UC-CNT-034
    #[test]
    fn consume_typed_hands_over_the_deserialized_body_and_refuses_a_body_that_is_not_a_t() {
        let mock = MockTransport::new();
        mock.reply(
            200,
            &[],
            &messages_json(&[message_json("m1", "{\"number\":9001,\"items\":[\"a\"]}")]),
        );
        mock.reply(204, &[], "");
        mock.reply(200, &[], &messages_json(&[message_json("m2", "[1,2]")]));
        let client = client(Config::new("http://taba:8080"), &mock);

        let mut got = None;
        client
            .consume_typed::<Order, _>("orders", &ConsumeOptions::default(), |d| {
                assert_eq!(d.message.id, "m1", "the whole message is still available");
                got = Some(std::mem::replace(
                    &mut d.body,
                    Order {
                        number: 0,
                        items: vec![],
                    },
                ));
                Ok(())
            })
            .unwrap();
        assert_eq!(
            got,
            Some(Order {
                number: 9001,
                items: vec!["a".into()]
            })
        );

        let mut called = false;
        let err = client
            .consume_typed::<Order, _>("orders", &ConsumeOptions::default(), |_| {
                called = true;
                Ok(())
            })
            .unwrap_err();
        assert!(matches!(err, ConsumeError::Api(e) if e.code == "client.invalid-response"));
        assert!(!called);
        assert_eq!(
            methods(&mock),
            ["GET", "DELETE", "GET"],
            "the one that is not an Order is not deleted"
        );
    }
}
