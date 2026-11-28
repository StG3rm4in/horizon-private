//! Ely.by credentials for Noelcraft's local launch path.

use super::{keychain_or_why, session, SecretStore};
use crate::i18n::{tr, trf};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    io::{self, IsTerminal, Read, Write},
    time::Duration,
};

#[path = "ely_local.rs"]
mod local;
pub use local::{is_missing_game, play, server_address};

// Microsoft account operations look up Secrets by profile UUID.
const ENTRY: &str = "noelcraft-ely-v1";
const CONSOLE_REQUIRED: &str = "Lancez horizon-launcher ely-account sign-in depuis PowerShell \
    ou le Terminal Windows. La saisie masquée du mot de passe exige une vraie console.";

#[derive(Serialize, Deserialize)]
struct Profile {
    id: String,
    name: String,
}

impl Profile {
    fn identity(&self) -> Option<String> {
        if self.id.contains('-') && !self.id.split('-').map(str::len).eq([8, 4, 4, 4, 12]) {
            return None;
        }
        let id = self.id.replace('-', "");
        (id.len() == 32
            && id.bytes().all(|b| b.is_ascii_hexdigit())
            && id.bytes().any(|b| b != b'0')
            && (1..=16).contains(&self.name.len())
            && self
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_'))
        .then(|| id.to_ascii_lowercase())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Credential {
    access_token: String,
    client_token: String,
    selected_profile: Profile,
}

impl Credential {
    fn game_session(&self) -> Value {
        json!({
            "name": self.selected_profile.name,
            "uuid": self.selected_profile.identity().expect("validated profile"),
            "access_token": self.access_token,
            "user_type": "ely",
        })
    }
}

enum Reply {
    Ok(Value),
    Totp,
    Rejected,
}

fn request(action: &str, payload: Value) -> Result<Reply, String> {
    let base = horizon_addresses::ELY_AUTH;
    let response = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout(Duration::from_secs(15))
        .build()
        .post(&format!("{base}/{action}"))
        .send_json(payload);
    let response = match response {
        Ok(response) if response.status() == 200 => response,
        Err(ureq::Error::Status(400 | 401 | 403, response)) => {
            if response.status() == 401 && action == "authenticate" {
                let mut body = Vec::new();
                response
                    .into_reader()
                    .take(16384)
                    .read_to_end(&mut body)
                    .map_err(|_| tr("Could not read the Ely.by response"))?;
                if serde_json::from_slice::<Value>(&body).is_ok_and(|v| {
                    v["error"] == "ForbiddenOperationException"
                        && v["errorMessage"] == "Account protected with two factor auth."
                }) {
                    return Ok(Reply::Totp);
                }
            }
            return Ok(Reply::Rejected);
        }
        _ => return Err(tr("Ely.by authentication service unavailable").into()),
    };
    let mut body = Vec::new();
    response
        .into_reader()
        .take(65537)
        .read_to_end(&mut body)
        .map_err(|_| tr("Could not read the Ely.by response"))?;
    if body.len() > 65536 {
        return Err(tr("Ely.by response exceeded the limit").into());
    }
    serde_json::from_slice(&body)
        .map(Reply::Ok)
        .map_err(|_| tr("Malformed Ely.by response").into())
}

fn token(value: &Value, field: &str) -> Result<String, String> {
    value[field]
        .as_str()
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| trf("Ely.by response has no {field}", &[("field", field)]))
}

fn read_profile(value: &Value) -> Result<Profile, String> {
    let profile: Profile = serde_json::from_value(value.clone())
        .map_err(|_| tr("Ely.by returned no valid selected profile"))?;
    if profile.identity().is_none() {
        return Err(tr("Ely.by returned no valid selected profile").into());
    }
    Ok(profile)
}

fn signed_in(reply: Reply, client_token: String) -> Result<Credential, String> {
    let Reply::Ok(value) = reply else {
        return Err(tr("Ely.by sign-in was refused").into());
    };
    if token(&value, "clientToken")? != client_token {
        return Err(tr("Ely.by returned a different client token").into());
    }
    Ok(Credential {
        access_token: token(&value, "accessToken")?,
        client_token,
        selected_profile: read_profile(&value["selectedProfile"])?,
    })
}

fn renewed(reply: Reply, mut old: Credential) -> Result<Credential, String> {
    let Reply::Ok(value) = reply else {
        return Err(tr("Ely.by session expired. Run ely-account sign-in again.").into());
    };
    if value.get("clientToken").is_some() && token(&value, "clientToken")? != old.client_token {
        return Err(tr("Ely.by returned a different client token").into());
    }
    if let Some(profile) = value.get("selectedProfile") {
        let profile = read_profile(profile)?;
        if profile.identity() != old.selected_profile.identity() {
            return Err(tr("Ely.by refresh changed the player identity").into());
        }
        old.selected_profile = profile;
    }
    old.access_token = token(&value, "accessToken")?;
    Ok(old)
}

fn load(store: &dyn SecretStore) -> Result<Option<Credential>, String> {
    store
        .get(ENTRY)?
        .map(|saved| {
            let invalid = || tr("Ely.by credential is invalid. Run ely-account sign-in again.");
            let credential: Credential = serde_json::from_str(&saved).map_err(|_| invalid())?;
            if credential.access_token.is_empty()
                || credential.client_token.is_empty()
                || credential.selected_profile.identity().is_none()
            {
                return Err(invalid().into());
            }
            Ok(credential)
        })
        .transpose()
}

fn save(store: &dyn SecretStore, credential: &Credential) -> Result<(), String> {
    let secret =
        serde_json::to_string(credential).map_err(|_| tr("Could not encode Ely.by credentials"))?;
    store.set(ENTRY, &secret)
}

fn prompt(label: &str) -> Result<String, String> {
    eprint!("{label}");
    io::stderr().flush().map_err(|e| e.to_string())?;
    let mut text = String::new();
    if io::stdin()
        .read_line(&mut text)
        .map_err(|e| e.to_string())?
        == 0
    {
        return Err(tr("Input closed").into());
    }
    while text.ends_with(['\r', '\n']) {
        text.pop();
    }
    if text.is_empty() {
        return Err(tr("Input was empty").into());
    }
    Ok(text)
}

#[cfg(unix)]
fn without_echo<T>(read: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    use std::mem::MaybeUninit;
    let mut saved = MaybeUninit::<libc::termios>::uninit();
    // tcgetattr initializes the termios on success. Both tcsetattr calls borrow live values.
    unsafe {
        if libc::tcgetattr(libc::STDIN_FILENO, saved.as_mut_ptr()) != 0 {
            return Err(CONSOLE_REQUIRED.into());
        }
        let saved = saved.assume_init();
        let mut hidden = saved;
        hidden.c_lflag &= !(libc::ECHO | libc::ECHONL);
        if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, &hidden) != 0 {
            return Err(tr("Could not hide terminal input").into());
        }
        let result = read();
        if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &saved) != 0 {
            return Err(tr("Could not restore terminal settings. Run stty sane.").into());
        }
        result
    }
}

#[cfg(windows)]
fn without_echo<T>(read: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    use windows_sys::Win32::System::Console::{
        GetConsoleMode, GetStdHandle, SetConsoleMode, ENABLE_ECHO_INPUT, STD_INPUT_HANDLE,
    };
    // Startup attaches the parent console before dispatch. The handle is borrowed.
    let input = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
    let mut saved = 0;
    if unsafe { GetConsoleMode(input, &mut saved) } == 0 {
        return Err(CONSOLE_REQUIRED.into());
    }
    if unsafe { SetConsoleMode(input, saved & !ENABLE_ECHO_INPUT) } == 0 {
        return Err(tr("Could not hide console input").into());
    }
    let result = read();
    if unsafe { SetConsoleMode(input, saved) } == 0 {
        return Err(tr("Could not restore console settings").into());
    }
    result
}

#[cfg(not(any(unix, windows)))]
fn without_echo<T>(_: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    Err(tr("Hidden input is unavailable on this platform").into())
}

fn secret(label: &str) -> Result<String, String> {
    let result = without_echo(|| prompt(label));
    eprintln!();
    result
}

pub enum SignInError {
    TotpRequired,
    InvalidTotp,
    Rejected,
    Unavailable,
    CredentialStore(String),
}

impl SignInError {
    fn cli_message(self) -> String {
        match self {
            Self::TotpRequired | Self::Rejected => tr("Ely.by sign-in was refused").into(),
            Self::InvalidTotp => tr("Enter a six-digit Ely.by TOTP code").into(),
            Self::Unavailable => tr("Ely.by authentication service unavailable").into(),
            Self::CredentialStore(error) => error,
        }
    }
}

/// One authentication attempt shared by the terminal and the desktop.
/// The retry token is generated by the caller once per sign-in flow.
pub struct SignInAttempt {
    client_token: String,
}

impl SignInAttempt {
    pub fn new() -> Result<Self, SignInError> {
        let mut random = [0; 16];
        getrandom::fill(&mut random).map_err(|_| SignInError::Unavailable)?;
        Ok(Self {
            client_token: random.iter().map(|b| format!("{b:02x}")).collect(),
        })
    }

    pub fn submit(
        &self,
        username: &str,
        password: &str,
        code: Option<&str>,
    ) -> Result<String, SignInError> {
        let password_with_code;
        let password = match code {
            None => password,
            Some(code) if code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit()) => {
                password_with_code = format!("{password}:{code}");
                &password_with_code
            }
            Some(_) => return Err(SignInError::InvalidTotp),
        };
        let reply = request(
            "authenticate",
            json!({
                "username": username.trim(),
                "password": password,
                "clientToken": self.client_token,
                "requestUser": false,
            }),
        )
        .map_err(|_| SignInError::Unavailable)?;
        let reply = match reply {
            Reply::Totp if code.is_none() => return Err(SignInError::TotpRequired),
            Reply::Totp => return Err(SignInError::InvalidTotp),
            Reply::Rejected => return Err(SignInError::Rejected),
            ok => ok,
        };
        let credential =
            signed_in(reply, self.client_token.clone()).map_err(|_| SignInError::Unavailable)?;
        let store = keychain_or_why().map_err(SignInError::CredentialStore)?;
        let _operation = session::lock(store.as_ref()).map_err(SignInError::CredentialStore)?;
        save(store.as_ref(), &credential).map_err(SignInError::CredentialStore)?;
        Ok(credential.selected_profile.name)
    }
}

pub fn sign_in() -> Result<String, String> {
    if !io::stdin().is_terminal() {
        return Err(CONSOLE_REQUIRED.into());
    }
    let username = prompt(tr("Ely.by name or email: "))?;
    if username.trim().is_empty() {
        return Err(tr("Enter an Ely.by name or email").into());
    }
    let password = secret(tr("Ely.by password: "))?;
    let attempt = SignInAttempt::new().map_err(SignInError::cli_message)?;
    match attempt.submit(&username, &password, None) {
        Err(SignInError::TotpRequired) => {
            let code = secret(tr("Ely.by TOTP code: "))?;
            attempt.submit(&username, &password, Some(&code))
        }
        result => result,
    }
    .map_err(SignInError::cli_message)
}

pub fn status() -> Result<Option<String>, String> {
    let store = keychain_or_why()?;
    let _operation = session::lock(store.as_ref())?;
    Ok(load(store.as_ref())?.map(|credential| credential.selected_profile.name))
}

pub fn sign_out() -> Result<(), String> {
    let store = keychain_or_why()?;
    let _operation = session::lock(store.as_ref())?;
    store.delete(ENTRY)
}

fn refresh() -> Result<Credential, String> {
    let store = keychain_or_why()?;
    let _operation = session::lock(store.as_ref())?;
    let old = load(store.as_ref())?.ok_or(tr("Run ely-account sign-in first"))?;
    let reply = request(
        "refresh",
        json!({
            "accessToken": old.access_token,
            "clientToken": old.client_token,
            "requestUser": false,
        }),
    )?;
    let credential = renewed(reply, old)?;
    save(store.as_ref(), &credential)?;
    Ok(credential)
}

#[cfg(test)]
#[test]
fn refresh_keeps_the_profile_and_rotates_only_the_token() {
    let login = Reply::Ok(json!({
        "clientToken": "client",
        "accessToken": "first",
        "selectedProfile": {"id":"069A79F4-44E9-4726-A5BE-FCA90E38AAF5","name":"Player"}
    }));
    let first = signed_in(login, "client".into()).unwrap();
    let updated = renewed(Reply::Ok(json!({"accessToken":"second"})), first).unwrap();
    assert_eq!(
        updated.game_session(),
        json!({
            "name": "Player",
            "uuid": "069a79f444e94726a5befca90e38aaf5",
            "access_token": "second",
            "user_type": "ely",
        })
    );
    let swapped = Reply::Ok(json!({
        "accessToken":"third",
        "selectedProfile": {"id":"169a79f444e94726a5befca90e38aaf5","name":"Player"}
    }));
    assert!(renewed(swapped, updated).is_err());
}
