/// The server has no options, the notebook is the workspace folder of the editor
#[derive(clap::Args, Debug)]
pub(crate) struct Args {}

pub(crate) fn run(_args: &Args) -> anyhow::Result<()> {
    crate::lsp::server::run()
}
