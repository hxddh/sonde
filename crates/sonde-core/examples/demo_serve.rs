//! The built-in demo store, left running: its URL on stdout, until killed.
//! For recording a real program against it (`sonde --url <it> run -- …`)
//! when making a fixture, with no store to install.
//!
//!     cargo run -p sonde-core --example demo_serve

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let server = sonde_core::demo::start().await?;
    println!("{}", server.url());
    std::future::pending::<()>().await;
    Ok(())
}
