/// Rust API server module that interoperates with TypeScript frontend

pub struct ApiServer {
    port: u16,
}

impl ApiServer {
    pub fn new(port: u16) -> Self {
        Self { port }
    }

    pub fn start(&self) {
        println!("Starting API server on port {}", self.port);
    }

    pub fn handle_request(&self, path: &str) -> String {
        match path {
            "/health" => r#"{"status":"ok"}"#.to_string(),
            "/users" => r#"[{"id":1,"name":"Alice"}]"#.to_string(),
            _ => r#"{"error":"not found"}"#.to_string(),
        }
    }
}

pub fn create_server(port: u16) -> ApiServer {
    ApiServer::new(port)
}
