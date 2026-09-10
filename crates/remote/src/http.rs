use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("request failed: {0}")]
    Transport(String),
    #[error("unexpected status {status}")]
    Status { status: u16, body: String },
}

pub struct HttpClient {
    agent: ureq::Agent,
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpClient {
    pub fn new() -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            .build();
        Self {
            agent: config.into(),
        }
    }

    pub fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<String, HttpError> {
        let mut req = self.agent.get(url);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let mut resp = req.call().map_err(|e| HttpError::Transport(e.to_string()))?;
        let status = resp.status().as_u16();
        let body = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| HttpError::Transport(e.to_string()))?;
        if !(200..300).contains(&status) {
            return Err(HttpError::Status { status, body });
        }
        Ok(body)
    }

    pub fn post_form(&self, url: &str, headers: &[(&str, &str)], form: &[(&str, &str)]) -> Result<String, HttpError> {
        let mut req = self.agent.post(url);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let mut resp = req
            .send_form(form.iter().map(|(k, v)| (*k, *v)))
            .map_err(|e| HttpError::Transport(e.to_string()))?;
        let status = resp.status().as_u16();
        let body = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| HttpError::Transport(e.to_string()))?;
        if !(200..300).contains(&status) {
            return Err(HttpError::Status { status, body });
        }
        Ok(body)
    }

    pub fn post_json(&self, url: &str, headers: &[(&str, &str)], body: &str) -> Result<String, HttpError> {
        let mut req = self.agent.post(url).header("content-type", "application/json");
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let mut resp = req
            .send(body)
            .map_err(|e| HttpError::Transport(e.to_string()))?;
        let status = resp.status().as_u16();
        let text = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| HttpError::Transport(e.to_string()))?;
        if !(200..300).contains(&status) {
            return Err(HttpError::Status { status, body: text });
        }
        Ok(text)
    }
}
