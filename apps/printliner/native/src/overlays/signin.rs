//! Sign In: the region, email, and password, then the emailed code when the
//! account asks for one.

use laankit_egui::dialog::{self, ButtonKind, Dialog};
use laankit_egui::settings;
use printliner_core::Region;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Credentials,
    Code,
    /// The account uses an authenticator app.
    Authenticator,
}

pub struct SignInState {
    pub region: Region,
    pub email: String,
    pub password: String,
    pub code: String,
    pub step: Step,
    /// Waiting for Bambu Lab.
    pub busy: bool,
    pub error: Option<String>,
}

impl SignInState {
    pub fn new(region: Region, email: Option<String>) -> Self {
        Self {
            region,
            email: email.unwrap_or_default(),
            password: String::new(),
            code: String::new(),
            step: Step::Credentials,
            busy: false,
            error: None,
        }
    }

    pub fn fail(&mut self, message: String) {
        self.busy = false;
        self.error = Some(message);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SignInAction {
    Close,
    Submit {
        region: Region,
        email: String,
        password: String,
    },
    /// Skip the password: email a code.
    CodeOnly {
        region: Region,
        email: String,
    },
    SubmitCode(String),
    Resend,
}

pub fn show(ctx: &egui::Context, state: &mut SignInState) -> Option<SignInAction> {
    let resp = Dialog::compact("printliner-signin", "Sign In to Bambu Lab").show(ctx, |ui| {
        match state.step {
            Step::Credentials => credentials(ui, state),
            Step::Code => code(ui, state),
            Step::Authenticator => authenticator(ui),
        }
    });
    if resp.close {
        return Some(SignInAction::Close);
    }
    resp.inner
}

fn credentials(ui: &mut egui::Ui, state: &mut SignInState) -> Option<SignInAction> {
    let mut out = None;
    let enabled = !state.busy;
    dialog::field_label(ui, "Region");
    let options: Vec<(&str, &str)> = Region::ALL.iter().map(|r| (r.id(), r.label())).collect();
    if let Some(r) = settings::select(ui, "Region", &options, state.region.id())
        .and_then(|id| Region::parse(&id))
    {
        state.region = r;
    }
    ui.add_space(8.0);
    dialog::field_label(ui, "Email");
    let email = dialog::text_input(ui, &mut state.email, "you@example.com", enabled);
    if state.email.is_empty() && !state.busy {
        email.request_focus();
    }
    ui.add_space(8.0);
    dialog::field_label(ui, "Password");
    let password = ui.add_enabled(
        enabled,
        egui::TextEdit::singleline(&mut state.password)
            .password(true)
            .desired_width(f32::INFINITY)
            .margin(egui::Margin::symmetric(12, 8)),
    );
    if !state.email.is_empty()
        && state.password.is_empty()
        && !state.busy
        && !password.has_focus()
        && !email.has_focus()
    {
        password.request_focus();
    }
    if let Some(e) = &state.error {
        ui.add_space(6.0);
        dialog::error_text(ui, e);
    }
    ui.add_space(6.0);
    dialog::muted_text(
        ui,
        "Your password is sent only to Bambu Lab and is never saved. PrintLiner keeps the sign-in token in your system keychain. Signed up with Google or Apple? Leave the password empty and use Email Me a Code Instead.",
    );
    let ready = enabled && !state.email.trim().is_empty() && !state.password.is_empty();
    let enter = dialog::submitted(ui, &password) || dialog::submitted(ui, &email);
    // No password (Google or Apple sign-in): an emailed code alone.
    ui.add_space(8.0);
    let has_email = enabled && !state.email.trim().is_empty();
    if dialog::button(
        ui,
        "Email Me a Code Instead",
        ButtonKind::Secondary,
        has_email,
    )
    .clicked()
    {
        out = Some(SignInAction::CodeOnly {
            region: state.region,
            email: state.email.trim().to_string(),
        });
    }
    dialog::actions(ui, |ui| {
        let label = if state.busy {
            "Signing In…"
        } else {
            "Sign In"
        };
        if dialog::button(ui, label, ButtonKind::Primary, ready).clicked() || (enter && ready) {
            out = Some(SignInAction::Submit {
                region: state.region,
                email: state.email.trim().to_string(),
                password: state.password.clone(),
            });
        }
        if dialog::button(ui, "Cancel", ButtonKind::Secondary, true).clicked() {
            out = Some(SignInAction::Close);
        }
    });
    out
}

fn code(ui: &mut egui::Ui, state: &mut SignInState) -> Option<SignInAction> {
    let mut out = None;
    let enabled = !state.busy;
    ui.label(format!(
        "Bambu Lab sent a sign-in code to {}. Enter it here.",
        state.email
    ));
    ui.add_space(8.0);
    dialog::field_label(ui, "Code");
    let input = dialog::text_input(ui, &mut state.code, "123456", enabled);
    if state.code.is_empty() && enabled {
        input.request_focus();
    }
    if let Some(e) = &state.error {
        ui.add_space(6.0);
        dialog::error_text(ui, e);
    }
    let ready = enabled && !state.code.trim().is_empty();
    let enter = dialog::submitted(ui, &input);
    dialog::actions(ui, |ui| {
        let label = if state.busy { "Checking…" } else { "Sign In" };
        if dialog::button(ui, label, ButtonKind::Primary, ready).clicked() || (enter && ready) {
            out = Some(SignInAction::SubmitCode(state.code.trim().to_string()));
        }
        if dialog::button(ui, "Send a New Code", ButtonKind::Secondary, enabled).clicked() {
            out = Some(SignInAction::Resend);
        }
    });
    out
}

fn authenticator(ui: &mut egui::Ui) -> Option<SignInAction> {
    let mut out = None;
    ui.label(
        "This account signs in with an authenticator app, which PrintLiner doesn't support yet.",
    );
    ui.add_space(6.0);
    dialog::muted_text(
        ui,
        "Accounts that confirm sign-ins with an emailed code work. Support for authenticator apps is planned.",
    );
    dialog::actions(ui, |ui| {
        if dialog::button(ui, "Close", ButtonKind::Primary, true).clicked() {
            out = Some(SignInAction::Close);
        }
    });
    out
}
