//! Form actions. The framework has already checked origin and CSRF token.

use rustkit::{Error, Form, Redirect};

type Ctx = rustkit::RequestContext<crate::AppState>;

pub async fn update_profile(ctx: Ctx, form: Form) -> Result<Redirect, Error> {
    let user = ctx.require_user()?;
    let name = form.get("name").unwrap_or("").trim();
    let len = name.chars().count();
    if len == 0 || len > 60 || name.chars().any(char::is_control) {
        return Err(Error::BadRequest("Display name must be 1 to 60 characters.".into()));
    }
    ctx.accounts_service.update_display_name(&user.id, name).await?;
    Ok(Redirect::to("/account"))
}

/// Demo sign-in: picks one of the seeded accounts. A real app would verify a
/// credential or complete an identity-provider flow here.
pub async fn login(ctx: Ctx, form: Form) -> Result<Redirect, Error> {
    match form.get("user") {
        Some(id @ ("alice" | "bob")) => {
            ctx.login(id);
            Ok(Redirect::to("/account"))
        }
        _ => Err(Error::BadRequest("Choose one of the demo accounts.".into())),
    }
}

pub async fn logout(ctx: Ctx, _form: Form) -> Result<Redirect, Error> {
    ctx.logout();
    Ok(Redirect::to("/"))
}
