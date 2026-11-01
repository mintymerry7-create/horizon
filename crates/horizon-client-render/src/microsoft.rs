//! Microsoft account sign-in inside the game, for iOS, where the launcher doesn't run
//! (docs/microsoft-account.md): the launcher's account.rs flow, the OAuth device code, then Xbox
//! Live, XSTS, the Minecraft services, the ownership check and the profile. The signed-in account
//! becomes [`crate::auth::current`], which online-mode servers join with.
//!
//! The refresh and Minecraft tokens live in the iOS keychain, never in Documents (the Files app
//! shows it), logs or errors. The Azure app is the build's `HORIZON_MSA_CLIENT_ID`; without one,
//! sign-in says it is not configured.
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MICROSOFT: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0";
const XBOX_USER: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MINECRAFT: &str = "https://api.minecraftservices.com";
const SCOPE: &str = "XboxLive.signin offline_access";
/// The Minecraft token is renewed when it has less than this left.
const RENEW_BEFORE: u64 = 3600;

pub const NOT_CONFIGURED: &str = "This build has no Microsoft sign-in yet: it needs the project's Azure app (HORIZON_MSA_CLIENT_ID, docs/microsoft-account.md).";
const DOES_NOT_OWN: &str = "This account doesn't own Minecraft: Java Edition.";
const NO_PROFILE: &str =
    "This account has no Minecraft profile yet. Create one at minecraft.net, then sign in again.";
const SIGN_IN_AGAIN: &str = "Your Microsoft sign-in expired. Sign in again.";

/// The build's Azure app, if any.
pub fn client_id() -> Option<&'static str> {
    option_env!("HORIZON_MSA_CLIENT_ID")
        .map(str::trim)
        .filter(|id| !id.is_empty())
}

/// Where sign-in stands, for the Online options page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    SignedOut,
    /// Asking Microsoft for a code.
    Starting,
    /// The player types `user_code` at `uri` (microsoft.com/link).
    Code {
        user_code: String,
        uri: String,
    },
    /// Approved: Xbox Live, XSTS and the Minecraft services.
    Finishing,
    SignedIn {
        name: String,
    },
    Failed(String),
}

struct State {
    status: Status,
    /// Set to stop the sign-in under way.
    cancel: Option<Arc<AtomicBool>>,
}
static STATE: Mutex<State> = Mutex::new(State {
    status: Status::SignedOut,
    cancel: None,
});

pub fn status() -> Status {
    STATE.lock().unwrap().status.clone()
}
fn set_status(status: Status) {
    STATE.lock().unwrap().status = status;
}

/// The page that takes the code, with the code already in it.
pub fn link(user_code: &str, uri: &str) -> String {
    format!("{uri}?otc={user_code}")
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

// ----- Requests ------------------------------------------------------------------------------------

fn request(method: &str, url: &str) -> ureq::Request {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(20))
        .user_agent(&format!("Horizon/{}", horizon_core::release::VERSION))
        .build()
        .request(method, url)
}

enum Body<'a> {
    Form(Vec<(&'a str, &'a str)>),
    Json(Value),
}

/// The status and the JSON body (null when there is none). Transport errors name the service,
/// never the request.
fn send(request: ureq::Request, body: Option<Body>, service: &str) -> Result<(u16, Value), String> {
    let result = match body {
        None => request.call(),
        Some(Body::Form(fields)) => request.send_form(&fields),
        Some(Body::Json(value)) => request
            .set("Accept", "application/json")
            .set("Content-Type", "application/json")
            .send_string(&value.to_string()),
    };
    let response = match result {
        Ok(r) | Err(ureq::Error::Status(_, r)) => r,
        Err(ureq::Error::Transport(_)) => {
            return Err(format!(
                "Could not reach {service}. Check your internet connection."
            ));
        }
    };
    let status = response.status();
    let text = response.into_string().unwrap_or_default();
    Ok((status, serde_json::from_str(&text).unwrap_or(Value::Null)))
}

fn str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_owned)
}

// ----- The device-code flow ----------------------------------------------------------------------

struct DeviceCode {
    user_code: String,
    uri: String,
    interval: u64,
    expires_at: u64,
    device_code: String,
}

fn parse_device_code(status: u16, v: &Value) -> Result<DeviceCode, String> {
    match (
        status,
        str_field(v, "user_code"),
        str_field(v, "device_code"),
    ) {
        (200, Some(user_code), Some(device_code)) => Ok(DeviceCode {
            user_code,
            uri: str_field(v, "verification_uri")
                .unwrap_or_else(|| "https://www.microsoft.com/link".into()),
            interval: v
                .get("interval")
                .and_then(Value::as_u64)
                .unwrap_or(5)
                .max(1),
            expires_at: now() + v.get("expires_in").and_then(Value::as_u64).unwrap_or(900),
            device_code,
        }),
        _ => Err(oauth_error(v)),
    }
}

/// The OAuth error codes as messages for the player.
fn oauth_error(v: &Value) -> String {
    match v.get("error").and_then(Value::as_str) {
        Some("authorization_declined") => "Sign-in was declined.".into(),
        Some("expired_token") => "The sign-in code expired. Start again.".into(),
        Some("bad_verification_code") => "The sign-in code was not recognised. Start again.".into(),
        Some("invalid_grant") => SIGN_IN_AGAIN.into(),
        Some("invalid_client" | "unauthorized_client") => {
            "Microsoft refused this build's app registration (client ID).".into()
        }
        Some(other) => format!("Microsoft sign-in failed ({other})."),
        None => "Microsoft sign-in failed.".into(),
    }
}

struct MsTokens {
    access: String,
    refresh: String,
}

fn ms_tokens(status: u16, v: &Value) -> Result<MsTokens, String> {
    match (
        status,
        str_field(v, "access_token"),
        str_field(v, "refresh_token"),
    ) {
        (200, Some(access), Some(refresh)) => Ok(MsTokens { access, refresh }),
        _ => Err(oauth_error(v)),
    }
}

/// Polls as Microsoft asks until the player approves, the code expires or `cancel` is set.
fn wait_for_approval(
    client_id: &str,
    code: &DeviceCode,
    cancel: &AtomicBool,
) -> Result<MsTokens, String> {
    let mut interval = code.interval;
    loop {
        let until = std::time::Instant::now() + Duration::from_secs(interval);
        while std::time::Instant::now() < until {
            if cancel.load(Ordering::Relaxed) {
                return Err("Sign-in cancelled.".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if now() > code.expires_at {
            return Err("The sign-in code expired. Start again.".into());
        }
        let (status, v) = send(
            request("POST", &format!("{MICROSOFT}/token")),
            Some(Body::Form(vec![
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("client_id", client_id),
                ("device_code", &code.device_code),
            ])),
            "Microsoft",
        )?;
        match v.get("error").and_then(Value::as_str) {
            Some("authorization_pending") => {}
            Some("slow_down") => interval += 5,
            _ => return ms_tokens(status, &v),
        }
    }
}

fn refresh_ms(client_id: &str, refresh: &str) -> Result<MsTokens, String> {
    let (status, v) = send(
        request("POST", &format!("{MICROSOFT}/token")),
        Some(Body::Form(vec![
            ("grant_type", "refresh_token"),
            ("client_id", client_id),
            ("refresh_token", refresh),
            ("scope", SCOPE),
        ])),
        "Microsoft",
    )?;
    ms_tokens(status, &v)
}

// ----- Xbox Live, XSTS, Minecraft ----------------------------------------------------------------

/// The XSTS `XErr` codes, as the official launcher explains them.
fn xsts_error(code: u64) -> String {
    match code {
        2148916233 => "This Microsoft account has no Xbox profile yet. Sign in once at xbox.com to create one, then try again.".into(),
        2148916235 => "Xbox Live is not available in your country or region.".into(),
        2148916236 | 2148916237 => "This account needs adult verification on xbox.com before it can play.".into(),
        2148916238 => "This is a child account: an adult has to add it to a Microsoft family first.".into(),
        other => format!("Xbox Live refused the sign-in (error {other})."),
    }
}

struct Minecraft {
    token: String,
    expires_at: u64,
    xuid: Option<String>,
}

/// The Minecraft access token from the Microsoft one, through Xbox Live and XSTS.
fn minecraft_login(ms_access: &str) -> Result<Minecraft, String> {
    let (status, xbl) = send(
        request("POST", XBOX_USER),
        Some(Body::Json(json!({
            "Properties": {
                "AuthMethod": "RPS",
                "SiteName": "user.auth.xboxlive.com",
                "RpsTicket": format!("d={ms_access}"),
            },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT",
        }))),
        "Xbox Live",
    )?;
    let xbl_token = str_field(&xbl, "Token")
        .filter(|_| status == 200)
        .ok_or("Xbox Live refused the sign-in.")?;
    let (status, xsts) = send(
        request("POST", XSTS),
        Some(Body::Json(json!({
            "Properties": { "SandboxId": "RETAIL", "UserTokens": [xbl_token] },
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT",
        }))),
        "Xbox Live",
    )?;
    if let Some(code) = xsts.get("XErr").and_then(Value::as_u64) {
        return Err(xsts_error(code));
    }
    let claims = xsts.pointer("/DisplayClaims/xui/0");
    let (Some(xsts_token), Some(uhs)) = (
        str_field(&xsts, "Token").filter(|_| status == 200),
        claims.and_then(|c| str_field(c, "uhs")),
    ) else {
        return Err("Xbox Live refused the sign-in.".into());
    };
    let xuid = claims.and_then(|c| str_field(c, "xid"));
    let (token, expires_in) = minecraft_token(&format!("XBL3.0 x={uhs};{xsts_token}"))?;
    Ok(Minecraft {
        token,
        expires_at: now() + expires_in,
        xuid,
    })
}

/// `launcher/login` first, `login_with_xbox` as the fallback, as the launcher does.
fn minecraft_token(xtoken: &str) -> Result<(String, u64), String> {
    let lifetime = |v: &Value| v.get("expires_in").and_then(Value::as_u64).unwrap_or(86400);
    let launcher = send(
        request("POST", &format!("{MINECRAFT}/launcher/login")),
        Some(Body::Json(
            json!({ "xtoken": xtoken, "platform": "PC_LAUNCHER" }),
        )),
        "the Minecraft services",
    );
    if let Ok((200, v)) = &launcher
        && let Some(token) = str_field(v, "access_token")
    {
        return Ok((token, lifetime(v)));
    }
    let (status, mc) = send(
        request(
            "POST",
            &format!("{MINECRAFT}/authentication/login_with_xbox"),
        ),
        Some(Body::Json(json!({ "identityToken": xtoken }))),
        "the Minecraft services",
    )?;
    match (status, str_field(&mc, "access_token")) {
        (200, Some(token)) => Ok((token, lifetime(&mc))),
        // New Azure apps are refused until Mojang approves them (Minecraft API access).
        (403, _) => Err(
            "The Minecraft services refused this build: its app is not approved by Mojang yet."
                .into(),
        ),
        (429, _) => Err("Too many sign-ins in a short time. Wait a minute and try again.".into()),
        _ => Err("The Minecraft services refused the sign-in.".into()),
    }
}

fn bearer(request: ureq::Request, token: &str) -> ureq::Request {
    request.set("Authorization", &format!("Bearer {token}"))
}

/// Whether the entitlements list Minecraft: Java Edition (Game Pass included).
fn owns(v: &Value) -> bool {
    v.get("items")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|i| {
                matches!(
                    i.get("name").and_then(Value::as_str),
                    Some("game_minecraft" | "product_minecraft")
                )
            })
        })
}

/// The profile's undashed UUID and name; none for a 404 (no profile).
fn parse_profile(status: u16, v: &Value) -> Result<Option<(String, String)>, String> {
    if status == 404 {
        return Ok(None);
    }
    match (str_field(v, "id"), str_field(v, "name")) {
        (Some(uuid), Some(name)) => Ok(Some((uuid.replace('-', "").to_lowercase(), name))),
        _ => Err("Could not read the Minecraft profile.".into()),
    }
}

/// From Microsoft tokens to a ready account: Xbox, XSTS, Minecraft, ownership, profile.
fn finish(ms: MsTokens) -> Result<Stored, String> {
    let mc = minecraft_login(&ms.access)?;
    let (status, v) = send(
        bearer(
            request("GET", &format!("{MINECRAFT}/entitlements/mcstore")),
            &mc.token,
        ),
        None,
        "the Minecraft services",
    )?;
    if status != 200 {
        return Err("Could not check which games this account owns.".into());
    }
    if !owns(&v) {
        return Err(DOES_NOT_OWN.into());
    }
    let (status, v) = send(
        bearer(
            request("GET", &format!("{MINECRAFT}/minecraft/profile")),
            &mc.token,
        ),
        None,
        "the Minecraft services",
    )?;
    let (uuid, name) = parse_profile(status, &v)?.ok_or(NO_PROFILE)?;
    Ok(Stored {
        name,
        uuid,
        refresh_token: ms.refresh,
        minecraft_token: mc.token,
        minecraft_expires_at: mc.expires_at,
        xuid: mc.xuid,
    })
}

// ----- The account and where it is kept ----------------------------------------------------------

/// What the keychain keeps: the profile and its tokens.
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Stored {
    name: String,
    /// Undashed.
    uuid: String,
    refresh_token: String,
    minecraft_token: String,
    minecraft_expires_at: u64,
    #[serde(default)]
    xuid: Option<String>,
}
impl std::fmt::Debug for Stored {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stored")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}
impl Stored {
    fn session(&self) -> Option<crate::auth::AuthSession> {
        let mut session = crate::auth::AuthSession::new(
            &self.name,
            crate::auth::parse_uuid(&self.uuid)?,
            &self.minecraft_token,
        );
        session.xuid = self.xuid.clone();
        Some(session)
    }
}

#[cfg(target_os = "ios")]
mod keychain {
    use security_framework::passwords;
    const SERVICE: &str = "dev.playhorizon.ios";
    const ACCOUNT: &str = "microsoft";
    pub fn load() -> Option<String> {
        let bytes = passwords::get_generic_password(SERVICE, ACCOUNT).ok()?;
        String::from_utf8(bytes).ok()
    }
    pub fn save(text: &str) -> Result<(), String> {
        passwords::set_generic_password(SERVICE, ACCOUNT, text.as_bytes())
            .map_err(|_| "The account couldn't be saved in the keychain.".to_owned())
    }
    pub fn delete() {
        let _ = passwords::delete_generic_password(SERVICE, ACCOUNT);
    }
}

/// Elsewhere the launcher keeps accounts; this one lives as long as the game.
#[cfg(not(target_os = "ios"))]
mod keychain {
    use std::sync::Mutex;
    static KEPT: Mutex<Option<String>> = Mutex::new(None);
    pub fn load() -> Option<String> {
        KEPT.lock().unwrap().clone()
    }
    pub fn save(text: &str) -> Result<(), String> {
        *KEPT.lock().unwrap() = Some(text.to_owned());
        Ok(())
    }
    pub fn delete() {
        *KEPT.lock().unwrap() = None;
    }
}

fn load() -> Option<Stored> {
    serde_json::from_str(&keychain::load()?).ok()
}
fn save(stored: &Stored) -> Result<(), String> {
    keychain::save(&serde_json::to_string(stored).map_err(|e| e.to_string())?)
}

/// Makes `stored` the game's account: online-mode servers join with it from now on.
fn use_account(stored: &Stored) {
    if let Some(session) = stored.session() {
        crate::auth::set_current(session);
        crate::auth::prepare_chat_keys(true);
    }
    set_status(Status::SignedIn {
        name: stored.name.clone(),
    });
}

// ----- What the game calls -----------------------------------------------------------------------

/// At start: the kept account, its Minecraft token renewed in the background when it has less
/// than an hour left. No network wait before the title screen.
pub fn restore() {
    let Some(stored) = load() else {
        return;
    };
    if stored.minecraft_expires_at > now() + 60 {
        use_account(&stored);
    } else {
        set_status(Status::SignedIn {
            name: stored.name.clone(),
        });
    }
    if stored.minecraft_expires_at > now() + RENEW_BEFORE {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("Microsoft sign-in".into())
        .spawn(move || match renew(&stored) {
            Ok(renewed) => use_account(&renewed),
            Err(e) => {
                log::warn!("Horizon: the Microsoft account was not renewed: {e}");
                if e == SIGN_IN_AGAIN {
                    set_status(Status::Failed(e));
                }
            }
        });
}

fn renew(stored: &Stored) -> Result<Stored, String> {
    let id = client_id().ok_or(NOT_CONFIGURED)?;
    let ms = refresh_ms(id, &stored.refresh_token)?;
    let mc = minecraft_login(&ms.access)?;
    let renewed = Stored {
        refresh_token: ms.refresh,
        minecraft_token: mc.token,
        minecraft_expires_at: mc.expires_at,
        xuid: mc.xuid.or_else(|| stored.xuid.clone()),
        ..stored.clone()
    };
    save(&renewed)?;
    Ok(renewed)
}

/// Starts signing in: a code to type at microsoft.com/link, then the account.
pub fn sign_in() {
    let Some(id) = client_id() else {
        set_status(Status::Failed(NOT_CONFIGURED.into()));
        return;
    };
    let cancel = {
        let mut state = STATE.lock().unwrap();
        if matches!(
            state.status,
            Status::Starting | Status::Code { .. } | Status::Finishing
        ) {
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        state.cancel = Some(cancel.clone());
        state.status = Status::Starting;
        cancel
    };
    let spawned = std::thread::Builder::new()
        .name("Microsoft sign-in".into())
        .spawn(move || {
            let result = (|| {
                let (status, v) = send(
                    request("POST", &format!("{MICROSOFT}/devicecode")),
                    Some(Body::Form(vec![("client_id", id), ("scope", SCOPE)])),
                    "Microsoft",
                )?;
                let code = parse_device_code(status, &v)?;
                if cancel.load(Ordering::Relaxed) {
                    return Err("Sign-in cancelled.".into());
                }
                set_status(Status::Code {
                    user_code: code.user_code.clone(),
                    uri: code.uri.clone(),
                });
                let ms = wait_for_approval(id, &code, &cancel)?;
                set_status(Status::Finishing);
                let stored = finish(ms)?;
                save(&stored)?;
                Ok::<_, String>(stored)
            })();
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            match result {
                Ok(stored) => use_account(&stored),
                Err(e) => set_status(Status::Failed(e)),
            }
        });
    if spawned.is_err() {
        set_status(Status::Failed("Microsoft sign-in failed.".into()));
    }
}

/// Stops the sign-in under way.
pub fn cancel() {
    let mut state = STATE.lock().unwrap();
    if let Some(cancel) = &state.cancel {
        cancel.store(true, Ordering::Relaxed);
    }
    if matches!(
        state.status,
        Status::Starting | Status::Code { .. } | Status::Finishing
    ) {
        state.status = if load().is_some() {
            Status::SignedIn {
                name: load().map(|s| s.name).unwrap_or_default(),
            }
        } else {
            Status::SignedOut
        };
    }
}

/// Forgets the account: the keychain entry and the game's session.
pub fn sign_out() {
    cancel();
    keychain::delete();
    crate::auth::clear_current();
    set_status(Status::SignedOut);
}

/// What the Online page says about the account.
pub fn status_lines(status: &Status) -> Vec<String> {
    match status {
        Status::SignedOut if client_id().is_none() => vec![NOT_CONFIGURED.into()],
        Status::SignedOut => vec![
            "Not signed in: single player and LAN only.".into(),
            "Online servers need a Microsoft account that owns Minecraft: Java Edition.".into(),
        ],
        Status::Starting => vec!["Asking Microsoft for a code...".into()],
        Status::Code { user_code, .. } => vec![
            "Open microsoft.com/link and enter this code:".into(),
            format!("    {user_code}"),
            "The game finishes signing in once you approve.".into(),
        ],
        Status::Finishing => vec!["Signing in to Xbox Live and Minecraft...".into()],
        Status::SignedIn { name } => vec![
            format!("Signed in as {name}."),
            "Online servers use this account.".into(),
        ],
        Status::Failed(message) => vec![message.clone()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_codes() {
        let v = json!({"user_code": "ABCD1234", "device_code": "secret", "verification_uri": "https://www.microsoft.com/link", "interval": 0, "expires_in": 900});
        let code = parse_device_code(200, &v).unwrap();
        assert_eq!(
            (code.user_code.as_str(), code.interval),
            ("ABCD1234", 1),
            "an interval of at least a second"
        );
        assert!(code.expires_at >= now() + 899);
        assert_eq!(
            link(&code.user_code, &code.uri),
            "https://www.microsoft.com/link?otc=ABCD1234"
        );
        assert_eq!(
            parse_device_code(400, &json!({"error": "invalid_client"}))
                .err()
                .unwrap(),
            "Microsoft refused this build's app registration (client ID)."
        );
    }

    #[test]
    fn token_answers() {
        assert!(ms_tokens(200, &json!({"access_token": "a", "refresh_token": "r"})).is_ok());
        assert_eq!(
            ms_tokens(400, &json!({"error": "invalid_grant"})).err(),
            Some(SIGN_IN_AGAIN.to_owned())
        );
        assert_eq!(
            ms_tokens(400, &json!({"error": "authorization_declined"})).err(),
            Some("Sign-in was declined.".to_owned())
        );
        assert!(xsts_error(2148916238).contains("child account"));
    }

    #[test]
    fn ownership_and_profile() {
        assert!(owns(
            &json!({"items": [{"name": "product_minecraft"}, {"name": "game_minecraft"}]})
        ));
        assert!(!owns(&json!({"items": []})));
        assert!(!owns(&Value::Null));
        assert_eq!(
            parse_profile(
                200,
                &json!({"id": "069A79F444E94726A5BEFCA90E38AAF5", "name": "Notch"})
            ),
            Ok(Some((
                "069a79f444e94726a5befca90e38aaf5".into(),
                "Notch".into()
            )))
        );
        assert_eq!(parse_profile(404, &Value::Null), Ok(None));
        assert!(parse_profile(200, &json!({})).is_err());
    }

    #[test]
    fn the_kept_account_hides_its_tokens() {
        let stored = Stored {
            name: "Notch".into(),
            uuid: "069a79f444e94726a5befca90e38aaf5".into(),
            refresh_token: "refresh-secret".into(),
            minecraft_token: "mc-secret".into(),
            minecraft_expires_at: 1,
            xuid: Some("25".into()),
        };
        let text = serde_json::to_string(&stored).unwrap();
        assert_eq!(serde_json::from_str::<Stored>(&text).unwrap(), stored);
        let shown = format!("{stored:?}");
        assert!(!shown.contains("secret"), "{shown}");
        let session = stored.session().unwrap();
        assert_eq!(session.uuid, 0x069a79f444e94726a5befca90e38aaf5);
        assert_eq!(session.xuid.as_deref(), Some("25"));
    }

    #[test]
    fn status_lines_show_the_code() {
        let lines = status_lines(&Status::Code {
            user_code: "ABCD1234".into(),
            uri: "https://www.microsoft.com/link".into(),
        });
        assert!(lines.iter().any(|l| l.trim() == "ABCD1234"));
        assert_eq!(
            status_lines(&Status::SignedIn {
                name: "Notch".into()
            })[0],
            "Signed in as Notch."
        );
    }
}
