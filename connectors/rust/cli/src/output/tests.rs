use super::*;

fn api_error(status: u16, body: &str, retry_after: Option<u64>) -> ApiError {
    let f = Fields::parse(body).unwrap();
    ApiError {
        status,
        code: f.string("code").unwrap(),
        message: f.string("message").unwrap(),
        request_id: f.string("requestId"),
        retry_after,
        details: f.raw("details").map(str::to_owned),
        body: body.as_bytes().to_vec(),
    }
}

/// UC: UC-CLI-004
#[test]
fn json_is_the_servers_answer_indented_and_raw_is_left_alone() {
    assert_eq!(
        render("{\"a\":[1,2]}", Format::Json),
        "{\n  \"a\": [\n    1,\n    2\n  ]\n}"
    );
    assert_eq!(render("not json  \n", Format::Json), "not json");
    assert_eq!(render("{\"a\":1}\n", Format::Raw), "{\"a\":1}");
    assert_eq!(Format::parse("table"), Ok(Format::Table));
    assert!(Format::parse("xml").is_err());
}

/// UC: UC-CLI-004
#[test]
fn table_lists_the_items_of_a_page_with_a_column_per_field() {
    let page = "{\"items\":[{\"id\":\"i1\",\"name\":\"orders\",\"fifo\":true,\"ttl\":null,\"tags\":{\"a\": \"b\"}},{\"id\":\"i2\",\"name\":\"x\",\"extra\":7}],\"nextCursor\":\"c\"}";
    let out = table(page).unwrap();
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "id  name    fifo  ttl  tags       extra");
    assert_eq!(lines[1], "i1  orders  true  -    {\"a\":\"b\"}  -");
    assert_eq!(lines[2], "i2  x       -     -    -          7");
}

/// UC: UC-CLI-004
#[test]
fn table_of_one_object_is_field_and_value_and_an_empty_list_says_so() {
    let out = table("{\"health\":\"ready\",\"reason\":\"draining\"}").unwrap();
    assert_eq!(out, "field   value\nhealth  ready\nreason  draining");
    assert_eq!(table("{\"items\":[]}").unwrap(), "(empty)");
    assert_eq!(table("{\"messages\":[]}").unwrap(), "(empty)");
    assert!(table("text").is_none());
    assert!(table("[1,2]").is_none());
    // With no possible table, it falls back to the indented JSON.
    assert_eq!(render("[1,2]", Format::Table), "[\n  1,\n  2\n]");
}

/// UC: UC-CLI-004
#[test]
fn long_cells_are_cut() {
    let long = "x".repeat(100);
    let out = table(&format!("[{{\"v\":\"{long}\"}}]")).unwrap();
    assert!(out.lines().nth(1).unwrap().chars().count() <= 60);
    assert!(out.ends_with('…'));
}

/// UC: UC-CLI-005
#[test]
fn api_errors_print_code_message_request_id_details_and_retry_after() {
    let body = "{\"code\":\"validation.failed\",\"message\":\"Invalid fields.\",\"requestId\":\"r-1\",\"details\":[{\"field\":\"name\",\"code\":\"validation.required\",\"message\":\"required\"}]}";
    let mut out = Vec::new();
    write_api_error(&mut out, &api_error(400, body, Some(5)), false).unwrap();
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "validation.failed: Invalid fields. (requestId r-1)\n  - name: required (validation.required)\nRetry-After: 5 s\n"
    );
}

/// UC: UC-CLI-005
#[test]
fn with_json_the_error_response_is_printed_as_it_came() {
    let body = "{\"code\":\"topic.not-found\",\"message\":\"Does not exist.\",\"requestId\":\"r\"}";
    let mut out = Vec::new();
    write_api_error(&mut out, &api_error(404, body, None), true).unwrap();
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "{\n  \"code\": \"topic.not-found\",\n  \"message\": \"Does not exist.\",\n  \"requestId\": \"r\"\n}\n"
    );
}

/// UC: UC-CLI-016
#[test]
fn the_cluster_status_table_has_one_row_per_node_and_per_condition() {
    let status = "{\"scope\":\"cluster\",\"partial\":false,\"worstCondition\":\"warning\",\"conditions\":[{\"condition\":\"under-replicated\",\"severity\":\"warning\"}],\"summary\":{\"members\":{\"active\":2},\"quorumAvailable\":true},\"nodes\":[{\"nodeId\":\"n1\",\"reachable\":true},{\"nodeId\":\"n2\",\"reachable\":false,\"status\":null}]}";
    let out = cluster_table(status).unwrap();
    assert!(out.starts_with("field           value\nscope           cluster\npartial         false\nworstCondition  warning\n"), "{out}");
    assert!(
        out.contains(
            "\n\nnodes\nnodeId  reachable  status\nn1      true       -\nn2      false      -"
        ),
        "{out}"
    );
    assert!(
        out.contains("\n\nconditions\ncondition         severity\nunder-replicated  warning"),
        "{out}"
    );
    assert!(cluster_table("not json").is_none());
}
