use ahjo_node_kit::NodeClient;
use serde_json::{Value, json};

pub async fn call(
    client: &NodeClient,
    name: &str,
    arguments: Value,
) -> Result<Value, ahjo_core::Error> {
    let request = json!({"jsonrpc":"2.0", "id":1, "method":"tools/call", "params":{"name":name, "arguments":arguments}});
    let (answer, _) = client
        .request(http::Method::POST, "/mcp", Some(request), None)
        .await
        .expect("node answers");
    let result = &answer["result"];
    let text = result["content"][0]["text"].as_str().expect("command text");
    if result["isError"] == false {
        Ok(serde_json::from_str(text).expect("command JSON"))
    } else {
        Err(serde_json::from_str(text).expect("command error"))
    }
}
