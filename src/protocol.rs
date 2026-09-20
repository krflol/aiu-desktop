use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Account {
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub org: String,
    #[serde(default)]
    pub label: String,
    #[serde(default, rename = "orgName")]
    pub org_name: String,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub tier: String,
    #[serde(default, rename = "readOnly")]
    pub read_only: bool,
    #[serde(default, rename = "canSwitch")]
    pub can_switch: bool,
    #[serde(default)]
    pub windows: Vec<Window>,
    #[serde(default)]
    pub stale: String,
    #[serde(default, rename = "fetchedAt")]
    pub fetched_at: String,
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub recommended: bool,
    #[serde(default)]
    pub why: String,
    #[serde(default, rename = "allSpent")]
    pub all_spent: bool,
    #[serde(default)]
    pub login: Login,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Login {
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Window {
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub percent: f64,
    #[serde(default = "default_true")]
    pub known: bool,
    #[serde(default, rename = "resetsAt")]
    pub resets_at: String,
    #[serde(default)]
    pub severity: String,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "event")]
pub enum Event {
    #[serde(rename = "hello")]
    Hello {
        version: u8,
        #[serde(default)]
        capabilities: Vec<String>,
    },
    #[serde(rename = "progress")]
    Progress {
        version: u8,
        #[serde(rename = "phase")]
        _phase: String,
        message: String,
    },
    #[serde(rename = "result")]
    Result {
        version: u8,
        ok: bool,
        cancelled: bool,
        #[serde(default)]
        message: String,
        accounts: Option<Vec<Account>>,
        error: Option<Error>,
    },
    #[serde(rename = "error")]
    ErrorResponse {
        version: u8,
        code: String,
        message: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub struct Error {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Cancel {
    pub cancel: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_rows_decode_unknown_and_locked_windows() {
        let rows = r#"[{"provider":"claude","email":"a@example.test","label":"A","canSwitch":true,"windows":[{"label":"session","known":false,"severity":"locked"}]}]"#;
        let accounts: Vec<Account> = serde_json::from_str(rows).expect("legacy rows");
        assert!(!accounts[0].windows[0].known);
        assert_eq!(accounts[0].windows[0].severity, "locked");
    }

    #[test]
    fn missing_known_field_defaults_to_known() {
        let window: Window = serde_json::from_str(r#"{"percent":12}"#).expect("window");
        assert!(window.known);
    }

    #[test]
    fn shared_frontend_fixtures_decode_terminal_results() {
        let accounts: Vec<Account> =
            serde_json::from_str(include_str!("../tests/fixtures/frontend/accounts.json"))
                .expect("accounts fixture");
        assert!(!accounts.is_empty());
        assert_eq!(accounts[0].org, "org-work");
        assert!(accounts[0].can_switch);
        let empty: Vec<Account> =
            serde_json::from_str(include_str!("../tests/fixtures/frontend/empty.json"))
                .expect("empty fixture");
        assert!(empty.is_empty());
        let _: Event =
            serde_json::from_str(include_str!("../tests/fixtures/frontend/cancelled.json"))
                .expect("cancelled fixture");
        let _: Event =
            serde_json::from_str(include_str!("../tests/fixtures/frontend/failure.json"))
                .expect("failure fixture");
    }
}
