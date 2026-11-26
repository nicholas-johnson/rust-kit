//! Data sources. A page imports one with `import { greeting } from "$sources";`
//! and loads it with `<DataSource source={greeting} load="server">`. The
//! generated code calls the function of the same name here.

use rustkit::Error;
use serde::Serialize;

type Ctx = rustkit::RequestContext<crate::AppState>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GreetingView {
    pub app_name: String,
    pub message: String,
}

pub async fn greeting(ctx: &Ctx) -> Result<GreetingView, Error> {
    Ok(GreetingView { app_name: ctx.app_name.clone(), message: "Hello from a Rust resolver.".into() })
}
