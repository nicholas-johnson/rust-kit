//! Public sources with deliberately awkward values: markup, quotes, emoji,
//! floating-point edge cases, empty strings, `None`, and strings whose order
//! differs between UTF-8 bytes and UTF-16 code units.

use rustkit::Error;
use serde::Serialize;

type Ctx = rustkit::RequestContext<crate::AppState>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SamplesView {
    pub markup: String,
    pub quotes: String,
    pub emoji: String,
    pub empty: String,
    pub zero_text: String,
    pub ten_text: String,
    pub nine_text: String,
    pub upper: String,
    pub lower: String,
    pub accented: String,
    pub crab: String,
    /// U+FF61 sorts after the crab in UTF-8 but before it in UTF-16.
    pub halfwidth: String,
    pub sum: f64,
    pub big: f64,
    pub tiny: f64,
    pub small: f64,
    pub neg_zero: f64,
    pub half: f64,
    /// JSON cannot represent these, so both renderers show them as null.
    pub not_a_number: f64,
    pub infinite: f64,
    /// Travels as the widened f64 `0.10000000149011612`; both sides render that.
    pub tenth: f32,
    pub count: i64,
    pub zero: i64,
    pub negative: i32,
    pub yes: bool,
    pub no: bool,
    pub nothing: Option<String>,
    pub something: Option<String>,
    pub no_number: Option<i64>,
}

pub async fn samples(_ctx: &Ctx) -> Result<SamplesView, Error> {
    Ok(SamplesView {
        markup: r#"<script>alert("hi") & 'bye'</script>"#.into(),
        quotes: r#"She said "yes" & he said 'no'"#.into(),
        emoji: "café 🦀".into(),
        empty: String::new(),
        zero_text: "0".into(),
        ten_text: "10".into(),
        nine_text: "9".into(),
        upper: "Z".into(),
        lower: "a".into(),
        accented: "é".into(),
        crab: "🦀".into(),
        halfwidth: "\u{FF61}".into(),
        sum: 0.1 + 0.2,
        big: 1e21,
        tiny: 1.5e-7,
        small: 0.000001,
        neg_zero: -0.0,
        half: 0.5,
        not_a_number: f64::NAN,
        infinite: f64::INFINITY,
        tenth: 0.1,
        count: 123_456_789,
        zero: 0,
        negative: -42,
        yes: true,
        no: false,
        nothing: None,
        something: Some("here".into()),
        no_number: None,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub name: String,
    pub price: f64,
    pub tags: Vec<String>,
    pub note: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemsView {
    pub items: Vec<Item>,
    pub none: Vec<Item>,
}

fn items_view() -> ItemsView {
    let item = |name: &str, price: f64, tags: &[&str], note: Option<&str>| Item {
        name: name.into(),
        price,
        tags: tags.iter().map(|t| (*t).to_owned()).collect(),
        note: note.map(str::to_owned),
    };
    ItemsView {
        items: vec![
            item("Wrench <M8>", 12.5, &["tools", "metal"], Some("Fits \"most\" nuts")),
            item("Rope & pulley", 0.1 + 0.2, &[], None),
            item("Crab 🦀 figurine", 1e21, &["toys"], Some("")),
        ],
        none: vec![],
    }
}

/// Loaded on the server by `list-server.rk`.
pub async fn items(_ctx: &Ctx) -> Result<ItemsView, Error> {
    Ok(items_view())
}

/// The same data, loaded in the browser by `list-client.rk`.
pub async fn items_live(_ctx: &Ctx) -> Result<ItemsView, Error> {
    Ok(items_view())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageView {
    pub text: String,
    pub has_text: bool,
}

pub async fn message(ctx: &Ctx) -> Result<MessageView, Error> {
    let text = ctx.last_message.lock().unwrap().clone();
    Ok(MessageView { has_text: !text.is_empty(), text })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostView {
    pub title: String,
    pub body: String,
}

fn post_view(slug: &str) -> PostView {
    match slug {
        "hello" => PostView {
            title: "Hello".into(),
            body: "The first post.".into(),
        },
        "notes" => PostView {
            title: "Notes & drafts".into(),
            body: "A <tag> & more".into(),
        },
        other => PostView {
            title: format!("Unknown {other}"),
            body: format!("No post named {other}."),
        },
    }
}

/// Loaded on the server by `blog/[slug]/index.rk`. Takes the route parameter
/// because every page that imports it sits on `[slug]`.
pub async fn post(_ctx: &Ctx, slug: &str) -> Result<PostView, Error> {
    Ok(post_view(slug))
}

/// The same post, loaded in the browser by `blog/[slug]/live.rk`.
pub async fn post_live(ctx: &Ctx, slug: &str) -> Result<PostView, Error> {
    post(ctx, slug).await
}

/// Returns unauthorized when the slug contains a space, so the redirect can
/// be checked with a concrete, encoded path.
pub async fn gate(ctx: &Ctx, slug: &str) -> Result<PostView, Error> {
    if slug.contains(' ') {
        return Err(Error::Unauthorized);
    }
    post(ctx, slug).await
}
