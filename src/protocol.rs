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
    #[serde(default, rename = "bankedResets")]
    pub banked_resets: Option<BankedResets>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BankedResets {
    #[serde(default, rename = "availableCount")]
    pub available_count: Option<u32>,
    #[serde(default)]
    pub credits: Option<Vec<BankedResetCredit>>,
    #[serde(default, rename = "fetchedAt")]
    pub fetched_at: String,
    #[serde(default)]
    pub stale: String,
    #[serde(default)]
    pub error: String,
    #[serde(default, rename = "canRedeem")]
    pub can_redeem: bool,
    #[serde(default, rename = "pendingRequest")]
    pub pending_request: Option<PendingResetRequest>,
    #[serde(default, rename = "autoReset")]
    pub auto_reset: bool,
    #[serde(default, rename = "autoResetStatus")]
    pub auto_reset_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BankedResetCredit {
    #[serde(default)]
    pub id: String,
    #[serde(default, rename = "resetType")]
    pub reset_type: String,
    #[serde(default)]
    pub status: String,
    #[serde(default, rename = "grantedAt")]
    pub granted_at: String,
    #[serde(default, rename = "expiresAt")]
    pub expires_at: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, rename = "canRedeem")]
    pub can_redeem: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PendingResetRequest {
    #[serde(default, rename = "requestId")]
    pub request_id: String,
    #[serde(default, rename = "creditId")]
    pub credit_id: Option<String>,
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
        let banked: Vec<Account> = serde_json::from_str(include_str!(
            "../tests/fixtures/frontend/banked-resets.json"
        ))
        .expect("banked resets fixture");
        assert_eq!(banked.len(), 2);
        let available = banked[0]
            .banked_resets
            .as_ref()
            .expect("available reset state");
        assert_eq!(available.available_count, Some(3));
        assert_eq!(
            available
                .credits
                .as_ref()
                .expect("loaded credit details")
                .len(),
            1
        );
        assert!(available.auto_reset);
        assert!(available.credits.as_ref().unwrap()[0].can_redeem);
        let pending = banked[1]
            .banked_resets
            .as_ref()
            .expect("pending reset state");
        assert_eq!(pending.available_count, None);
        assert!(pending.credits.is_none());
        assert_eq!(
            pending.pending_request.as_ref().unwrap().request_id,
            "12345678-1234-4234-8234-123456789abc"
        );
        assert!(!pending.auto_reset);
        let _: Event =
            serde_json::from_str(include_str!("../tests/fixtures/frontend/cancelled.json"))
                .expect("cancelled fixture");
        let _: Event =
            serde_json::from_str(include_str!("../tests/fixtures/frontend/failure.json"))
                .expect("failure fixture");
    }

    #[test]
    fn banked_reset_unknown_count_and_missing_details_stay_unknown() {
        let account: Account = serde_json::from_str(r#"{"provider":"codex","bankedResets":{"availableCount":null,"credits":null,"fetchedAt":"","canRedeem":false,"autoReset":false,"future":true}}"#).unwrap();
        let resets = account.banked_resets.unwrap();
        assert_eq!(resets.available_count, None);
        assert!(resets.credits.is_none());
        assert!(!resets.auto_reset);
    }

    #[test]
    fn banked_reset_capped_credit_list_does_not_infer_count() {
        let account: Account = serde_json::from_str(r#"{"bankedResets":{"availableCount":9,"credits":[{"id":"c1","canRedeem":true}],"canRedeem":true,"autoReset":true,"autoResetStatus":"enabled"}}"#).unwrap();
        let resets = account.banked_resets.unwrap();
        assert_eq!(resets.available_count, Some(9));
        assert_eq!(resets.credits.unwrap().len(), 1);
        assert!(resets.auto_reset);
    }
}
