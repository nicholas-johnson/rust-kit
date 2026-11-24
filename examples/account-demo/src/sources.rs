//! Data sources: a stable name, a typed view, and a resolver.
//!
//! The compiler calls these directly during server rendering. A source that
//! some page loads with `load="client"` also gets `GET /_rk/source/{name}`,
//! which runs the same resolver with the same session. Views serialize with
//! camelCase names, which is what templates read (`data.displayName`).

use rustkit::Error;
use serde::Serialize;

type Ctx = rustkit::RequestContext<crate::AppState>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    pub display_name: String,
}

pub async fn account(ctx: &Ctx) -> Result<AccountView, Error> {
    let user = ctx.require_user()?;
    let record = ctx.accounts_service.get(&user.id).await?;
    Ok(AccountView { display_name: record.display_name })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityView {
    pub items: Vec<ActivityItem>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityItem {
    pub description: String,
}

pub async fn activity(ctx: &Ctx) -> Result<ActivityView, Error> {
    let user = ctx.require_user()?;
    let records = ctx.accounts_service.activity(&user.id).await?;
    Ok(ActivityView { items: records.into_iter().map(|r| ActivityItem { description: r.description }).collect() })
}
