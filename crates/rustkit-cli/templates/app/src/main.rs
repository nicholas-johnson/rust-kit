use std::path::PathBuf;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        // `rustkit build [dir]`: write the prerendered pages and assets.
        Some("build") => {
            let out = PathBuf::from(args.get(1).map_or("dist", String::as_str));
            let app = {{crate}}::app(true);
            let report = app.build(&out).await.expect("build failed");
            println!("{:<12} {:<8} {:>10} {:>10}", "route", "mode", "JS bytes", "CSS bytes");
            for r in app.routes() {
                let mode = if r.prerender { "static" } else { "request" };
                println!("{:<12} {:<8} {:>10} {:>10}", r.path, mode, r.js_bytes, r.css_bytes);
            }
            println!("\nwrote {} files to {}", report.files.len(), out.display());
        }
        // `rustkit serve [addr]`, and what `rustkit dev` runs.
        Some("serve") | None => {
            let addr = args.get(1).map_or("127.0.0.1:3000", String::as_str);
            // Under `rustkit dev` this adopts the dev server's socket instead.
            let listener = rustkit::dev::listener(addr).await.expect("bind");
            let local = listener.local_addr().expect("local address");
            // Sessions survive a restart. `rustkit dev` keeps its own file instead.
            let app = {{crate}}::app(!local.ip().is_loopback()).session_store(rustkit::SessionStore::persistent("sessions.json"));
            let router = app.into_router().await.expect("prerender failed");
            println!("listening on http://{local}");
            axum::serve(listener, router).await.unwrap();
        }
        Some(other) => {
            eprintln!("unknown command `{other}`; use `serve [addr]` or `build [dir]`");
            std::process::exit(2);
        }
    }
}
