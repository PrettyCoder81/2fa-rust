use rocket::Request;

#[allow(dead_code)]
pub struct RequestMetadata {
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
}

#[allow(dead_code)]
impl RequestMetadata {
    pub fn from_request(request: &Request<'_>) -> Self {
        Self {
            ip_address: request.client_ip().map(|ip| ip.to_string()),

            user_agent: request.headers().get_one("User-Agent").map(str::to_string),
        }
    }

    pub fn ip(&self) -> Option<&str> {
        self.ip_address.as_deref()
    }

    pub fn user_agent(&self) -> Option<&str> {
        self.user_agent.as_deref()
    }
}
