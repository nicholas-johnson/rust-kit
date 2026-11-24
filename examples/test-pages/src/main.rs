#[tokio::main]
async fn main() {
    let addr = std::env::args().nth(1).unwrap_or_else(|| "127.0.0.1:3918".into());
    let router = test_pages::app(false).into_router().await.expect("prerender failed");
    let listener = tokio::net::TcpListener::bind(&addr).await.expect("bind");
    println!("test pages on http://{addr}");
    axum::serve(listener, router).await.unwrap();
}
