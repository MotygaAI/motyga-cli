#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    motyga_code_mode_host::run_stdio().await
}
