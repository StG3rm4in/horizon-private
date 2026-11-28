//! The desktop form and Play controls for the separate Ely.by credential slot.
use super::*;
use horizon_launcher::account::ely::{self, SignInAttempt, SignInError};
use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, TryRecvError},
};

const STORE_ERROR: &str = "Impossible d’accéder au compte Ely.by dans le coffre du système.";
const MISSING_GAME: &str = "Fichiers de Minecraft 26.3 introuvables ou incomplets. \
    Lancez une fois l’instance Noelcraft (ou Minecraft 26.3) dans Prism Launcher \
    ou dans le lanceur officiel, puis réessayez.";
const SERVER_ERROR: &str = "Adresse du serveur Noelcraft absente ou invalide dans noelcraft.json.";

type Notice = &'static str;

#[derive(Default)]
struct Form {
    open: bool,
    challenge: Option<SignInAttempt>,
    error: Option<Notice>,
}

impl Form {
    fn needs_totp(&self) -> bool {
        self.open && self.challenge.is_some()
    }

    fn refused(&mut self, attempt: Option<SignInAttempt>, error: SignInError) {
        if self.open {
            self.error = Some(sign_in_error(
                &error,
                self.needs_totp() || attempt.is_some(),
            ));
            self.challenge = attempt;
        }
    }
}

fn sign_in_error(error: &SignInError, retry: bool) -> Notice {
    match error {
        SignInError::TotpRequired => "Ely.by demande un code à six chiffres. Ressaisissez votre mot de passe et entrez le code.",
        SignInError::InvalidTotp => "Entrez un code Ely.by valide à six chiffres.",
        SignInError::Rejected if retry => "Mot de passe ou code Ely.by incorrect.",
        SignInError::Rejected => "Pseudo, adresse e-mail ou mot de passe Ely.by incorrect.",
        SignInError::Unavailable => "Impossible de joindre Ely.by. Réessayez dans un instant.",
        SignInError::CredentialStore(_) => STORE_ERROR,
    }
}

#[derive(Clone, Copy)]
enum Field {
    Name = 0,
    Password = 1,
    Code = 2,
}

impl Field {
    fn label(self) -> &'static str {
        match self {
            Self::Name => "Pseudo ou e-mail",
            Self::Password => "Mot de passe",
            Self::Code => "Code à six chiffres",
        }
    }

    fn masked(self) -> bool {
        !matches!(self, Self::Name)
    }
}

enum PlayResult {
    Done,
    Error(String),
    MissingGame(Option<PathBuf>),
}

enum Answer {
    Status(Result<Option<String>, String>),
    SignedIn(Result<String, SignInError>, Option<SignInAttempt>),
    SignedOut(Result<(), String>),
    Play(PlayResult),
}

pub(super) struct ElyUi {
    form: Form,
    fields: [line_edit::LineEdit; 3],
    focus: [FocusHandle; 3],
    focused: [bool; 3],
    pending: Option<Receiver<Answer>>,
    profile: Option<String>,
    pub(super) selected: bool,
    pub(super) playing: bool,
    account_error: Option<Notice>,
    play_error: Option<String>,
    prism: Option<PathBuf>,
}

impl ElyUi {
    pub(super) fn new(cx: &mut Context<Launcher>) -> Self {
        let mut state = Self {
            form: Form::default(),
            fields: [
                line_edit::LineEdit::new(String::new(), 320, 30),
                line_edit::LineEdit::private(1024, 30),
                line_edit::LineEdit::private(6, 6),
            ],
            focus: std::array::from_fn(|_| cx.focus_handle()),
            focused: [false; 3],
            pending: None,
            profile: None,
            selected: false,
            playing: false,
            account_error: None,
            play_error: None,
            prism: None,
        };
        state.start(|| Answer::Status(ely::status()));
        state
    }

    fn start(&mut self, job: impl FnOnce() -> Answer + Send + 'static) {
        debug_assert!(self.pending.is_none());
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(job());
        });
    }

    pub(super) fn note_focus(&mut self, window: &Window) {
        self.focused = std::array::from_fn(|i| self.focus[i].is_focused(window));
    }

    pub(super) fn profile_name(&self) -> Option<String> {
        self.profile.clone()
    }

    pub(super) fn close_form(&mut self) -> bool {
        let was_open = self.form.open;
        self.form = Form::default();
        for field in &mut self.fields {
            field.set(String::new());
        }
        was_open
    }

    fn submit(&mut self) {
        if self.pending.is_some() || !self.form.open {
            return;
        }
        let username = self.fields[Field::Name as usize].text().trim().to_owned();
        let password = self.fields[Field::Password as usize].take();
        let code = self.fields[Field::Code as usize].take();
        if username.is_empty() || password.is_empty() {
            self.form.error = Some("Entrez votre pseudo ou e-mail et votre mot de passe Ely.by.");
            return;
        }
        let retry = self.form.needs_totp();
        if retry && (code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit())) {
            self.form.error =
                Some("Entrez le code Ely.by à six chiffres et ressaisissez votre mot de passe.");
            return;
        }
        let attempt = self.form.challenge.take();
        self.form.error = None;
        self.start(move || {
            let attempt = match attempt.map_or_else(SignInAttempt::new, Ok) {
                Ok(attempt) => attempt,
                Err(error) => return Answer::SignedIn(Err(error), None),
            };
            let result = attempt.submit(&username, &password, retry.then_some(code.as_str()));
            let challenge = match &result {
                Err(SignInError::TotpRequired) => Some(attempt),
                Err(_) if retry => Some(attempt),
                _ => None,
            };
            Answer::SignedIn(result, challenge)
        });
    }

    pub(super) fn poll(&mut self) -> bool {
        let Some(pending) = &self.pending else {
            return false;
        };
        let answer = match pending.try_recv() {
            Ok(answer) => Some(answer),
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => None,
        };
        self.pending = None;
        match answer {
            Some(Answer::Status(Ok(profile))) => self.profile = profile,
            Some(Answer::SignedIn(Ok(name), _)) => {
                self.close_form();
                self.account_error = None;
                self.profile = Some(name);
            }
            Some(Answer::SignedIn(Err(error), attempt)) => self.form.refused(attempt, error),
            Some(Answer::SignedOut(Ok(()))) => {
                self.profile = None;
                self.selected = false;
                self.account_error = None;
            }
            Some(Answer::Status(Err(_)) | Answer::SignedOut(Err(_))) => {
                self.account_error = Some(STORE_ERROR);
            }
            Some(Answer::Play(result)) => {
                self.playing = false;
                match result {
                    PlayResult::Done => {}
                    PlayResult::Error(error) => self.play_error = Some(error),
                    PlayResult::MissingGame(prism) => {
                        self.play_error = Some(MISSING_GAME.into());
                        self.prism = prism;
                    }
                }
            }
            None if self.playing => {
                self.playing = false;
                self.play_error = Some("Le lancement de Noelcraft a été interrompu.".into());
            }
            None => self.account_error = Some("L’opération Ely.by a été interrompue."),
        }
        true
    }
}

fn preset_server(bundle: &Path) -> Result<String, Notice> {
    let text = fs::read_to_string(bundle.join("noelcraft.json")).map_err(|_| SERVER_ERROR)?;
    let config: serde_json::Value = serde_json::from_str(&text).map_err(|_| SERVER_ERROR)?;
    let address = config["server"].as_str().ok_or(SERVER_ERROR)?;
    ely::server_address(address.trim()).map_err(|_| SERVER_ERROR)
}

fn play_bundle() -> PlayResult {
    let bundle = match env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_owned))
    {
        Some(bundle) => bundle,
        None => return PlayResult::Error("Dossier de Noelcraft introuvable.".into()),
    };
    let server = match preset_server(&bundle) {
        Ok(server) => server,
        Err(error) => return PlayResult::Error(error.into()),
    };
    match ely::play(&bundle, None, None, None, &server) {
        Ok(0) => PlayResult::Done,
        Ok(code) => PlayResult::Error(format!("Noelcraft s’est arrêté avec le code {code}.")),
        Err(error) if ely::is_missing_game(&error) => PlayResult::MissingGame(prism_executable()),
        Err(error) => PlayResult::Error(format!("Impossible de lancer Noelcraft.\n{error}")),
    }
}

#[cfg(windows)]
fn prism_executable() -> Option<PathBuf> {
    use winreg::{enums::*, RegKey};

    const EXE: &str = "prismlauncher.exe";
    const APP_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\App Paths\prismlauncher.exe";
    let registered = [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE]
        .into_iter()
        .flat_map(|hive| {
            [
                KEY_READ,
                KEY_READ | KEY_WOW64_64KEY,
                KEY_READ | KEY_WOW64_32KEY,
            ]
            .into_iter()
            .filter_map(move |flags| {
                RegKey::predef(hive)
                    .open_subkey_with_flags(APP_PATH, flags)
                    .ok()?
                    .get_value::<String, _>("")
                    .ok()
                    .map(|value| PathBuf::from(value.trim().trim_matches('"')))
            })
        });
    let installed = [
        "ProgramW6432",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "LOCALAPPDATA",
    ]
    .into_iter()
    .filter_map(env::var_os)
    .flat_map(|base| {
        let base = PathBuf::from(base);
        [
            base.join("PrismLauncher").join(EXE),
            base.join("Programs/PrismLauncher").join(EXE),
        ]
    });
    let on_path = env::var_os("PATH").into_iter().flat_map(|path| {
        env::split_paths(&path)
            .map(|dir| dir.join(EXE))
            .collect::<Vec<_>>()
    });
    registered
        .chain(installed)
        .chain(on_path)
        .find(|path| path.is_absolute() && path.is_file())
}

#[cfg(target_os = "macos")]
fn prism_executable() -> Option<PathBuf> {
    let system = PathBuf::from("/Applications");
    std::iter::once(system)
        .chain(env::var_os("HOME").map(|home| PathBuf::from(home).join("Applications")))
        .map(|dir| dir.join("Prism Launcher.app/Contents/MacOS/prismlauncher"))
        .find(|path| path.is_absolute() && path.is_file())
}

#[cfg(not(any(windows, target_os = "macos")))]
fn prism_executable() -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|dir| dir.join("prismlauncher"))
        .find(|path| {
            path.is_absolute()
                && path
                    .metadata()
                    .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        })
}

impl Launcher {
    fn ely_can_start(&self) -> bool {
        self.ely_ui.pending.is_none()
            && self.client.is_none()
            && self.starting_client.is_none()
            && !self.preparing
            && !self.pre_launch_stoppable()
            && self.play_when_prepared.is_none()
    }

    pub(super) fn play_ely(&mut self) {
        if self.ely_can_start() && self.ely_ui.profile.is_some() {
            self.ely_ui.play_error = None;
            self.ely_ui.prism = None;
            self.ely_ui.playing = true;
            self.ely_ui.start(|| Answer::Play(play_bundle()));
        }
    }

    pub(super) fn ely_play_panel(&self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let play = if self.ely_ui.playing {
            div()
                .child("Noelcraft en cours de lancement ou ouvert…")
                .into_any_element()
        } else if self.ely_can_start() {
            button("ely-play", "Jouer", Kind::Primary, &t)
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.play();
                    cx.notify();
                }))
                .into_any_element()
        } else {
            div()
                .child("Une opération est en cours…")
                .into_any_element()
        };
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .p(px(20.))
            .child("Noelcraft · Minecraft 26.3")
            .child(self.ely_ui.profile.clone().unwrap_or_default())
            .child(div().flex().child(play))
            .children(
                self.ely_ui
                    .play_error
                    .clone()
                    .map(|error| div().text_color(t.danger).whitespace_normal().child(error)),
            )
            .children(self.ely_ui.prism.clone().map(|path| {
                div().flex().child(
                    button(
                        "ely-open-prism",
                        "Ouvrir Prism Launcher",
                        Kind::Secondary,
                        &t,
                    )
                    .on_click(cx.listener(
                        move |this, _: &ClickEvent, _, cx| {
                            match std::process::Command::new(&path).spawn() {
                                Ok(mut child) => {
                                    std::thread::spawn(move || {
                                        let _ = child.wait();
                                    });
                                }
                                Err(_) => {
                                    this.ely_ui.play_error = Some(format!(
                                        "{MISSING_GAME}\nImpossible d’ouvrir Prism Launcher."
                                    ));
                                }
                            }
                            cx.notify();
                        },
                    )),
                )
            }))
    }

    pub(super) fn ely_account_card(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let busy = self.ely_ui.pending.is_some();
        let mut card = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .p(px(16.))
            .rounded(px(theme::RADIUS))
            .border_1()
            .border_color(t.outline)
            .bg(t.well)
            .child(div().font_weight(FontWeight::SEMIBOLD).child("Ely.by"));
        if let Some(name) = self.ely_ui.profile.clone() {
            card = card.child(div().child(name));
            if self.ely_ui.selected {
                card = card.child("Compte sélectionné pour Noelcraft");
            }
            if !busy {
                let select = if self.ely_ui.selected {
                    Some(
                        button(
                            "ely-use-microsoft",
                            "Revenir au compte Microsoft",
                            Kind::Ghost,
                            &t,
                        )
                        .on_click(cx.listener(
                            |this, _: &ClickEvent, _, cx| {
                                this.ely_ui.selected = false;
                                cx.notify();
                            },
                        )),
                    )
                } else if self.ely_can_start() {
                    Some(
                        button("ely-select", "Jouer en tant que", Kind::Secondary, &t).on_click(
                            cx.listener(|this, _: &ClickEvent, _, cx| {
                                if this.ely_can_start() && this.ely_ui.profile.is_some() {
                                    this.ely_ui.selected = true;
                                }
                                cx.notify();
                            }),
                        ),
                    )
                } else {
                    None
                };
                card = card.children(select).child(
                    button("ely-sign-out", "Se déconnecter", Kind::Ghost, &t).on_click(
                        cx.listener(|this, _: &ClickEvent, _, cx| {
                            if this.ely_ui.pending.is_none() {
                                this.ely_ui.start(|| Answer::SignedOut(ely::sign_out()));
                            }
                            cx.notify();
                        }),
                    ),
                );
            }
        } else if self.ely_ui.form.open && !busy {
            card = card
                .child(self.ely_field(Field::Name, cx))
                .child(self.ely_field(Field::Password, cx));
            if self.ely_ui.form.needs_totp() {
                card = card.child(self.ely_field(Field::Code, cx));
            }
            card =
                card.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(8.))
                        .child(
                            button("ely-submit", "Se connecter", Kind::Primary, &t).on_click(
                                cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.ely_ui.submit();
                                    cx.notify();
                                }),
                            ),
                        )
                        .child(button("ely-cancel", "Annuler", Kind::Ghost, &t).on_click(
                            cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.ely_ui.close_form();
                                cx.notify();
                            }),
                        )),
                );
        } else if !busy {
            card = card.child(
                button(
                    "ely-open-form",
                    "Se connecter avec Ely.by",
                    Kind::Secondary,
                    &t,
                )
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.open_ely_form(window, cx);
                    cx.notify();
                })),
            );
        }
        card.children(busy.then(|| div().child("Opération Ely.by en cours…")))
            .children(
                self.ely_ui
                    .form
                    .error
                    .or(self.ely_ui.account_error)
                    .map(|error| div().text_color(t.danger).child(error)),
            )
    }

    pub(super) fn open_ely_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ely_ui.pending.is_none() && self.ely_ui.profile.is_none() {
            self.ely_ui.account_error = None;
            self.ely_ui.form.open = true;
            window.focus(&self.ely_ui.focus[Field::Name as usize], cx);
        }
    }

    fn ely_field(&mut self, field: Field, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let index = field as usize;
        let focused = self.ely_ui.focused[index];
        let control = div()
            .id(match field {
                Field::Name => "ely-name",
                Field::Password => "ely-password",
                Field::Code => "ely-totp",
            })
            .track_focus(&self.ely_ui.focus[index])
            .tab_index(0)
            .flex()
            .items_center()
            .w_full()
            .h(px(32.))
            .px(px(10.))
            .overflow_hidden()
            .border_2()
            .border_color(if focused { t.accent } else { t.outline })
            .bg(t.well)
            .cursor_text()
            .font_family(theme::MONO)
            .text_size(px(13.))
            .child(line_edit::content_with(
                &self.ely_ui.fields[index],
                &t,
                focused,
                false,
                field.masked(),
                field.label().into(),
            ))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                window.focus(&this.ely_ui.focus[index], cx);
                cx.notify();
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                match line_edit::handle(&mut this.ely_ui.fields[index], event, cx) {
                    line_edit::Key::Pass => return,
                    line_edit::Key::Enter => this.ely_ui.submit(),
                    line_edit::Key::Escape => {
                        this.ely_ui.close_form();
                    }
                    line_edit::Key::Edited if matches!(field, Field::Name) => {
                        this.ely_ui.form.challenge = None;
                        this.ely_ui.form.error = None;
                        this.ely_ui.fields[Field::Code as usize].set(String::new());
                    }
                    _ => {}
                }
                cx.stop_propagation();
                cx.notify();
            }));
        let control = line_edit::mouse(control, cx, move |this| &mut this.ely_ui.fields[index]);
        let control = if t.glass {
            glass_ui::wide_field(control, focused)
        } else {
            control
        };
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(field.label())
            .child(control)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn totp_is_visible_only_after_a_challenge_for_the_open_form() {
        let mut form = Form {
            open: true,
            ..Form::default()
        };
        assert!(!form.needs_totp());
        form.refused(None, SignInError::Rejected);
        assert!(!form.needs_totp());
        let attempt = SignInAttempt::new().ok().expect("OS randomness");
        form.refused(Some(attempt), SignInError::TotpRequired);
        assert!(form.needs_totp());
        assert!(form.error.unwrap().contains("six chiffres"));
        form.open = false;
        assert!(!form.needs_totp());
        form = Form::default();
        form.refused(SignInAttempt::new().ok(), SignInError::TotpRequired);
        assert!(form.challenge.is_none());
        assert!(form.error.is_none());
    }

    #[test]
    fn sign_in_errors_are_french_and_do_not_expose_store_details() {
        assert!(sign_in_error(&SignInError::Rejected, false).contains("mot de passe"));
        assert!(sign_in_error(&SignInError::Rejected, true).contains("ou code"));
        assert!(sign_in_error(&SignInError::Unavailable, false).contains("joindre Ely.by"));
        assert_eq!(
            sign_in_error(&SignInError::CredentialStore("detail".into()), false),
            STORE_ERROR
        );
    }

    #[test]
    fn bundle_server_is_read_and_validated() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("noelcraft.json");
        assert_eq!(preset_server(temp.path()), Err(SERVER_ERROR));
        for invalid in ["{", r#"{"server":42}"#, r#"{"server":"example.org:0"}"#] {
            fs::write(&config, invalid).unwrap();
            assert_eq!(preset_server(temp.path()), Err(SERVER_ERROR));
        }
        for address in ["example.org:25565", "[::1]:25565"] {
            fs::write(&config, serde_json::json!({"server": address}).to_string()).unwrap();
            assert_eq!(preset_server(temp.path()).unwrap(), address);
        }
    }

    #[test]
    fn private_fields_cannot_copy_or_recover_submitted_text() {
        let mut field = line_edit::LineEdit::private(32, 20);
        field.insert("secret");
        field.select_all();
        assert_eq!(field.key("c", None, true, false), line_edit::Key::Moved);
        assert_eq!(field.key("x", None, true, false), line_edit::Key::Moved);
        assert_eq!(field.take(), "secret");
        field.insert("other");
        field.key("z", None, true, false);
        assert_eq!(field.text(), "other");
    }
}
