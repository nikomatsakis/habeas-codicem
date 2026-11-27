//! Scripted Ollama endpoint for exercising reviews through the configured SDK agent.
use super::*;
use std::io::{BufRead, BufReader, Read};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

pub(super) struct RecordingProvider {
    backend: config::BackendConfig,
    pub requests: Arc<Mutex<Vec<Value>>>,
}
impl std::ops::Deref for RecordingProvider {
    type Target = config::BackendConfig;
    fn deref(&self) -> &Self::Target {
        &self.backend
    }
}
impl RecordingProvider {
    pub fn new(turns: Vec<(&'static str, Value)>) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        std::thread::spawn(move || {
            for (name, arguments) in turns {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(&mut stream);
                let mut length = 0;
                for line in reader.by_ref().lines().map(Result::unwrap) {
                    if line.is_empty() {
                        break;
                    }
                    if let Some((key, value)) = line.split_once(':')
                        && key.eq_ignore_ascii_case("content-length")
                    {
                        length = value.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let request: Value = serde_json::from_slice(&body).unwrap();
                let arguments = if name == "finish_search" && arguments == "first" {
                    let page = request["messages"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .rev()
                        .filter(|m| m["role"] == "tool")
                        .filter_map(|m| serde_json::from_str::<Value>(m["content"].as_str()?).ok())
                        .find(|r| r.get("matches").is_some())
                        .unwrap();
                    json!({"search_id":page["search_id"], "used":[page["matches"][0]["tag"]]})
                } else {
                    arguments
                };
                recorded.lock().unwrap().push(request);
                let reply = json!({"model":"fixture", "created_at":"2026-01-01T00:00:00Z", "message":{"role":"assistant","content":"","tool_calls":[{"function":{"name":name,"arguments":arguments}}]},"done":true,"done_reason":"stop","prompt_eval_count":11,"eval_count":7}).to_string();
                write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",reply.len()).unwrap();
            }
        });
        let backend = serde_json::from_value(
            json!({"backend":"ollama","model":"fixture","endpoint":endpoint}),
        )
        .unwrap();
        Self { requests, backend }
    }
    pub fn assert_tools(&self, turn: usize) {
        let requests = self.requests.lock().unwrap();
        let mut names: Vec<_> = requests[turn]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["function"]["name"].as_str().unwrap())
            .collect();
        names.sort_unstable();
        assert_eq!(
            names,
            [
                "finish_search",
                "get_next_page",
                "github",
                "list_sources",
                "lookup_requirements",
                "outgoing_edges",
                "outline_source",
                "read_node",
                "run_named_query",
                "search_structure",
                "search_text",
                "submit_result"
            ]
        );
    }
    pub fn prompts(&self) -> Vec<(String, Vec<String>)> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|r| {
                (
                    r.to_string(),
                    r["tools"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|t| t["function"]["name"].as_str().unwrap().to_owned())
                        .collect(),
                )
            })
            .collect()
    }
}
